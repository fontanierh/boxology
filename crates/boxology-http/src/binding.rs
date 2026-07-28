//! The public HTTP server binding and the HTTP/1 serve-over-socket core that
//! frames real connections onto the tested request codec in [`crate::server`].

use std::{
    collections::BTreeSet,
    fmt,
    net::SocketAddr,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use boxology_contract::{CapabilityDescriptor, Detail, ExposureLevel};
use boxology_runtime::{
    TransportBinding, TransportExposure, TransportHandle, TransportJoinFuture, TransportRuntime,
};
use http::Request;
use hyper::{body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::conformance::conform_capability;
use crate::server::{DispatchTasks, handle_request};
use crate::syntax::{DEFAULT_DEPTH_LIMIT, SyntaxLimits};

/// The deadline applied to a request that carries no explicit timeout header.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The inclusive request body cap, mirroring the client's response cap.
const DEFAULT_MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
/// The default complete HTTP/1 request-head cap, including its request line.
const DEFAULT_MAX_REQUEST_HEAD_BYTES: usize = 16 * 1024;
/// Hyper's minimum accepted `max_buf_size`; lower configured values use this.
const MIN_MAX_REQUEST_HEAD_BYTES: usize = 8 * 1024;
/// The default time allowed to receive one complete HTTP/1 request head.
const DEFAULT_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for an HTTP server binding.
#[derive(Clone)]
pub struct HttpServerConfig {
    bind_addr: SocketAddr,
    default_timeout: Duration,
    limits: SyntaxLimits,
    max_request_head_bytes: usize,
    header_read_timeout: Duration,
}

impl HttpServerConfig {
    /// Creates configuration accepting connections on `bind_addr`.
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            default_timeout: DEFAULT_REQUEST_TIMEOUT,
            limits: SyntaxLimits(DEFAULT_MAX_BODY_BYTES, DEFAULT_DEPTH_LIMIT),
            max_request_head_bytes: DEFAULT_MAX_REQUEST_HEAD_BYTES,
            header_read_timeout: DEFAULT_HEADER_READ_TIMEOUT,
        }
    }

    /// Replaces the default deadline and maximum accepted client deadline.
    pub fn with_default_timeout(mut self, default_timeout: Duration) -> Self {
        self.default_timeout = default_timeout;
        self
    }

    /// Replaces the inclusive request byte and syntax-depth limits.
    pub fn with_request_limits(mut self, max_body_bytes: usize, max_decode_depth: usize) -> Self {
        self.limits = SyntaxLimits(max_body_bytes, max_decode_depth);
        self
    }

    /// Replaces the complete HTTP/1 request-head cap, including the request
    /// line and all header fields. Values below Hyper's 8192-byte minimum are
    /// silently floored to 8192 rather than panicking in Hyper's builder.
    pub fn with_max_request_head_bytes(mut self, max_request_head_bytes: usize) -> Self {
        self.max_request_head_bytes = max_request_head_bytes.max(MIN_MAX_REQUEST_HEAD_BYTES);
        self
    }

    /// Replaces the timeout for receiving a complete HTTP/1 request head.
    pub fn with_header_read_timeout(mut self, header_read_timeout: Duration) -> Self {
        self.header_read_timeout = header_read_timeout;
        self
    }
}

/// A transport binding serving composed exposures over HTTP/1.
/// [`TransportBinding::start`] requires an ambient Tokio runtime: it registers
/// the bound listener with the reactor and the accept driver on the tracker.
pub struct HttpServerBinding {
    config: Arc<HttpServerConfig>,
    bound: Mutex<Option<SocketAddr>>,
}

impl HttpServerBinding {
    /// Constructs an unstarted binding retaining `config`.
    pub fn new(config: HttpServerConfig) -> Self {
        Self {
            config: Arc::new(config),
            bound: Mutex::new(None),
        }
    }

