extern crate hello_contract as boxology_generated_contract;

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use boxology_contract::{
    BoxId, CallContext, CallError, Caller, CancelToken, CapabilityDescriptor, CapabilityId,
    CapabilityName, CapabilityShape, ContractDescriptor, ContractRevision, Deadline, Detail,
    ErasedCallError, ErasedCallTarget, ErasedTarget, ExposureLevel, Idempotency, IdempotencyKey,
    ImportDescriptor, SlotValue, TraceContext, TypeDescriptor, VariantDescriptor, VariantPayload,
};
use boxology_generated_contract::HelloHandle;
use boxology_http::{HttpClientConfig, HttpClientTarget, HttpServerBinding, HttpServerConfig};
use boxology_runtime::{AssemblyError, CompositionBuilder, ImportHandle, ImportTarget};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::timeout;

mod generated {
    include!("../../hello/generated/adapter/adapter.rs");
}

fn hello_greet() -> &'static CapabilityDescriptor {
    &boxology_generated_contract::contract_descriptor().capabilities()[0]
}

fn hello_builder(binding: &Arc<HttpServerBinding>) -> CompositionBuilder {
    let mut builder = CompositionBuilder::new();
    builder.add_box(generated::implementation_descriptor(), |imports| {
        generated::factory(hello_implementation::HelloService, imports)
    });
    builder.expose(
        BoxId::new("hello").unwrap(),
        hello_greet().id().clone(),
        binding.clone(),
        ExposureLevel::External,
    );
    builder
}

fn serve_generated_hello() -> (boxology_runtime::Composition, Arc<HttpServerBinding>) {
    let binding = Arc::new(HttpServerBinding::new(HttpServerConfig::new(
        "127.0.0.1:0".parse().unwrap(),
    )));
    let composition = hello_builder(&binding).start().unwrap();
    (composition, binding)
}

async fn round_trip(address: SocketAddr, target: &[u8], body: &[u8]) -> (u16, Vec<u8>) {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        let mut request = b"POST ".to_vec();
        request.extend_from_slice(target);
        request.extend_from_slice(
            format!(
                " HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\n\
                 Connection: close\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        );
        request.extend_from_slice(body);
        stream.write_all(&request).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        split_response(&raw)
    })
    .await
    .expect("HTTP round trip exceeded five-second timeout")
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

fn hello_descriptor() -> &'static CapabilityDescriptor {
    static HELLO: std::sync::LazyLock<CapabilityDescriptor> = std::sync::LazyLock::new(|| {
        CapabilityDescriptor::new(
            CapabilityId::new(
                BoxId::new("hello").unwrap(),
                CapabilityName::new("greet").unwrap(),
            ),
            TypeDescriptor::string(),
            TypeDescriptor::string(),
            TypeDescriptor::enumeration([VariantDescriptor::new(
                "EmptyName",
                VariantPayload::Unit,
                None,
            )])
            .unwrap(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        )
    });
    &HELLO
}

fn consumer_descriptor() -> boxology_contract::ImplementationDescriptor {
    let revision = ContractRevision::new("r1").unwrap();
    let contract = Box::leak(Box::new(
        ContractDescriptor::new(BoxId::new("consumer").unwrap(), [], revision.clone()).unwrap(),
    ));
    boxology_contract::ImplementationDescriptor::new(
        contract,
        [ImportDescriptor::new(
            BoxId::new("hello").unwrap(),
            revision,
            [hello_descriptor().id().clone()],
        )
        .unwrap()],
    )
    .unwrap()
}

fn hello_context() -> CallContext {
    context(None, None, None, None)
}

fn context(
    deadline: Option<Deadline>,
    traceparent: Option<&str>,
    tracestate: Option<&str>,
    key: Option<&str>,
) -> CallContext {
    CallContext::new(
        Caller::Anonymous,
        deadline,
        CancelToken::new(),
        TraceContext::new(
            traceparent.map(str::to_owned),
            tracestate.map(str::to_owned),
        ),
        key.map(|value| IdempotencyKey::new(value).unwrap()),
    )
}

async fn raw_server(
    fragments: Vec<Vec<u8>>,
    watch_second: bool,
) -> (String, JoinHandle<(Vec<u8>, bool)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = timeout(Duration::from_secs(5), listener.accept())
            .await
            .expect("client did not connect")
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut block = [0; 1024];
            let read = timeout(Duration::from_secs(5), stream.read(&mut block))
                .await
                .expect("client request stalled")
                .unwrap();
            assert_ne!(read, 0, "client closed before completing request");
            request.extend_from_slice(&block[..read]);
            let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
                continue;
            };
            let head = std::str::from_utf8(&request[..end]).unwrap();
            let length = head.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            });
            if request.len() >= end + 4 + length.unwrap_or(0) {
                break;
            }
        }
        for fragment in fragments {
            stream.write_all(&fragment).await.unwrap();
            tokio::task::yield_now().await;
        }
        stream.shutdown().await.unwrap();
        let second = watch_second
            && timeout(Duration::from_millis(200), listener.accept())
                .await
                .is_ok();
        (request, second)
    });
    (format!("http://{address}"), task)
}

