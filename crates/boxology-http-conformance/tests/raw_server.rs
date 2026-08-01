#[allow(dead_code)]
mod support;
use boxology_contract::{
    CallContext, CallError, Caller, CancelToken, Deadline, Detail, TraceContext,
};
use boxology_http::{HttpClientConfig, HttpClientTarget};
use hello_contract::{GreetError, HelloHandle};
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};
use support::hello_greet;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
const ROW_COUNT: usize = 35;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const SENTINEL: &str = "SENTINEL";
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SpecParagraph {
    S3D2Codec,
    S3D3CanonicalResponseEncoding,
    S3D5StableWireErrorCodes,
    S3D6HeaderGrammars,
    S3D7RequestLifecycle,
    S3D8ClientBinding,
    RuntimeInvocationStatusTable,
    RuntimeStableWireCodes,
}

use SpecParagraph::{
    RuntimeInvocationStatusTable as STATUS, RuntimeStableWireCodes as CODES, S3D2Codec as D2,
    S3D3CanonicalResponseEncoding as D3, S3D5StableWireErrorCodes as D5, S3D6HeaderGrammars as D6,
    S3D7RequestLifecycle as D7, S3D8ClientBinding as D8,
};
const A_D3_D8: &[SpecParagraph] = &[D3, D8];
const A_D2_D8: &[SpecParagraph] = &[D2, D8];
const A_CODES: &[SpecParagraph] = &[D5, D8, STATUS, CODES];
const A_D6_D8: &[SpecParagraph] = &[D6, D8];
const A_D7_D8: &[SpecParagraph] = &[D7, D8];
const A_D8: &[SpecParagraph] = &[D8];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Category {
    Unavailable,
    ContractViolation,
    InvalidResponse,
    Internal,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Expected {
    Value(&'static str),
    DomainEmptyName,
    DomainUnknownTag(&'static str),
    Failure(Category, &'static str),
    Deadline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResponseScript {
    Send {
        head: &'static [u8],
        body: &'static [u8],
        watch_second: bool,
    },
    Hold,
}

impl ResponseScript {
    const fn send(head: &'static [u8], body: &'static [u8]) -> Self {
        Self::Send {
            head,
            body,
            watch_second: false,
        }
    }

    const fn redirect(head: &'static [u8], body: &'static [u8]) -> Self {
        Self::Send {
            head,
            body,
            watch_second: true,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawServerRow {
    id: &'static str,
    authority: &'static [SpecParagraph],
    response: ResponseScript,
    tuning: Option<(usize, usize)>,
    deadline: Option<Duration>,
    expected: Expected,
}

const fn row(
    id: &'static str,
    authority: &'static [SpecParagraph],
    response: ResponseScript,
    tuning: Option<(usize, usize)>,
    deadline: Option<Duration>,
    expected: Expected,
) -> RawServerRow {
    RawServerRow {
        id,
        authority,
        response,
        tuning,
        deadline,
        expected,
    }
}
const HEAD_200: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_204: &[u8] =
    b"HTTP/1.1 204 No Content\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_302: &[u8] = b"HTTP/1.1 302 Found\r\nLocation: /rpc/hello/greet\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_400: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_404: &[u8] =
    b"HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_405: &[u8] =
    b"HTTP/1.1 405 Method Not Allowed\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_413: &[u8] =
    b"HTTP/1.1 413 Payload Too Large\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_415: &[u8] =
    b"HTTP/1.1 415 Unsupported Media Type\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_422: &[u8] =
    b"HTTP/1.1 422 Unprocessable Content\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_500: &[u8] =
    b"HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_501: &[u8] =
    b"HTTP/1.1 501 Not Implemented\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_502: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_503: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_504: &[u8] =
    b"HTTP/1.1 504 Gateway Timeout\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";
const HEAD_MISSING: &[u8] = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";
const HEAD_TEXT_JSON: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/json\r\nConnection: close\r\n\r\n";
const HEAD_CHARSET: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nConnection: close\r\n\r\n";
const HEAD_TRUNCATED: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 34\r\nConnection: close\r\n\r\n";
const HEAD_CHUNKED: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

const OK_BODY: &[u8] = br#"{"result":{"value":"Hello, Ada!"}}"#;
const EMPTY_DOMAIN_BODY: &[u8] =
    br#"{"error":{"kind":"domain","value":{"tag":"EmptyName","payload":null}}}"#;
const UNKNOWN_DOMAIN_BODY: &[u8] =
    br#"{"error":{"kind":"domain","value":{"tag":"Future","payload":{"x":1}}}}"#;
const UNKNOWN_BOX_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#;
const UNKNOWN_CAPABILITY_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#;
const INVALID_REQUEST_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#;
const METHOD_NOT_ALLOWED_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#;
const PAYLOAD_TOO_LARGE_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"payload_too_large","message":"payload too large"}}"#;
const UNSUPPORTED_MEDIA_TYPE_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#;
const DEADLINE_EXCEEDED_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"deadline_exceeded","message":"deadline exceeded"}}"#;
const UNAVAILABLE_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"unavailable","message":"service unavailable"}}"#;
const INVALID_UPSTREAM_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"invalid_upstream_response","message":"invalid upstream response"}}"#;
const INTERNAL_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"internal","message":"internal error"}}"#;
const WRONG_MESSAGE_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"internal","message":"wrong"}}"#;
const UNKNOWN_CODE_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"mystery","message":"SENTINEL-CALL"}}"#;
const WRONG_KIND_BODY: &[u8] =
    br#"{"error":{"kind":"domain","code":"internal","message":"internal error"}}"#;