    /// Returns the resolved listening address once startup has bound a socket.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        *self.bound.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The composition-owned lifecycle handle of a started [`HttpServerBinding`].
/// `intake` closes admission and lets live connections finish gracefully;
/// `abort` is the hard stop that drops connections still parked on a client
/// once the composition's drain and cancellation grace have both expired.
pub struct HttpServerHandle {
    intake: CancellationToken,
    abort: CancellationToken,
    tasks: DispatchTasks,
    accept: Mutex<Option<JoinHandle<()>>>,
    connections: ConnectionTasks,
}

impl HttpServerHandle {
    /// Locks the accept-driver slot, tolerating a poisoned lock.
    fn accept_slot(&self) -> MutexGuard<'_, Option<JoinHandle<()>>> {
        self.accept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The per-binding registry of accepted-connection tasks. Joining every
/// transport-owned task is the handle's contract, and the shared composition
/// tracker cannot serve that purpose: awaiting it would block this binding on
/// every *other* binding's tasks. So the accept loop hands each connection
/// handle here instead, and [`HttpServerHandle::join_tasks`] drains it.
#[derive(Clone, Default)]
pub(crate) struct ConnectionTasks(Arc<Mutex<Vec<JoinHandle<()>>>>);

impl ConnectionTasks {
    /// Registers `handle`, first dropping the handles of connections that have
    /// already finished. A long-lived server accepts unboundedly many
    /// connections, so retaining them all would leak; `is_finished` is one
    /// atomic load, which keeps the sweep proportional to the registry's
    /// current size rather than to the lifetime connection count.
    pub(crate) fn register(&self, handle: JoinHandle<()>) {
        let mut registered = self.lock();
        registered.retain(|connection| !connection.is_finished());
        registered.push(handle);
    }

    /// Takes every registered handle, leaving the registry empty.
    fn take(&self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut *self.lock())
    }