fn http_response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status}\r\n{headers}\r\n").into_bytes();
    response.extend_from_slice(body);
    response
}

#[tokio::test]
async fn raw_hello_request_gets_canonical_bytes() {
    let (composition, binding) = serve_generated_hello();
    let address = binding.local_addr().unwrap();
    let (status, body) = round_trip(address, b"/rpc/hello/greet", br#""Ada""#).await;
    assert_eq!(status, 200);
    assert_eq!(
        body.as_slice(),
        br#"{"result":{"value":"Hello, Ada!"}}"#.as_slice()
    );
    composition.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn s3_t3_literal_tcp_request_targets_are_not_normalized() {
    let (composition, binding) = serve_generated_hello();
    let address = binding.local_addr().unwrap();
    let cases: [(&[u8], u16, &[u8]); 4] = [
        (
            b"/rpc/hello/greet",
            200,
            br#"{"result":{"value":"Hello, Ada!"}}"#,
        ),
        (
            b"/rpc/h%65llo/greet",
            404,
            br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#,
        ),
        (
            b"/rpc/hello/gr%65et",
            404,
            br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#,
        ),
        (
            b"/rpc/hello/greet?probe=1",
            400,
            br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#,
        ),
    ];

    for (target, expected_status, expected_body) in cases {
        let (status, body) = round_trip(address, target, br#""Ada""#).await;
        assert_eq!(status, expected_status, "target: {target:?}");
        assert_eq!(body.as_slice(), expected_body, "target: {target:?}");
    }

    composition.shutdown(Duration::from_secs(1)).await.unwrap();
}

#[tokio::test]
async fn s3_t3_occupied_http_address_fails_composition_start() {
    let occupying = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = occupying.local_addr().unwrap();
    let binding = Arc::new(HttpServerBinding::new(HttpServerConfig::new(address)));
    let error = hello_builder(&binding)
        .start()
        .err()
        .expect("occupied HTTP address unexpectedly started");

    let [AssemblyError::TransportStartFailed { detail }] = error.errors() else {
        panic!("expected one transport start failure, got {error:?}");
    };
    assert_eq!(detail.code(), "http_bind");
    assert!(detail.message().is_some_and(|message| !message.is_empty()));
    assert_eq!(binding.local_addr(), None);
    assert!(TcpListener::bind(address).await.is_err());
}

#[tokio::test]
async fn s3_t3_http_conformance_rejects_streaming_and_top_level_field() {
    for (shape, input, expected_detail, expected_display) in [
        (
            CapabilityShape::ServerStreaming,
            TypeDescriptor::string(),
            Detail::new("http_non_unary").with_message("HTTP supports unary capabilities only"),
            "transport conformance failed for capability hello.greet: http_non_unary: HTTP supports unary capabilities only",
        ),
        (
            CapabilityShape::Unary,
            TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap(),
            Detail::new("http_top_level_field")
                .with_message("HTTP cannot represent top-level Field in input"),
            "transport conformance failed for capability hello.greet: http_top_level_field: HTTP cannot represent top-level Field in input",
        ),
    ] {
        let capability = CapabilityDescriptor::new(
            hello_greet().id().clone(),
            input,
            hello_greet().output().clone(),
            hello_greet().error().clone(),
            shape,
            hello_greet().max_exposure(),
            hello_greet().idempotency(),
            None,
        );
        let contract: &'static ContractDescriptor = Box::leak(Box::new(
            ContractDescriptor::new(
                BoxId::new("hello").unwrap(),
                [capability],
                ContractRevision::new("r1").unwrap(),
            )
            .unwrap(),
        ));
        let implementation =
            boxology_contract::ImplementationDescriptor::new(contract, []).unwrap();
        let binding = Arc::new(HttpServerBinding::new(HttpServerConfig::new(
            "127.0.0.1:0".parse().unwrap(),
        )));
        let mut builder = CompositionBuilder::new();
        builder.add_box(implementation, |imports| {
            generated::factory(hello_implementation::HelloService, imports)
        });
        builder.expose(
            BoxId::new("hello").unwrap(),
            contract.capabilities()[0].id().clone(),
            binding.clone(),
            ExposureLevel::External,
        );

        let error = builder
            .start()
            .err()
            .expect("invalid HTTP capability unexpectedly started");
        assert_eq!(
            error.errors(),
            &[AssemblyError::TransportConformanceFailed {
                capability: hello_greet().id().clone(),
                detail: expected_detail,
            }]
        );
        assert_eq!(error.to_string(), expected_display);
        assert_eq!(binding.local_addr(), None);
    }
}

#[tokio::test]
async fn public_target_drives_generated_handle_with_exact_contract() {
    let body = br#"{"result":{"value":"Hello, Ada!"}}"#;
    let (origin, server) = raw_server(
        vec![http_response(
            "200 OK",
            &format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ),
            body,
        )],
        false,
    )
    .await;
    let config = HttpClientConfig::new(&origin).unwrap();
    let target = HttpClientTarget::new(config, [hello_descriptor()]).unwrap();
    fn assert_public_bounds<T: Clone + Send + Sync + 'static>() {}
    assert_public_bounds::<HttpClientConfig>();
    assert_public_bounds::<HttpClientTarget>();
    let erased: Arc<dyn ErasedCallTarget> = Arc::new(target.clone());
    let handle = HelloHandle::from_erased(erased);
    let future = handle.greet(hello_context(), "Ada".into());
    fn assert_send<T: Send>(_: &T) {}
    assert_send(&future);
    assert_eq!(future.await.unwrap(), "Hello, Ada!");

    let (request, _) = server.await.unwrap();
    let request = String::from_utf8(request).unwrap();
    let (head, payload) = request.split_once("\r\n\r\n").unwrap();
    assert_eq!(head.lines().next(), Some("POST /rpc/hello/greet HTTP/1.1"));
    assert!(
        head.to_ascii_lowercase()
            .contains("content-type: application/json")
    );
    assert!(head.to_ascii_lowercase().contains("content-length: 5"));
    assert!(head.to_ascii_lowercase().contains(&format!(
        "host: {}",
        origin.strip_prefix("http://").unwrap()
    )));
    assert_eq!(payload, "\"Ada\"");
}