const EXTRA_CALL_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"internal","message":"internal error","extra":0}}"#;
const EXTRA_RESULT_BODY: &[u8] = br#"{"result":{"value":"Hello, Ada!"},"extra":0}"#;
const EXTRA_INNER_BODY: &[u8] = br#"{"result":{"value":"Hello, Ada!","extra":0}}"#;
const WRONG_DOMAIN_KIND_BODY: &[u8] =
    br#"{"error":{"kind":"call","value":{"tag":"EmptyName","payload":null}}}"#;
const EXTRA_DOMAIN_BODY: &[u8] =
    br#"{"error":{"kind":"domain","value":{"tag":"EmptyName","payload":null},"extra":0}}"#;
const BOM_BODY: &[u8] = b"\xef\xbb\xbf{\"result\":{\"value\":\"Hello, Ada!\"}}";
const MALFORMED_BODY: &[u8] = b"{";
const TRUNCATED_BODY: &[u8] = b"SENTINEL-TRUNCATED!!";
const OVERSIZED_CHUNKED_BODY: &[u8] =
    b"64\r\n{\"result\":{\"value\":\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"}}\r\n0\r\n\r\n";

#[rustfmt::skip]
const ROWS: [RawServerRow; ROW_COUNT] = [
    // `decode_result` is the only rejecting-free path here; the driver also pins the request.
    row("ok-result", A_D3_D8, ResponseScript::send(HEAD_200, OK_BODY), None, None, Expected::Value("Hello, Ada!")),
    // ConsumerOutput is deliberately used for both known and unknown domain enum tags.
    row("domain-empty-name", A_D8, ResponseScript::send(HEAD_422, EMPTY_DOMAIN_BODY), None, None, Expected::DomainEmptyName),
    row("domain-unknown-variant-tolerant", A_D2_D8, ResponseScript::send(HEAD_422, UNKNOWN_DOMAIN_BODY), None, None, Expected::DomainUnknownTag("Future")),
    // Each code row reaches its own `decode_call` match arm with pinned status and message.
    row("unknown-box", A_CODES, ResponseScript::send(HEAD_404, UNKNOWN_BOX_BODY), None, None, Expected::Failure(Category::Unavailable, "unknown_box")),
    row("unknown-capability", A_CODES, ResponseScript::send(HEAD_404, UNKNOWN_CAPABILITY_BODY), None, None, Expected::Failure(Category::Unavailable, "unknown_capability")),
    row("invalid-request", A_CODES, ResponseScript::send(HEAD_400, INVALID_REQUEST_BODY), None, None, Expected::Failure(Category::ContractViolation, "invalid_request")),
    row("method-not-allowed", A_CODES, ResponseScript::send(HEAD_405, METHOD_NOT_ALLOWED_BODY), None, None, Expected::Failure(Category::InvalidResponse, "method_not_allowed")),
    row("payload-too-large", A_CODES, ResponseScript::send(HEAD_413, PAYLOAD_TOO_LARGE_BODY), None, None, Expected::Failure(Category::ContractViolation, "payload_too_large")),
    row("unsupported-media-type", A_CODES, ResponseScript::send(HEAD_415, UNSUPPORTED_MEDIA_TYPE_BODY), None, None, Expected::Failure(Category::InvalidResponse, "unsupported_media_type")),
    row("deadline-exceeded", A_CODES, ResponseScript::send(HEAD_504, DEADLINE_EXCEEDED_BODY), None, None, Expected::Deadline),
    row("unavailable", A_CODES, ResponseScript::send(HEAD_503, UNAVAILABLE_BODY), None, None, Expected::Failure(Category::Unavailable, "unavailable")),
    row("invalid-upstream-response", A_CODES, ResponseScript::send(HEAD_502, INVALID_UPSTREAM_BODY), None, None, Expected::Failure(Category::InvalidResponse, "invalid_upstream_response")),
    row("internal", A_CODES, ResponseScript::send(HEAD_500, INTERNAL_BODY), None, None, Expected::Failure(Category::Internal, "internal")),
    // These rows change one envelope binding at a time; the other fields remain canonical.
    row("call-wrong-status", A_D8, ResponseScript::send(HEAD_501, INTERNAL_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("call-wrong-message", A_D8, ResponseScript::send(HEAD_500, WRONG_MESSAGE_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("call-unknown-code", A_D8, ResponseScript::send(HEAD_500, UNKNOWN_CODE_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("call-wrong-kind", A_D8, ResponseScript::send(HEAD_500, WRONG_KIND_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("call-extra-field", A_D8, ResponseScript::send(HEAD_500, EXTRA_CALL_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("result-on-error-status", A_D8, ResponseScript::send(HEAD_400, OK_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("result-extra-top-field", A_D8, ResponseScript::send(HEAD_200, EXTRA_RESULT_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("result-extra-inner-field", A_D8, ResponseScript::send(HEAD_200, EXTRA_INNER_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("domain-envelope-on-200", A_D8, ResponseScript::send(HEAD_200, EMPTY_DOMAIN_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("domain-kind-not-domain", A_D8, ResponseScript::send(HEAD_422, WRONG_DOMAIN_KIND_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("domain-extra-field", A_D8, ResponseScript::send(HEAD_422, EXTRA_DOMAIN_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    // Missing and duplicate headers isolate the response content-type cardinality check.
    row("missing-content-type", A_D6_D8, ResponseScript::send(HEAD_MISSING, OK_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("text-json-content-type", A_D6_D8, ResponseScript::send(HEAD_TEXT_JSON, OK_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("charset-content-type", A_D6_D8, ResponseScript::send(HEAD_CHARSET, OK_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    // Redirect policy is pinned by the watched second accept; no follow-up connection is allowed.
    row("redirect-302", A_D7_D8, ResponseScript::redirect(HEAD_302, INTERNAL_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    // The result-shaped bytes are deliberately sent to exercise the status routing arm.
    row("no-content-204", A_D8, ResponseScript::send(HEAD_204, OK_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("bom-prefixed-response", A_D2_D8, ResponseScript::send(HEAD_200, BOM_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("malformed-json", A_D2_D8, ResponseScript::send(HEAD_200, MALFORMED_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    // The short body must fail in reqwest's chunk stream; the sentinel must not enter diagnostics.
    row("truncated-body", A_D7_D8, ResponseScript::send(HEAD_TRUNCATED, TRUNCATED_BODY), None, None, Expected::Failure(Category::InvalidResponse, "http_response")),
    // Three independent byte caps defend this row; M13 must remove declared, streamed, and parse caps together.
    row("oversized-streamed", A_D7_D8, ResponseScript::send(HEAD_CHUNKED, OVERSIZED_CHUNKED_BODY), Some((64, 128)), None, Expected::Failure(Category::InvalidResponse, "http_response")),
    row("connect-refused", A_D7_D8, ResponseScript::Hold, None, None, Expected::Failure(Category::Unavailable, "http_transport")),
    // The Hold server never completes; this is the deadline arm of the biased race, not cancellation.
    row("deadline-vs-stall", A_D7_D8, ResponseScript::Hold, None, Some(Duration::from_millis(100)), Expected::Deadline),
];

#[derive(Debug)]
struct Observation {
    request: Vec<u8>,
    second_connection: bool,
}

#[tokio::test]
async fn raw_server_client_cases_are_canonical() {
    traceability_gate(&ROWS);
    for row in ROWS {
        run_row(row).await;
    }
}

#[test]
fn raw_server_traceability_is_structural_and_complete() {
    traceability_gate(&ROWS);
}

#[test]
fn raw_server_traceability_mutants_are_active() {
    let mut deleted = ROWS.to_vec();
    deleted.pop();
    assert_traceability_rejects(&deleted, "row deletion");

    let mut duplicate = ROWS.to_vec();
    duplicate[1].id = duplicate[0].id;
    assert_traceability_rejects(&duplicate, "id duplication");

    let mut authority_deleted = ROWS.to_vec();
    authority_deleted[0].authority = &[];
    assert_traceability_rejects(&authority_deleted, "authority deletion");

    let mut anchor_deleted = ROWS.to_vec();
    anchor_deleted[0].authority = &[D3];
    assert_traceability_rejects(&anchor_deleted, "anchor-less authority");

    let mut outcome_drift = ROWS.to_vec();
    outcome_drift[0].expected = Expected::Failure(Category::Unavailable, "http_response");
    assert_traceability_rejects(&outcome_drift, "expected-outcome drift");
}

fn assert_traceability_rejects(rows: &[RawServerRow], mutant: &str) {
    assert!(
        traceability_check(rows).is_err(),
        "{mutant} mutation stayed green"
    );
}

fn traceability_gate(rows: &[RawServerRow]) {
    traceability_check(rows).unwrap_or_else(|error| panic!("{error}"));
}

fn traceability_check(rows: &[RawServerRow]) -> Result<(), String> {
    if rows.len() != ROW_COUNT {
        return Err(format!(
            "raw-server table has {} rows, expected {ROW_COUNT}",
            rows.len()
        ));
    }
    let mut ids = BTreeSet::new();
    for row in rows {
        if row.id.is_empty() || !ids.insert(row.id) {
            return Err(format!("row id is empty or duplicated: {}", row.id));
        }
        validate_authority(row.id, row.authority)?;
        validate_expected(row.id, row.expected)?;
    }
    Ok(())
}

fn validate_authority(id: &str, authority: &[SpecParagraph]) -> Result<(), String> {
    let mut unique = BTreeSet::new();
    if authority.is_empty() || !authority.contains(&D8) {
        return Err(format!(
            "{id} has empty authority or lacks S3D8ClientBinding"
        ));
    }
    if authority.iter().any(|paragraph| !unique.insert(*paragraph)) {
        return Err(format!("{id} repeats an authority paragraph"));
    }
    Ok(())
}

fn validate_expected(id: &str, expected: Expected) -> Result<(), String> {
    match expected {
        Expected::Value("") => Err(format!("{id} has empty value")),
        Expected::DomainUnknownTag("") => Err(format!("{id} has empty tag")),
        Expected::Failure(category, code) if detail_category(code) != Some(category) => Err(
            format!("{id} has incompatible category/detail code: {category:?}/{code}"),
        ),
        _ => Ok(()),
    }
}

fn detail_category(code: &str) -> Option<Category> {
    Some(match code {
        "http_transport" | "unknown_box" | "unknown_capability" | "unavailable" => {
            Category::Unavailable
        }
        "invalid_request" | "payload_too_large" => Category::ContractViolation,
        "http_response"
        | "method_not_allowed"
        | "unsupported_media_type"
        | "invalid_upstream_response" => Category::InvalidResponse,
        "internal" => Category::Internal,
        _ => return None,
    })
}

async fn run_row(row: RawServerRow) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("raw server binds");
    let address = listener.local_addr().expect("raw server has address");
    if row.id == "connect-refused" {
        drop(listener);
        let actual = call(address, row.tuning, row.deadline).await;
        assert_expected(row.id, actual, row.expected);
        return;
    }

    let mut server = tokio::spawn(serve(listener, row.response));
    let actual = match timeout(IO_TIMEOUT, call(address, row.tuning, row.deadline)).await {
        Ok(actual) => actual,
        Err(_) => {
            server.abort();
            let _ = server.await;
            panic!("{} client call exceeded five seconds", row.id);
        }
    };
    assert_expected(row.id, actual, row.expected);

    if matches!(row.response, ResponseScript::Hold) {
        server.abort();
        assert!(
            server
                .await
                .expect_err("Hold server was not aborted")
                .is_cancelled()
        );
        return;
    }
    let observation = match timeout(IO_TIMEOUT, &mut server).await {
        Ok(Ok(observation)) => observation,
        Ok(Err(error)) => panic!("{} server failed: {error}", row.id),
        Err(_) => {
            server.abort();
            let _ = server.await;
            panic!("{} server exceeded five seconds", row.id);
        }
    };
    if row.id == "redirect-302" {
        assert!(
            !observation.second_connection,
            "redirect followed unexpectedly"
        );
    }
    if matches!(row.expected, Expected::Value(_)) {
        assert_contractual_request(&observation.request);
    }
}

async fn call(
    address: std::net::SocketAddr,
    tuning: Option<(usize, usize)>,
    deadline: Option<Duration>,
) -> Result<String, CallError<GreetError>> {
    let mut config = HttpClientConfig::new(format!("http://{address}"))
        .expect("raw server address is a valid HTTP origin");
    if let Some((bytes, depth)) = tuning {
        config = config.with_response_limits(bytes, depth);
    }
    let target = HttpClientTarget::new(config, [hello_greet()])
        .expect("generated hello.greet conforms to HTTP client binding");
    let handle = HelloHandle::from_erased(Arc::new(target));
    handle.greet(context(deadline), "Ada".to_owned()).await
}

fn context(deadline: Option<Duration>) -> CallContext {
    CallContext::new(
        Caller::Anonymous,
        deadline.map(|duration| Deadline::at(Instant::now() + duration)),
        CancelToken::new(),
        TraceContext::empty(),
        None,
    )
}

fn assert_expected(id: &str, actual: Result<String, CallError<GreetError>>, expected: Expected) {
    let formatted = format!("{actual:?}");
    match (actual, expected) {
        (Ok(value), Expected::Value(want)) => assert_eq!(value, want, "{id}"),
        (Err(CallError::Domain(GreetError::EmptyName)), Expected::DomainEmptyName) => {}
        (
            Err(CallError::Domain(GreetError::Unknown { tag, .. })),
            Expected::DomainUnknownTag(want),
        ) => {
            assert_eq!(tag, want, "{id}");
        }
        (Err(error), Expected::Failure(category, code)) => match (category, error) {
            (Category::Unavailable, CallError::Unavailable(detail))
            | (Category::ContractViolation, CallError::ContractViolation(detail))
            | (Category::InvalidResponse, CallError::InvalidResponse(detail))
            | (Category::Internal, CallError::Internal(detail)) => {
                assert_detail(id, &detail, code);
            }
            (category, error) => panic!("{id}: expected {category:?}, got {error:?}"),
        },
        (Err(CallError::Deadline), Expected::Deadline) => {}
        (actual, expected) => panic!("{id}: expected {expected:?}, got {actual:?}"),
    }
    assert!(
        !formatted.contains(SENTINEL),
        "{id} echoed a sentinel: {formatted}"
    );
}

fn assert_detail(id: &str, detail: &Detail, code: &str) {
    assert_eq!(detail.code(), code, "{id} detail code");
}

async fn serve(listener: TcpListener, script: ResponseScript) -> Observation {
    let (mut stream, _) = timeout(IO_TIMEOUT, listener.accept())
        .await
        .expect("raw server accept timed out")
        .expect("raw server accept failed");
    let request = read_request(&mut stream).await;
    let ResponseScript::Send {
        head,
        body,
        watch_second,
    } = script
    else {
        std::future::pending::<()>().await;
        unreachable!();
    };
    write_response(&mut stream, head, body).await;
    let second_connection = if watch_second {
        match timeout(Duration::from_millis(200), listener.accept()).await {
            Ok(Ok((mut second, _))) => {
                write_response(&mut second, HEAD_200, OK_BODY).await;
                true
            }
            Ok(Err(error)) => panic!("redirect watch failed: {error}"),
            Err(_) => false,
        }
    } else {
        false
    };
    Observation {
        request,
        second_connection,
    }
}

async fn write_response(stream: &mut TcpStream, head: &'static [u8], body: &'static [u8]) {
    let _ = stream.write_all(head).await;
    for fragment in body.chunks(8) {
        tokio::task::yield_now().await;
        if stream.write_all(fragment).await.is_err() {
            break;
        }
    }
    let _ = stream.shutdown().await;
}

async fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let body_end = loop {
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            let head_end = index + 4;
            break head_end + content_length(&request[..index]).unwrap_or(0);
        }
        read_more(stream, &mut request).await;
    };
    while request.len() < body_end {
        read_more(stream, &mut request).await;
    }
    request
}

async fn read_more(stream: &mut TcpStream, bytes: &mut Vec<u8>) {
    let mut block = [0; 1024];
    let read = timeout(IO_TIMEOUT, stream.read(&mut block))
        .await
        .expect("request read timed out")
        .expect("request read failed");
    assert_ne!(read, 0, "request closed before its body arrived");
    bytes.extend_from_slice(&block[..read]);
}

fn content_length(head: &[u8]) -> Option<usize> {
    head.split(|byte| *byte == b'\n').find_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let colon = line.iter().position(|byte| *byte == b':')?;
        (line[..colon].eq_ignore_ascii_case(b"content-length"))
            .then(|| {
                std::str::from_utf8(line[colon + 1..].trim_ascii())
                    .ok()?
                    .parse()
                    .ok()
            })
            .flatten()
    })
}

fn assert_contractual_request(request: &[u8]) {
    let boundary = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("request has a head/body boundary");
    let head = &request[..boundary];
    assert_eq!(
        head.split(|byte| *byte == b'\n')
            .next()
            .unwrap()
            .trim_ascii(),
        b"POST /rpc/hello/greet HTTP/1.1"
    );
    assert_eq!(&request[boundary + 4..], br#""Ada""#);
    assert_header(head, b"content-type", b"application/json");
}

fn assert_header(head: &[u8], wanted: &[u8], value: &[u8]) {
    assert!(
        head.split(|byte| *byte == b'\n').any(|line| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let Some(colon) = line.iter().position(|byte| *byte == b':') else {
                return false;
            };
            line[..colon].eq_ignore_ascii_case(wanted) && line[colon + 1..].trim_ascii() == value
        }),
        "missing contractual header {wanted:?}: {value:?}"
    );
}