    /// Aborts every registered connection without consuming its join handle.
    fn abort_all(&self) {
        for connection in self.lock().iter() {
            connection.abort();
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<JoinHandle<()>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn bind_failure(error: std::io::Error) -> Detail {
    Detail::new("http_bind").with_message(error.to_string())
}

impl TransportBinding for HttpServerBinding {
    type Config = HttpServerConfig;
    type Handle = HttpServerHandle;

    fn config(&self) -> Arc<HttpServerConfig> {
        self.config.clone()
    }

    fn conform(
        &self,
        descriptor: &CapabilityDescriptor,
        level: ExposureLevel,
    ) -> Result<(), Detail> {
        if level == ExposureLevel::CodeOnly {
            return Err(Detail::new("http_code_only_exposure")
                .with_message("code-only capabilities never cross an HTTP boundary"));
        }
        conform_capability(descriptor)
    }

    fn prepare(&self, descriptors: &[&'static CapabilityDescriptor]) -> Result<(), Detail> {
        let mut seen = BTreeSet::new();
        for descriptor in descriptors {
            conform_capability(descriptor)?;
            if !seen.insert(descriptor.id()) {
                return Err(Detail::new("http_duplicate_capability")
                    .with_message("HTTP routes each capability identifier once"));
            }
        }
        Ok(())
    }

    fn start(
        &self,
        runtime: TransportRuntime<HttpServerConfig>,
    ) -> Result<HttpServerHandle, Detail> {
        // A `start` outside a Tokio runtime panics inside this guard, so the
        // probe reads recover the inner value rather than propagating poison.
        let listener = {
            let mut bound = self.bound.lock().unwrap_or_else(PoisonError::into_inner);
            if bound.is_some() {
                return Err(Detail::new("http_server_already_started"));
            }
            let socket =
                std::net::TcpListener::bind(self.config.bind_addr).map_err(bind_failure)?;
            socket.set_nonblocking(true).map_err(bind_failure)?;
            let listener = TcpListener::from_std(socket).map_err(bind_failure)?;
            *bound = Some(listener.local_addr().map_err(bind_failure)?);
            listener
        };

        let (intake, abort) = (CancellationToken::new(), CancellationToken::new());
        let tracker = runtime.tracker().clone();
        let tasks = DispatchTasks::new(tracker.clone());
        let config = self.config.clone();
        let connections = ConnectionTasks::default();
        let accept = tracker.spawn({
            let (intake, abort, tasks) = (intake.clone(), abort.clone(), tasks.clone());
            let connections = connections.clone();
            async move {
                tokio::select! {
                    () = runtime.wait_until_active() => {}
                    () = intake.cancelled() => return,
                }
                serve(
                    listener,
                    ConnectionContext {
                        exposures: runtime.exposures().iter().cloned().collect(),
                        tasks,
                        default_timeout: config.default_timeout,
                        limits: config.limits,
                        max_request_head_bytes: config.max_request_head_bytes,
                        header_read_timeout: config.header_read_timeout,
                        shutdown: intake,
                        abort,
                    },
                    connections,
                )
                .await;
            }
        });
        Ok(HttpServerHandle {
            intake,
            abort,
            tasks,
            accept: Mutex::new(Some(accept)),
            connections,
        })
    }
}

impl TransportHandle for HttpServerHandle {
    fn stop_intake(&self) {
        self.intake.cancel();
    }

    fn cancel_tasks(&self) {
        self.tasks.cancel_all();
    }

    fn abort_tasks(&self) {
        self.abort.cancel();
        self.tasks.abort_all();
        self.connections.abort_all();
        if let Some(accept) = self.accept_slot().as_ref() {
            accept.abort();
        }
    }

    fn join_tasks(self: Box<Self>) -> TransportJoinFuture {
        let accept = self.accept_slot().take();
        Box::pin(async move {
            // A faulted accept task is latched rather than returned, so the
            // connection registry is still drained below; returning here would
            // detach every connection task, which is the defect this join
            // exists to prevent.
            let mut failure = None;
            if let Some(accept) = accept
                && let Err(error) = accept.await
                && !error.is_cancelled()
            {
                failure = Some(Detail::new("http_accept_failed").with_message(error.to_string()));
            }
            // The accept loop has stopped, so no further connection can
            // register: draining the registry now joins the whole set. A
            // cancelled join is this handle's own `abort_tasks`, not a fault,
            // and every handle is joined before the first fault is reported.
            for connection in self.connections.take() {
                if let Err(error) = connection.await
                    && !error.is_cancelled()
                    && failure.is_none()
                {
                    failure =
                        Some(Detail::new("http_connection_failed").with_message(error.to_string()));
                }
            }
            self.tasks.wait_empty().await;
            failure.map_or(Ok(()), Err)
        })
    }
}

/// A service error that instructs hyper to drop a connection without writing a
/// response, matching the "abandoned request" discard posture of the codec.
#[derive(Debug)]
struct Abandoned;

impl fmt::Display for Abandoned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("request abandoned")
    }
}

impl std::error::Error for Abandoned {}

/// Everything an accepted connection needs, bundled so that it is cloned in one
/// place and so that [`serve`] stays within the argument budget.
#[derive(Clone)]
pub(crate) struct ConnectionContext {
    exposures: Arc<[TransportExposure]>,
    tasks: DispatchTasks,
    default_timeout: Duration,
    limits: SyntaxLimits,
    max_request_head_bytes: usize,
    header_read_timeout: Duration,
    shutdown: CancellationToken,
    abort: CancellationToken,
}

/// Accepts connections until `shutdown` is triggered, dispatching each request
/// through the shared codec. The tracker behind `tasks` owns both the accepted
/// connection tasks and the per-request dispatch tasks, `connections` retains
/// each connection task so the binding's handle can join it, and `abort` is the
/// hard stop the handle uses to drop connections that ignore `shutdown`.
pub(crate) async fn serve(
    listener: TcpListener,
    context: ConnectionContext,
    connections: ConnectionTasks,
) {
    loop {
        tokio::select! {
            biased;
            () = context.shutdown.cancelled() => break,
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else {
                    continue;
                };
                connections.register(
                    context
                        .tasks
                        .tracker()
                        .spawn(serve_connection(stream, context.clone())),
                );
            }
        }
    }
}

/// Serves one accepted connection, letting `shutdown` request a graceful close
/// of an in-flight or kept-alive connection and `abort` drop it outright.
async fn serve_connection(stream: TcpStream, context: ConnectionContext) {
    let ConnectionContext {
        exposures,
        tasks,
        default_timeout,
        limits,
        max_request_head_bytes,
        header_read_timeout,
        shutdown,
        abort,
    } = context;
    let service = service_fn(move |request: Request<Incoming>| {
        let exposures = Arc::clone(&exposures);
        let tasks = tasks.clone();
        async move {
            handle_request(
                request,
                Instant::now(),
                &exposures,
                &tasks,
                default_timeout,
                limits,
            )
            .await
            .map_err(|_| Abandoned)
        }
    });
    let mut builder = http1::Builder::new();
    builder
        .max_buf_size(max_request_head_bytes)
        .timer(TokioTimer::new())
        .header_read_timeout(header_read_timeout);
    let connection = builder.serve_connection(TokioIo::new(stream), service);
    tokio::pin!(connection);
    tokio::select! {
        biased;
        () = abort.cancelled() => {}
        () = shutdown.cancelled() => {
            connection.as_mut().graceful_shutdown();
            tokio::select! {
                biased;
                () = abort.cancelled() => {}
                _ = connection.as_mut() => {}
            }
        }
        _ = connection.as_mut() => {}
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::net::SocketAddr;
    use std::pin::Pin;

    use boxology_contract::{
        BoxId, CallContext, CapabilityDescriptor, CapabilityId, CapabilityName, CapabilityShape,
        ContractDescriptor, ContractRevision, ContractType, Detail, ErasedCallError, ErasedTarget,
        ExposureLevel, Idempotency, ImplementationDescriptor, SlotValue, TypeDescriptor,
    };
    use boxology_runtime::test_support::StubTransport;
    use boxology_runtime::{Composition, CompositionBuilder, TransportTaskTracker};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;
    use tokio::task::JoinHandle;

    use super::*;
    use crate::encoder::WireCallError;

    /// A hand-written provider target that greets its decoded string input.
    struct Greeter;

    impl ErasedTarget for Greeter {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            _context: CallContext,
            input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            Box::pin(async move {
                let name = String::decode(&input)
                    .map_err(|_| ErasedCallError::ContractViolation(Detail::new("input_decode")))?;
                format!("Hello, {name}!")
                    .encode()
                    .map_err(|_| ErasedCallError::InvalidResponse(Detail::new("output_encode")))
            })
        }
    }

    #[derive(Clone)]
    struct DisconnectObserver {
        entered: Arc<AtomicUsize>,
        observed: Arc<AtomicUsize>,
        entry_signal: Arc<Mutex<Option<oneshot::Sender<()>>>>,
        cancellation_signal: Arc<Mutex<Option<oneshot::Sender<()>>>>,
        release: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
    }

    impl DisconnectObserver {
        fn new() -> (
            Self,
            oneshot::Receiver<()>,
            oneshot::Receiver<()>,
            oneshot::Sender<()>,
        ) {
            let (entry_signal, entry) = oneshot::channel();
            let (cancellation_signal, cancellation) = oneshot::channel();
            let (release, release_gate) = oneshot::channel();
            (
                Self {
                    entered: Arc::new(AtomicUsize::new(0)),
                    observed: Arc::new(AtomicUsize::new(0)),
                    entry_signal: Arc::new(Mutex::new(Some(entry_signal))),
                    cancellation_signal: Arc::new(Mutex::new(Some(cancellation_signal))),
                    release: Arc::new(Mutex::new(Some(release_gate))),
                },
                entry,
                cancellation,
                release,
            )
        }
    }

    impl ErasedTarget for DisconnectObserver {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            context: CallContext,
            _input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            self.entered.fetch_add(1, Ordering::SeqCst);
            self.entry_signal
                .lock()
                .unwrap()
                .take()
                .expect("handler entered more than once")
                .send(())
                .expect("entry receiver dropped");
            let observed = self.observed.clone();
            let cancellation_signal = self
                .cancellation_signal
                .lock()
                .unwrap()
                .take()
                .expect("cancellation observed more than once");
            let release = self.release.lock().unwrap().take().unwrap();
            Box::pin(async move {
                context.cancellation().cancelled().await;
                observed.fetch_add(1, Ordering::SeqCst);
                cancellation_signal
                    .send(())
                    .expect("cancellation receiver dropped");
                release.await.expect("handler release gate dropped");
                "late"
                    .to_owned()
                    .encode()
                    .map_err(|_| ErasedCallError::InvalidResponse(Detail::new("late_encode")))
            })
        }
    }