#[tokio::test]
async fn composition_injects_public_target_into_generated_handle() {
    struct Consumer;
    impl ErasedTarget for Consumer {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            _context: CallContext,
            _input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            Box::pin(std::future::pending())
        }
    }

    let absent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unsupported = HttpClientTarget::new(
        HttpClientConfig::new(format!("http://{}", absent.local_addr().unwrap())).unwrap(),
        [],
    )
    .unwrap();
    let mut invalid = CompositionBuilder::new();
    invalid.add_box(consumer_descriptor(), |_| Consumer);
    invalid.resolve_import(
        BoxId::new("consumer").unwrap(),
        BoxId::new("hello").unwrap(),
        ImportTarget::remote(Arc::new(unsupported)),
    );
    for errors in [
        invalid.validate().unwrap_err(),
        invalid.start().err().unwrap(),
    ] {
        assert!(matches!(
            errors.errors(),
            [boxology_runtime::AssemblyError::MissingImportedCapability { capability, .. }]
                if capability == hello_descriptor().id()
        ));
    }
    assert!(
        timeout(Duration::from_millis(50), absent.accept())
            .await
            .is_err()
    );

    let body = br#"{"result":{"value":"Hello, Ada!"}}"#;
    let (origin, server) = raw_server(
        vec![http_response(
            "200 OK",
            &format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            ),
            body,
        )],
        true,
    )
    .await;
    let target = HttpClientTarget::new(
        HttpClientConfig::new(&origin).unwrap(),
        [hello_descriptor()],
    )
    .unwrap();
    let mut imported: Option<ImportHandle> = None;
    let mut builder = CompositionBuilder::new();
    builder.add_box(consumer_descriptor(), |imports| {
        imported = Some(
            imports
                .handle(&BoxId::new("hello").unwrap())
                .unwrap()
                .clone(),
        );
        Consumer
    });
    builder.resolve_import(
        BoxId::new("consumer").unwrap(),
        BoxId::new("hello").unwrap(),
        ImportTarget::remote(Arc::new(target)),
    );
    let imported = imported.unwrap();
    assert!(matches!(
        imported
            .call(hello_descriptor().id(), hello_context(), SlotValue::Null)
            .await,
        Err(ErasedCallError::Unavailable(ref detail)) if detail.code() == "unsealed_import"
    ));
    let _composition = builder.start().unwrap();
    let unknown = CapabilityId::new(
        BoxId::new("hello").unwrap(),
        CapabilityName::new("unknown").unwrap(),
    );
    assert!(matches!(
        imported.call(&unknown, hello_context(), SlotValue::Null).await,
        Err(ErasedCallError::ContractViolation(ref detail))
            if detail.code() == "undeclared_import_capability"
    ));
    let context = context(
        None,
        Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        Some("vendor=value"),
        Some("request-7"),
    );
    assert_eq!(
        HelloHandle::from_erased(Arc::new(imported))
            .greet(context, "Ada".into())
            .await
            .unwrap(),
        "Hello, Ada!"
    );

    let (request, second_connection) = server.await.unwrap();
    assert!(!second_connection);
    let request = String::from_utf8(request).unwrap();
    let (head, payload) = request.split_once("\r\n\r\n").unwrap();
    let lower = head.to_ascii_lowercase();
    assert_eq!(head.lines().next(), Some("POST /rpc/hello/greet HTTP/1.1"));
    assert!(lower.contains("traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"));
    assert!(lower.contains("tracestate: vendor=value"));
    assert!(lower.contains("idempotency-key: request-7"));
    assert_eq!(payload, "\"Ada\"");
}

