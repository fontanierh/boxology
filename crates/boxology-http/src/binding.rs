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
use hyper_util::rt::TokioIo;
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

/// Configuration for an HTTP server binding.
#[derive(Clone)]
pub struct HttpServerConfig {
    bind_addr: SocketAddr,
    default_timeout: Duration,
    limits: SyntaxLimits,
}

impl HttpServerConfig {
    /// Creates configuration accepting connections on `bind_addr`.
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self {
            bind_addr,
            default_timeout: DEFAULT_REQUEST_TIMEOUT,
            limits: SyntaxLimits(DEFAULT_MAX_BODY_BYTES, DEFAULT_DEPTH_LIMIT),
        }
    }

    /// Replaces the deadline given to requests that carry no timeout header.
    pub fn with_default_timeout(mut self, default_timeout: Duration) -> Self {
        self.default_timeout = default_timeout;
        self
    }

    /// Replaces the inclusive request byte and syntax-depth limits.
    pub fn with_request_limits(mut self, max_body_bytes: usize, max_decode_depth: usize) -> Self {
        self.limits = SyntaxLimits(max_body_bytes, max_decode_depth);
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
}

impl HttpServerHandle {
    /// Locks the accept-driver slot, tolerating a poisoned lock.
    fn accept_slot(&self) -> MutexGuard<'_, Option<JoinHandle<()>>> {
        self.accept.lock().unwrap_or_else(PoisonError::into_inner)
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
        let accept = tracker.spawn({
            let (intake, abort, tasks) = (intake.clone(), abort.clone(), tasks.clone());
            async move {
                tokio::select! {
                    () = runtime.wait_until_active() => {}
                    () = intake.cancelled() => return,
                }
                serve(
                    listener,
                    runtime.exposures().iter().cloned().collect(),
                    tasks,
                    config.default_timeout,
                    config.limits,
                    intake,
                    abort,
                )
                .await;
            }
        });
        Ok(HttpServerHandle {
            intake,
            abort,
            tasks,
            accept: Mutex::new(Some(accept)),
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
        if let Some(accept) = self.accept_slot().as_ref() {
            accept.abort();
        }
    }

    fn join_tasks(self: Box<Self>) -> TransportJoinFuture {
        let accept = self.accept_slot().take();
        Box::pin(async move {
            if let Some(accept) = accept
                && let Err(error) = accept.await
                && !error.is_cancelled()
            {
                return Err(Detail::new("http_accept_failed").with_message(error.to_string()));
            }
            self.tasks.wait_empty().await;
            Ok(())
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

/// Accepts connections until `shutdown` is triggered, dispatching each request
/// through the shared codec. The tracker behind `tasks` owns both the accepted
/// connection tasks and the per-request dispatch tasks, and `abort` is the hard
/// stop the binding's handle uses to drop connections that ignore `shutdown`.
pub(crate) async fn serve(
    listener: TcpListener,
    exposures: Arc<[TransportExposure]>,
    tasks: DispatchTasks,
    default_timeout: Duration,
    limits: SyntaxLimits,
    shutdown: CancellationToken,
    abort: CancellationToken,
) {
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else {
                    continue;
                };
                tasks.tracker().spawn(serve_connection(
                    stream,
                    exposures.clone(),
                    tasks.clone(),
                    default_timeout,
                    limits,
                    shutdown.clone(),
                    abort.clone(),
                ));
            }
            () = shutdown.cancelled() => break,
        }
    }
}

/// Serves one accepted connection, letting `shutdown` request a graceful close
/// of an in-flight or kept-alive connection and `abort` drop it outright.
async fn serve_connection(
    stream: TcpStream,
    exposures: Arc<[TransportExposure]>,
    tasks: DispatchTasks,
    default_timeout: Duration,
    limits: SyntaxLimits,
    shutdown: CancellationToken,
    abort: CancellationToken,
) {
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
    let connection = http1::Builder::new().serve_connection(TokioIo::new(stream), service);
    tokio::pin!(connection);
    tokio::select! {
        _ = connection.as_mut() => {}
        () = abort.cancelled() => {}
        () = shutdown.cancelled() => {
            connection.as_mut().graceful_shutdown();
            tokio::select! {
                _ = connection.as_mut() => {}
                () = abort.cancelled() => {}
            }
        }
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::task::JoinHandle;

    use super::*;
    use crate::encoder::WireCallError;

    /// The real generated Hello adapter, included read-only from the fixture.
    mod generated {
        include!("../../fixtures/hello/generated/adapter/adapter.rs");
    }

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

    async fn start_serving(
        exposures: Arc<[TransportExposure]>,
    ) -> (SocketAddr, CancellationToken, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let handle = tokio::spawn(serve(
            listener,
            exposures,
            DispatchTasks::new(TransportTaskTracker::new()),
            Duration::from_secs(5),
            SyntaxLimits(64 * 1024, DEFAULT_DEPTH_LIMIT),
            shutdown.clone(),
            CancellationToken::new(),
        ));
        (address, shutdown, handle)
    }

    /// Sends one closing request and returns the parsed status code and body.
    async fn round_trip(address: SocketAddr, path: &str, body: &[u8]) -> (u16, Vec<u8>) {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\n\
             Connection: close\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(body).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        split_response(&raw)
    }

    fn split_response(raw: &[u8]) -> (u16, Vec<u8>) {
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
        (status, raw[boundary + 4..].to_vec())
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

    #[tokio::test]
    async fn unknown_route_and_malformed_body_are_canonical_on_the_wire() {
        let (composition, exposures) = expose_hello();
        let (address, shutdown, server) = start_serving(exposures).await;

        let unknown = WireCallError::UnknownCapability.encode();
        let (status, body) = round_trip(address, "/rpc/hello/unknown", br#""Ada""#).await;
        assert_eq!(status, 404);
        assert_eq!(status, unknown.status());
        assert_eq!(body.as_slice(), unknown.body());

        let invalid = WireCallError::InvalidRequest.encode();
        let (status, body) = round_trip(address, "/rpc/hello/greet", b"{").await;
        assert_eq!(status, 400);
        assert_eq!(status, invalid.status());
        assert_eq!(body.as_slice(), invalid.body());

        shutdown.cancel();
        server.await.unwrap();
        drop(composition);
    }

    /// Returns the one capability of the real generated Hello contract.
    fn hello_greet() -> &'static CapabilityDescriptor {
        &boxology_generated_contract::contract_descriptor().capabilities()[0]
    }

    /// Composes the real generated Hello box behind `binding`.
    fn hello_builder(binding: &Arc<HttpServerBinding>) -> CompositionBuilder {
        let mut builder = CompositionBuilder::new();
        builder.add_box(generated::implementation_descriptor(), |imports| {
            generated::factory(::hello_implementation::HelloService, imports)
        });
        builder.expose(
            BoxId::new("hello").unwrap(),
            hello_greet().id().clone(),
            binding.clone(),
            ExposureLevel::External,
        );
        builder
    }

    /// Starts the composed Hello box on an ephemeral loopback port.
    fn serve_generated_hello() -> (Composition, Arc<HttpServerBinding>) {
        let binding = Arc::new(HttpServerBinding::new(HttpServerConfig::new(
            "127.0.0.1:0".parse().unwrap(),
        )));
        let composition = hello_builder(&binding).start().unwrap();
        (composition, binding)
    }

    #[cfg(feature = "client")]
    #[tokio::test]
    async fn composed_hello_box_answers_typed_client_over_real_http() {
        use boxology_contract::{Caller, CancelToken, TraceContext};
        use boxology_generated_contract::HelloHandle;

        let (composition, binding) = serve_generated_hello();
        let address = binding.local_addr().unwrap();
        let target = crate::HttpClientTarget::new(
            crate::HttpClientConfig::new(format!("http://{address}")).unwrap(),
            [hello_greet()],
        )
        .unwrap();
        let context = CallContext::new(
            Caller::Anonymous,
            None,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        );

        let greeting = HelloHandle::from_erased(Arc::new(target))
            .greet(context, "Ada".into())
            .await;
        assert_eq!(greeting.unwrap(), "Hello, Ada!");

        composition.shutdown(Duration::from_secs(1)).await.unwrap();
        assert!(TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn raw_hello_request_gets_canonical_bytes() {
        let (composition, binding) = serve_generated_hello();
        let address = binding.local_addr().unwrap();
        let (status, body) = round_trip(address, "/rpc/hello/greet", br#""Ada""#).await;
        assert_eq!(status, 200);
        assert_eq!(
            body.as_slice(),
            br#"{"result":{"value":"Hello, Ada!"}}"#.as_slice()
        );
        composition.shutdown(Duration::from_secs(1)).await.unwrap();
    }

    #[test]
    fn conform_and_prepare_reject_unroutable_exposures() {
        let binding = HttpServerBinding::new(HttpServerConfig::new("127.0.0.1:0".parse().unwrap()));
        assert!(binding.local_addr().is_none());
        for level in [ExposureLevel::Internal, ExposureLevel::External] {
            assert!(binding.conform(hello_greet(), level).is_ok());
        }
        let detail = binding
            .conform(hello_greet(), ExposureLevel::CodeOnly)
            .unwrap_err();
        assert_eq!(detail.code(), "http_code_only_exposure");

        assert!(binding.prepare(&[hello_greet()]).is_ok());
        let detail = binding
            .prepare(&[hello_greet(), hello_greet()])
            .unwrap_err();
        assert_eq!(detail.code(), "http_duplicate_capability");
    }

    #[tokio::test]
    async fn shutdown_stops_intake_and_refuses_new_connections() {
        let (composition, binding) = serve_generated_hello();
        let address = binding.local_addr().unwrap();
        let idle = TcpStream::connect(address).await.unwrap();

        let Err(errors) = hello_builder(&binding).start() else {
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
    async fn drain_timeout_aborts_a_parked_connection() {
        let (composition, binding) = serve_generated_hello();
        let address = binding.local_addr().unwrap();
        let mut parked = park_connection(address).await;
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

        let mut discard = [0_u8; 64];
        let read = tokio::time::timeout(Duration::from_secs(5), parked.read(&mut discard))
            .await
            .expect("shutdown returned while the parked connection was still open");
        assert!(
            matches!(read, Ok(0) | Err(_)),
            "parked connection was not dropped: {read:?}"
        );
    }
}