    fn greet_capability() -> CapabilityId {
        CapabilityId::new(
            BoxId::new("hello").unwrap(),
            CapabilityName::new("greet").unwrap(),
        )
    }

    fn hello_implementation() -> ImplementationDescriptor {
        let greet = CapabilityDescriptor::new(
            greet_capability(),
            TypeDescriptor::string(),
            TypeDescriptor::string(),
            TypeDescriptor::enumeration([]).unwrap(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        );
        let contract = Box::leak(Box::new(
            ContractDescriptor::new(
                BoxId::new("hello").unwrap(),
                [greet],
                ContractRevision::new("r1").unwrap(),
            )
            .unwrap(),
        ));
        ImplementationDescriptor::new(contract, []).unwrap()
    }

    /// Composes and starts the greeter, returning the live composition (which
    /// keeps the runtime handle alive) and a cloned exposure slice.
    fn expose_hello() -> (Composition, Arc<[TransportExposure]>) {
        let stub = Arc::new(StubTransport::new());
        let mut builder = CompositionBuilder::new();
        builder.add_box(hello_implementation(), |_imports| Greeter);
        builder.expose(
            BoxId::new("hello").unwrap(),
            greet_capability(),
            stub.clone(),
            ExposureLevel::External,
        );
        let composition = builder.start().unwrap();
        let exposures: Arc<[TransportExposure]> = stub
            .runtime()
            .unwrap()
            .exposures()
            .iter()
            .cloned()
            .collect();
        (composition, exposures)
    }

    fn hand_written_hello_builder(binding: &Arc<HttpServerBinding>) -> CompositionBuilder {
        let mut builder = CompositionBuilder::new();
        builder.add_box(hello_implementation(), |_imports| Greeter);
        builder.expose(
            BoxId::new("hello").unwrap(),
            greet_capability(),
            binding.clone(),
            ExposureLevel::External,
        );
        builder
    }

    fn serve_hand_written_hello(binding: &Arc<HttpServerBinding>) -> Composition {
        hand_written_hello_builder(binding).start().unwrap()
    }

    async fn start_serving(
        exposures: Arc<[TransportExposure]>,
    ) -> (SocketAddr, CancellationToken, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let handle = tokio::spawn(serve(
            listener,
            ConnectionContext {
                exposures,
                tasks: DispatchTasks::new(TransportTaskTracker::new()),
                default_timeout: Duration::from_secs(5),
                limits: SyntaxLimits(64 * 1024, DEFAULT_DEPTH_LIMIT),
                max_request_head_bytes: DEFAULT_MAX_REQUEST_HEAD_BYTES,
                header_read_timeout: DEFAULT_HEADER_READ_TIMEOUT,
                shutdown: shutdown.clone(),
                abort: CancellationToken::new(),
            },
            ConnectionTasks::default(),
        ));
        (address, shutdown, handle)
    }

    /// Sends one closing request and returns the parsed status code and body.
    async fn round_trip(address: SocketAddr, path: &str, body: &[u8]) -> (u16, Vec<u8>) {
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\n\
             Connection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        let mut request = head.into_bytes();
        request.extend_from_slice(body);
        let raw = raw_request(address, &request).await;
        split_response(&raw)
    }

    async fn raw_request(address: SocketAddr, request: &[u8]) -> Vec<u8> {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(request).await.unwrap();
        let mut raw = Vec::new();
        if let Err(error) = stream.read_to_end(&mut raw).await {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        }
        raw
    }

    fn split_response(raw: &[u8]) -> (u16, Vec<u8>) {
        let (status, _headers, body) = split_response_parts(raw);
        (status, body.to_vec())
    }

    fn split_response_parts(raw: &[u8]) -> (u16, &[u8], &[u8]) {
        let line_end = raw
            .windows(2)
            .position(|window| window == b"\r\n")
            .expect("response is missing a status line");
        let status = std::str::from_utf8(&raw[..line_end])
            .expect("status line is not UTF-8")
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("status line is missing a numeric code");
        let boundary = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("response is missing a header/body boundary");
        (status, &raw[line_end + 2..boundary], &raw[boundary + 4..])
    }

    fn has_header(headers: &[u8], name: &[u8]) -> bool {
        headers.split(|byte| *byte == b'\n').any(|line| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            line.get(..name.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
                && line.get(name.len()) == Some(&b':')
        })
    }

    fn padded_request_head(length: usize) -> Vec<u8> {
        let prefix = b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\n\
Content-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\
X-Padding: ";
        let suffix = b"\r\n\r\n";
        assert!(length >= prefix.len() + suffix.len());
        let mut head = Vec::with_capacity(length);
        head.extend_from_slice(prefix);
        head.extend(std::iter::repeat_n(
            b'x',
            length - prefix.len() - suffix.len(),
        ));
        head.extend_from_slice(suffix);
        assert_eq!(head.len(), length);
        head
    }

    #[test]
    fn server_head_limits_have_explicit_defaults_and_hyper_floor() {
        let address = "127.0.0.1:0".parse().unwrap();
        let defaults = HttpServerConfig::new(address);
        assert_eq!(defaults.max_request_head_bytes, 16 * 1024);
        assert_eq!(defaults.header_read_timeout, Duration::from_secs(30));
        assert_eq!(
            HttpServerConfig::new(address)
                .with_max_request_head_bytes(1)
                .max_request_head_bytes,
            8192
        );
    }

    #[tokio::test]
    async fn serves_exposures_over_a_real_socket() {
        let (composition, exposures) = expose_hello();
        let (address, shutdown, server) = start_serving(exposures).await;

        let (status, body) = round_trip(address, "/rpc/hello/greet", br#""Ada""#).await;
        assert_eq!(status, 200);
        assert_eq!(
            body.as_slice(),
            br#"{"result":{"value":"Hello, Ada!"}}"#.as_slice()
        );

        shutdown.cancel();
        server.await.unwrap();
        drop(composition);
    }

    async fn converge_tracker_len(tracker: &TransportTaskTracker, expected: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if tracker.len() == expected {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("tracker count did not converge");
        assert_eq!(tracker.len(), expected);
    }

    #[tokio::test]
    async fn peer_full_close_cancels_dispatch_and_keeps_it_composition_owned() {
        let (observer, entered, cancellation, release) = DisconnectObserver::new();
        let stub = Arc::new(StubTransport::new());
        let binding = Arc::new(HttpServerBinding::new(
            HttpServerConfig::new("127.0.0.1:0".parse().unwrap())
                .with_default_timeout(Duration::from_secs(60)),
        ));
        let capability = greet_capability();
        let mut builder = CompositionBuilder::new();
        let target = observer.clone();
        builder.add_box(hello_implementation(), move |_imports| target);
        builder.expose(
            BoxId::new("hello").unwrap(),
            capability.clone(),
            stub.clone(),
            ExposureLevel::External,
        );
        builder.expose(
            BoxId::new("hello").unwrap(),
            capability,
            binding.clone(),
            ExposureLevel::External,
        );
        let composition = builder.start().unwrap();
        let tracker = stub.runtime().unwrap().tracker().clone();
        assert_eq!(tracker.len(), 1, "only the HTTP accept task is live");

        let mut stream = TcpStream::connect(binding.local_addr().unwrap())
            .await
            .unwrap();
        stream
            .write_all(
                b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\n\
                  Content-Type: application/json\r\nContent-Length: 5\r\n\r\n\"Ada\"",
            )
            .await
            .unwrap();
        stream.flush().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), entered)
            .await
            .expect("handler did not enter")
            .expect("handler entry signal dropped");
        assert_eq!(
            tracker.len(),
            3,
            "accept, connection, and dispatch are live"
        );
        assert_eq!(observer.entered.load(Ordering::SeqCst), 1);
        assert_eq!(observer.observed.load(Ordering::SeqCst), 0);

        drop(stream);
        tokio::time::timeout(Duration::from_secs(5), cancellation)
            .await
            .expect("full-close cancellation was not observed")
            .expect("cancellation signal dropped");
        converge_tracker_len(&tracker, 2).await;
        assert_eq!(observer.entered.load(Ordering::SeqCst), 1);
        assert_eq!(observer.observed.load(Ordering::SeqCst), 1);

        release.send(()).expect("handler release receiver dropped");
        converge_tracker_len(&tracker, 1).await;
        assert_eq!(observer.entered.load(Ordering::SeqCst), 1);
        assert_eq!(observer.observed.load(Ordering::SeqCst), 1);

        tokio::time::timeout(
            Duration::from_secs(1),
            composition.shutdown(Duration::from_millis(100)),
        )
        .await
        .expect("composition shutdown exceeded one second")
        .expect("composition shutdown failed");
    }

    #[tokio::test]
    async fn unknown_route_and_malformed_body_are_canonical_on_the_wire() {
        let (composition, exposures) = expose_hello();
        let (address, shutdown, server) = start_serving(exposures).await;

        let unknown = WireCallError::UnknownCapability.encode();
        let (status, body) = round_trip(address, "/rpc/hello/unknown", br#""Ada""#).await;
        assert_eq!(status, 404);
        assert_eq!(status, unknown.status());
        assert_eq!(body.as_slice(), unknown.body());

        let framing = raw_request(
            address,
            b"POST /rpc/hello/greet\r\nHost: boxology\r\nConnection: close\r\n\r\n",
        )
        .await;
        let (status, headers, body) = split_response_parts(&framing);
        assert_eq!(status, 400);
        assert!(body.is_empty());
        assert!(!has_header(headers, b"content-type"));

        let invalid = WireCallError::InvalidRequest.encode();
        let (status, body) = round_trip(address, "/rpc/hello/greet", b"{").await;
        assert_eq!(status, 400);
        assert_eq!(status, invalid.status());
        assert_eq!(body.as_slice(), invalid.body());

        shutdown.cancel();
        server.await.unwrap();
        drop(composition);
    }

    #[tokio::test]
    async fn default_request_head_cap_accepts_16384_and_rejects_16385_bare() {
        let binding = Arc::new(HttpServerBinding::new(HttpServerConfig::new(
            "127.0.0.1:0".parse().unwrap(),
        )));
        let composition = serve_hand_written_hello(&binding);
        let address = binding.local_addr().unwrap();

        let mut exact = padded_request_head(16 * 1024);
        exact.extend_from_slice(br#""Ada""#);
        let response = raw_request(address, &exact).await;
        let (status, _headers, body) = split_response_parts(&response);
        assert_eq!(status, 200);
        assert_eq!(body, br#"{"result":{"value":"Hello, Ada!"}}"#);

        let mut over = padded_request_head(16 * 1024 + 1);
        over.extend_from_slice(br#""Ada""#);
        let response = raw_request(address, &over).await;
        let (status, headers, body) = split_response_parts(&response);
        assert_eq!(status, 431);
        assert!(body.is_empty());
        assert!(!has_header(headers, b"content-type"));

        composition.shutdown(Duration::from_secs(1)).await.unwrap();
    }

    #[tokio::test]
    async fn configured_one_byte_request_head_cap_uses_hyper_floor() {
        let config =
            HttpServerConfig::new("127.0.0.1:0".parse().unwrap()).with_max_request_head_bytes(1);
        assert_eq!(config.max_request_head_bytes, 8192);
        let binding = Arc::new(HttpServerBinding::new(config));
        let composition = serve_hand_written_hello(&binding);
        let address = binding.local_addr().unwrap();

        let mut request = padded_request_head(8192);
        request.extend_from_slice(br#""Ada""#);
        let response = raw_request(address, &request).await;
        let (status, _headers, body) = split_response_parts(&response);
        assert_eq!(status, 200);
        assert_eq!(body, br#"{"result":{"value":"Hello, Ada!"}}"#);

        let mut over = padded_request_head(8193);
        over.extend_from_slice(br#""Ada""#);
        let response = raw_request(address, &over).await;
        let (status, headers, body) = split_response_parts(&response);
        assert_eq!(status, 431);
        assert!(body.is_empty());
        assert!(!has_header(headers, b"content-type"));

        composition.shutdown(Duration::from_secs(1)).await.unwrap();
    }

    #[tokio::test]
    async fn partial_request_head_closes_after_configured_timeout() {
        let configured_timeout = Duration::from_millis(300);
        let config = HttpServerConfig::new("127.0.0.1:0".parse().unwrap())
            .with_header_read_timeout(configured_timeout);
        let binding = Arc::new(HttpServerBinding::new(config));
        let composition = serve_hand_written_hello(&binding);
        let address = binding.local_addr().unwrap();
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\n")
            .await
            .unwrap();

        let mut byte = [0_u8; 1];
        assert!(
            tokio::time::timeout(Duration::from_millis(225), stream.read(&mut byte))
                .await
                .is_err(),
            "partial head closed before the configured timeout"
        );
        let mut rest = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut rest))
            .await
            .expect("header-read timeout did not close the connection")
            .unwrap();
        assert_eq!(read, 0);
        assert!(rest.is_empty());

        composition.shutdown(Duration::from_secs(1)).await.unwrap();
    }
    #[test]
    fn conform_and_prepare_reject_unroutable_exposures() {
        let binding = HttpServerBinding::new(HttpServerConfig::new("127.0.0.1:0".parse().unwrap()));
        assert!(binding.local_addr().is_none());
        let descriptor = hello_implementation()
            .contract()
            .capabilities()
            .first()
            .unwrap();
        for level in [ExposureLevel::Internal, ExposureLevel::External] {
            assert!(binding.conform(descriptor, level).is_ok());
        }
        let detail = binding
            .conform(descriptor, ExposureLevel::CodeOnly)
            .unwrap_err();
        assert_eq!(detail.code(), "http_code_only_exposure");

        assert!(binding.prepare(&[descriptor]).is_ok());
        let detail = binding.prepare(&[descriptor, descriptor]).unwrap_err();
        assert_eq!(detail.code(), "http_duplicate_capability");
    }

    #[tokio::test]
    async fn shutdown_stops_intake_and_refuses_new_connections() {
        let binding = Arc::new(HttpServerBinding::new(HttpServerConfig::new(
            "127.0.0.1:0".parse().unwrap(),
        )));
        let composition = serve_hand_written_hello(&binding);
        let address = binding.local_addr().unwrap();
        let idle = TcpStream::connect(address).await.unwrap();

        let Err(errors) = hand_written_hello_builder(&binding).start() else {
            panic!("a started binding accepted a second startup")
        };
        assert_eq!(
            errors.to_string(),
            "transport start failed: http_server_already_started"
        );

        composition.shutdown(Duration::from_secs(1)).await.unwrap();
        drop(idle);
        assert!(TcpStream::connect(address).await.is_err());
    }

    /// Announces a body that never arrives, under a deadline far beyond any
    /// drain timeout: the connection parks inside request-body collection.
    async fn park_connection(address: SocketAddr) -> TcpStream {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\n\
                  Content-Type: application/json\r\nboxology-timeout-ms: 9999999999\r\n\
                  Content-Length: 20\r\n\r\n",
            )
            .await
            .unwrap();
        stream.flush().await.unwrap();
        stream
    }

    #[tokio::test]
    async fn handle_abort_aborts_and_joins_owned_connection() {
        let tracker = TransportTaskTracker::new();
        let tasks = DispatchTasks::new(tracker.clone());
        let connections = ConnectionTasks::default();
        let connection = tracker.spawn(std::future::pending::<()>());
        assert!(!connection.is_finished());
        connections.register(connection);
        let handle = HttpServerHandle {
            intake: CancellationToken::new(),
            abort: CancellationToken::new(),
            tasks,
            accept: Mutex::new(None),
            connections,
        };
        assert_eq!(tracker.len(), 1);

        handle.abort_tasks();
        tokio::time::timeout(Duration::from_secs(1), Box::new(handle).join_tasks())
            .await
            .expect("handle must abort and join its retained connection")
            .unwrap();
        assert_eq!(tracker.len(), 0);
    }

    #[tokio::test]
    async fn drain_timeout_aborts_and_joins_a_parked_connection() {
        let binding = Arc::new(HttpServerBinding::new(HttpServerConfig::new(
            "127.0.0.1:0".parse().unwrap(),
        )));
        let composition = serve_hand_written_hello(&binding);
        let address = binding.local_addr().unwrap();
        let parked = park_connection(address).await;
        // Let the server read the head and park on the missing body.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // A parked connection outlives drain and cancellation, so shutdown must
        // reach the abort rung of the ladder before it reports success.
        assert!(
            composition
                .shutdown(Duration::from_millis(100))
                .await
                .is_ok()
        );

        // Shutdown joins every transport-owned task, so the connection is
        // already closed at this point. The check must therefore not await:
        // awaiting would hand this single-threaded runtime the chance to poll a
        // connection task that shutdown left running, which is exactly the
        // regression under test. Blocking the runtime thread instead lets only
        // the kernel deliver the peer's FIN, so a loaded machine cannot make an
        // unjoined connection look joined.
        let parked = parked.into_std().expect("parked connection is registered");
        std::thread::sleep(Duration::from_millis(20));
        let mut discard = [0_u8; 64];
        let read = std::io::Read::read(&mut &parked, &mut discard);
        let closed = match &read {
            Ok(read) => *read == 0,
            Err(error) => error.kind() != std::io::ErrorKind::WouldBlock,
        };
        assert!(
            closed,
            "shutdown returned before the parked connection was closed: {read:?}"
        );
    }
}