#[tokio::test]
async fn public_target_rejects_hostile_responses_and_applies_custom_limits() {
    for (headers, body, bytes, depth, succeeds) in [
        (
            "Content-Type: text/plain",
            br#"SENTINEL-HOSTILE"#.as_slice(),
            1024,
            128,
            false,
        ),
        (
            "Content-Type: application/json",
            br#"{"SENTINEL-HOSTILE":"wrong-envelope"}"#.as_slice(),
            1024,
            128,
            false,
        ),
        (
            "Content-Type: application/json",
            br#"{"result":{"value":"Hello, Ada!"}}"#.as_slice(),
            33,
            128,
            false,
        ),
        (
            "Content-Type: application/json",
            br#"{"result":{"value":"Hello, Ada!"}}"#.as_slice(),
            1024,
            1,
            false,
        ),
        (
            "Content-Type: application/json",
            br#"{"result":{"value":"Hello, Ada!"}}"#.as_slice(),
            34,
            2,
            true,
        ),
    ] {
        let (origin, server) = raw_server(
            vec![http_response(
                "200 OK",
                &format!("{headers}\r\nContent-Length: {}\r\n", body.len()),
                body,
            )],
            false,
        )
        .await;
        let config = HttpClientConfig::new(origin)
            .unwrap()
            .with_response_limits(bytes, depth);
        let target = HttpClientTarget::new(config, [hello_descriptor()]).unwrap();
        let result = HelloHandle::from_erased(Arc::new(target))
            .greet(hello_context(), "Ada".into())
            .await;
        if succeeds {
            assert_eq!(result.unwrap(), "Hello, Ada!");
        } else {
            let error = result.unwrap_err();
            assert!(
                matches!(error, CallError::InvalidResponse(ref detail) if detail.code() == "http_response")
            );
            assert!(!format!("{error:?}").contains("SENTINEL"));
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn selection_is_safe_exact_and_network_free_on_rejection() {
    let config = HttpClientConfig::new("http://127.0.0.1:1").unwrap();
    let duplicate = HttpClientTarget::new(config.clone(), [hello_descriptor(), hello_descriptor()])
        .err()
        .unwrap();
    assert_eq!(
        duplicate,
        boxology_contract::Detail::new("http_duplicate_capability")
    );

    let rejected: &'static CapabilityDescriptor = Box::leak(Box::new(CapabilityDescriptor::new(
        hello_descriptor().id().clone(),
        TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap(),
        TypeDescriptor::string(),
        TypeDescriptor::enumeration([]).unwrap(),
        CapabilityShape::Unary,
        ExposureLevel::External,
        Idempotency::None,
        None,
    )));
    assert_eq!(
        HttpClientTarget::new(config, [rejected])
            .err()
            .unwrap()
            .code(),
        "http_top_level_field"
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = HttpClientTarget::new(
        HttpClientConfig::new(format!("http://{}", listener.local_addr().unwrap())).unwrap(),
        [],
    )
    .unwrap();
    let error = HelloHandle::from_erased(Arc::new(target))
        .greet(hello_context(), "Ada".into())
        .await
        .unwrap_err();
    assert!(
        matches!(error, CallError::ContractViolation(ref detail) if detail.code() == "http_client_capability")
    );
    assert!(
        timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}
