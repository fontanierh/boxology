//! HTTP/1 serve-over-socket core that frames real connections onto the tested
//! request codec in [`crate::server`]. This is the framing half of the
//! HTTP-serving axis; the public binding, configuration, and handle arrive with
//! the next slice and reuse this core unchanged.

use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant},
};

use boxology_runtime::{TransportExposure, TransportTaskTracker};
use http::Request;
use hyper::{body::Incoming, server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::server::{DispatchTasks, handle_request};
use crate::syntax::SyntaxLimits;

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
/// through the shared codec. `tracker` owns both the accepted connection tasks
/// and the per-request dispatch tasks, so a later public binding can pass its
/// runtime's tracker and exposures straight through.
pub(crate) async fn serve(
    listener: TcpListener,
    exposures: Arc<[TransportExposure]>,
    tracker: TransportTaskTracker,
    default_timeout: Duration,
    limits: SyntaxLimits,
    shutdown: CancellationToken,
) {
    let tasks = DispatchTasks::new(tracker.clone());
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else {
                    continue;
                };
                tracker.spawn(serve_connection(
                    stream,
                    exposures.clone(),
                    tasks.clone(),
                    default_timeout,
                    limits,
                    shutdown.clone(),
                ));
            }
            () = shutdown.cancelled() => break,
        }
    }
}

/// Serves one accepted connection, letting `shutdown` request a graceful close
/// of an in-flight or kept-alive connection.
async fn serve_connection(
    stream: TcpStream,
    exposures: Arc<[TransportExposure]>,
    tasks: DispatchTasks,
    default_timeout: Duration,
    limits: SyntaxLimits,
    shutdown: CancellationToken,
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
        () = shutdown.cancelled() => {
            connection.as_mut().graceful_shutdown();
            let _ = connection.await;
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
    use boxology_runtime::{Composition, CompositionBuilder, test_support::StubTransport};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::task::JoinHandle;

    use super::*;
    use crate::encoder::WireCallError;
    use crate::syntax::DEFAULT_DEPTH_LIMIT;

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
            TransportTaskTracker::new(),
            Duration::from_secs(5),
            SyntaxLimits(64 * 1024, DEFAULT_DEPTH_LIMIT),
            shutdown.clone(),
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
}
