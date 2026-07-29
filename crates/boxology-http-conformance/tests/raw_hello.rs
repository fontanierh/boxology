#[allow(dead_code)]
mod support;

use std::{
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use boxology_contract::CallContext;
use hello_contract::{GreetError, HelloDispatch};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use support::RunningHello;

const REQUEST_BODY: &[u8] = br#""Ada""#;
const SUCCESS_BODY: &[u8] = br#"{"result":{"value":"Hello, Ada!"}}"#;
const UNKNOWN_BOX_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#;
const UNKNOWN_CAPABILITY_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#;
const INVALID_REQUEST_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#;
const METHOD_NOT_ALLOWED_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SpecParagraph {
    S3D3CanonicalResponseEncoding,
    S3D4RoutingAndIdentifierCanonicality,
    S3D5StableWireErrorCodes,
    S3D6MethodTable,
    S3D7RequestProcessingPipeline,
    RuntimeInvocationStatusTable,
    RuntimeStableWireCodes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RequestShape(&'static str, &'static str);
// ORACLE independently expands each shape into complete raw request bytes.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExpectedResponse {
    status_line: &'static [u8],
    body: &'static [u8],
    allow: Option<&'static [u8]>,
    dispatches: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawCase {
    id: &'static str,
    request: RequestShape,
    authority: &'static [SpecParagraph],
    expected: ExpectedResponse,
}

impl RawCase {
    const fn new(
        id: &'static str,
        request: RequestShape,
        authority: &'static [SpecParagraph],
        expected: ExpectedResponse,
    ) -> Self {
        Self {
            id,
            request,
            authority,
            expected,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OracleRow {
    rule: &'static str,
    authority: &'static [SpecParagraph],
    request: &'static [u8],
    expected: ExpectedResponse,
}

const fn expected(
    status_line: &'static [u8],
    body: &'static [u8],
    allow: Option<&'static [u8]>,
    dispatches: usize,
) -> ExpectedResponse {
    ExpectedResponse {
        status_line,
        body,
        allow,
        dispatches,
    }
}

const SUCCESS: ExpectedResponse = expected(b"HTTP/1.1 200 OK", SUCCESS_BODY, None, 1);
const UNKNOWN_BOX: ExpectedResponse =
    expected(b"HTTP/1.1 404 Not Found", UNKNOWN_BOX_BODY, None, 0);
const UNKNOWN_CAPABILITY: ExpectedResponse =
    expected(b"HTTP/1.1 404 Not Found", UNKNOWN_CAPABILITY_BODY, None, 0);
const INVALID_REQUEST: ExpectedResponse =
    expected(b"HTTP/1.1 400 Bad Request", INVALID_REQUEST_BODY, None, 0);
const METHOD_NOT_ALLOWED: ExpectedResponse = expected(
    b"HTTP/1.1 405 Method Not Allowed",
    METHOD_NOT_ALLOWED_BODY,
    Some(b"POST"),
    0,
);

use SpecParagraph::{
    RuntimeInvocationStatusTable as STATUS, RuntimeStableWireCodes as CODES,
    S3D3CanonicalResponseEncoding as D3, S3D4RoutingAndIdentifierCanonicality as D4,
    S3D5StableWireErrorCodes as D5, S3D6MethodTable as D6, S3D7RequestProcessingPipeline as D7,
};
const SUCCESS_AUTHORITY: &[SpecParagraph] = &[D3, D4];
const ROUTING_AUTHORITY: &[SpecParagraph] = &[D3, D4, D5, STATUS, CODES];
const METHOD_AUTHORITY: &[SpecParagraph] = &[D3, D5, D6, STATUS, CODES];
const ROUTE_PIPELINE_AUTHORITY: &[SpecParagraph] = &[D3, D4, D5, D7, STATUS, CODES];
const QUERY_PIPELINE_AUTHORITY: &[SpecParagraph] = &[D3, D4, D5, D6, D7, STATUS, CODES];
const SA: &[SpecParagraph] = SUCCESS_AUTHORITY;
const RA: &[SpecParagraph] = ROUTING_AUTHORITY;
const MA: &[SpecParagraph] = METHOD_AUTHORITY;
const PA: &[SpecParagraph] = ROUTE_PIPELINE_AUTHORITY;
const QA: &[SpecParagraph] = QUERY_PIPELINE_AUTHORITY;
const SU: ExpectedResponse = SUCCESS;
const BX: ExpectedResponse = UNKNOWN_BOX;
const CAP: ExpectedResponse = UNKNOWN_CAPABILITY;
const BAD: ExpectedResponse = INVALID_REQUEST;
const NA: ExpectedResponse = METHOD_NOT_ALLOWED;

const EXACT_REQUEST: RequestShape = RequestShape("POST", "/rpc/hello/greet");
const UNKNOWN_BOX_REQUEST: RequestShape = RequestShape("POST", "/rpc/ghost/greet");
const UNKNOWN_CAPABILITY_REQUEST: RequestShape = RequestShape("POST", "/rpc/hello/ghost");
const ESCAPED_BOX_REQUEST: RequestShape = RequestShape("POST", "/rpc/hell%6F/greet");
const ESCAPED_CAPABILITY_REQUEST: RequestShape = RequestShape("POST", "/rpc/hello/gree%74");
const UPPERCASE_PREFIX_REQUEST: RequestShape = RequestShape("POST", "/RPC/hello/greet");
const UPPERCASE_BOX_REQUEST: RequestShape = RequestShape("POST", "/rpc/Hello/greet");
const UPPERCASE_CAPABILITY_REQUEST: RequestShape = RequestShape("POST", "/rpc/hello/Greet");
const TRAILING_SLASH_REQUEST: RequestShape = RequestShape("POST", "/rpc/hello/greet/");
const QUERY_REQUEST: RequestShape = RequestShape("POST", "/rpc/hello/greet?probe=1");
const GET_REQUEST: RequestShape = RequestShape("GET", "/rpc/hello/greet");
const OPTIONS_REQUEST: RequestShape = RequestShape("OPTIONS", "/rpc/hello/greet");
const UNKNOWN_ROUTE_GET_REQUEST: RequestShape = RequestShape("GET", "/rpc/ghost/greet");
const QUERY_GET_REQUEST: RequestShape = RequestShape("GET", "/rpc/hello/greet?probe=1");

const RAW_CASES: [RawCase; 14] = [
    RawCase::new("exact-success", EXACT_REQUEST, SA, SU),
    RawCase::new("unknown-box", UNKNOWN_BOX_REQUEST, RA, BX),
    RawCase::new("unknown-capability", UNKNOWN_CAPABILITY_REQUEST, RA, CAP),
    RawCase::new("percent-encoded-box", ESCAPED_BOX_REQUEST, RA, BX),
    RawCase::new(
        "percent-encoded-capability",
        ESCAPED_CAPABILITY_REQUEST,
        RA,
        CAP,
    ),
    RawCase::new("uppercase-prefix", UPPERCASE_PREFIX_REQUEST, RA, BX),
    RawCase::new("uppercase-box", UPPERCASE_BOX_REQUEST, RA, BX),
    RawCase::new(
        "uppercase-capability",
        UPPERCASE_CAPABILITY_REQUEST,
        RA,
        CAP,
    ),
    RawCase::new("trailing-slash", TRAILING_SLASH_REQUEST, RA, CAP),
    RawCase::new("query-string", QUERY_REQUEST, QA, BAD),
    RawCase::new("get-method", GET_REQUEST, MA, NA),
    RawCase::new("options-method", OPTIONS_REQUEST, MA, NA),
    RawCase::new(
        "unknown-route-wrong-method",
        UNKNOWN_ROUTE_GET_REQUEST,
        PA,
        BX,
    ),
    RawCase::new("query-wrong-method", QUERY_GET_REQUEST, QA, BAD),
];

const fn oracle(
    rule: &'static str,
    authority: &'static [SpecParagraph],
    request: &'static [u8],
    expected: ExpectedResponse,
) -> OracleRow {
    OracleRow {
        rule,
        authority,
        request,
        expected,
    }
}

// Independently pinned: do not derive this oracle from RAW_CASES, its request renderer, or its
// response constants. Every row carries exact raw bytes, outcome, dispatch count, and authority.
const ORACLE: [OracleRow; 14] = [
    oracle("exact-success", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 200 OK", br#"{"result":{"value":"Hello, Ada!"}}"#, None, 1)),
    oracle("unknown-box", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/ghost/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, None, 0)),
    oracle("unknown-capability", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/ghost HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#, None, 0)),
    oracle("percent-encoded-box", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hell%6F/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, None, 0)),
    oracle("percent-encoded-capability", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/gree%74 HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#, None, 0)),
    oracle("uppercase-prefix", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /RPC/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, None, 0)),
    oracle("uppercase-box", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/Hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, None, 0)),
    oracle("uppercase-capability", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/Greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#, None, 0)),
    oracle("trailing-slash", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet/ HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#, None, 0)),
    oracle("query-string", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6MethodTable, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet?probe=1 HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, None, 0)),
    oracle("get-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6MethodTable, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"GET /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 405 Method Not Allowed", br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#, Some(b"POST"), 0)),
    oracle("options-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6MethodTable, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"OPTIONS /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 405 Method Not Allowed", br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#, Some(b"POST"), 0)),
    oracle("unknown-route-wrong-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"GET /rpc/ghost/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, None, 0)),
    oracle("query-wrong-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6MethodTable, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"GET /rpc/hello/greet?probe=1 HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", expected(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, None, 0)),
];

struct CountingHello {
    dispatches: Arc<AtomicUsize>,
}

impl HelloDispatch for CountingHello {
    fn greet<'a>(
        &'a self,
        _context: CallContext,
        name: String,
    ) -> Pin<Box<dyn Future<Output = Result<String, GreetError>> + Send + 'a>> {
        self.dispatches.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(format!("Hello, {name}!")) })
    }
}

#[tokio::test]
async fn raw_hello_routing_method_query_cases_are_canonical() {
    traceability_gate(&RAW_CASES, &ORACLE);
    for case in RAW_CASES {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let running = RunningHello::start(CountingHello {
            dispatches: Arc::clone(&dispatches),
        });
        let address = running.local_addr();
        let request = render_request(case.request);

        running
            .assert_then_shutdown(async move {
                let response = raw_exchange(address, &request).await;
                assert_response(&response, case.expected);
                assert_eq!(
                    dispatches.load(Ordering::SeqCst),
                    case.expected.dispatches,
                    "{} dispatch count",
                    case.id
                );
            })
            .await;
    }
}

#[test]
fn raw_hello_traceability_is_independent_and_complete() {
    traceability_gate(&RAW_CASES, &ORACLE);
}

#[test]
fn traceability_semantic_drift_mutants_are_active() {
    let mut deleted_case = RAW_CASES.to_vec();
    deleted_case.pop();
    assert_traceability_rejects(&deleted_case, &ORACLE, "rule deletion");

    let mut duplicated_case = RAW_CASES.to_vec();
    duplicated_case[1].id = duplicated_case[0].id;
    assert_traceability_rejects(&duplicated_case, &ORACLE, "rule duplication");

    let mut percent_escape_drift = RAW_CASES.to_vec();
    percent_escape_drift[3].request = RequestShape("POST", "/rpc/ghost/greet");
    assert_traceability_rejects(
        &percent_escape_drift,
        &ORACLE,
        "percent escape became unknown route",
    );

    let mut status_drift = RAW_CASES.to_vec();
    status_drift[0].expected.status_line = b"HTTP/1.1 201 Created";
    assert_traceability_rejects(&status_drift, &ORACLE, "status drift");

    let mut body_drift = RAW_CASES.to_vec();
    body_drift[0].expected.body = br#"{"result":{"value":"Hello, Grace!"}}"#;
    assert_traceability_rejects(&body_drift, &ORACLE, "body drift");

    let mut allow_drift = RAW_CASES.to_vec();
    allow_drift[0].expected.allow = Some(b"POST");
    assert_traceability_rejects(&allow_drift, &ORACLE, "allow drift");

    let mut dispatch_drift = RAW_CASES.to_vec();
    dispatch_drift[0].expected.dispatches = 0;
    assert_traceability_rejects(&dispatch_drift, &ORACLE, "dispatch-count drift");

    let mut deleted_authority = RAW_CASES.to_vec();
    deleted_authority[0].authority = &[SpecParagraph::S3D4RoutingAndIdentifierCanonicality];
    assert_traceability_rejects(&deleted_authority, &ORACLE, "authority deletion");

    let mut duplicated_authority = RAW_CASES.to_vec();
    duplicated_authority[0].authority = &[
        SpecParagraph::S3D3CanonicalResponseEncoding,
        SpecParagraph::S3D3CanonicalResponseEncoding,
        SpecParagraph::S3D4RoutingAndIdentifierCanonicality,
    ];
    assert_traceability_rejects(&duplicated_authority, &ORACLE, "authority duplication");
}

fn assert_traceability_rejects(cases: &[RawCase], oracle: &[OracleRow], mutant: &str) {
    assert!(
        traceability_check(cases, oracle).is_err(),
        "{mutant} mutation stayed green"
    );
}

fn traceability_gate(cases: &[RawCase], oracle: &[OracleRow]) {
    if let Err(error) = traceability_check(cases, oracle) {
        panic!("{error}");
    }
}

fn traceability_check(cases: &[RawCase], oracle: &[OracleRow]) -> Result<(), String> {
    if cases.len() != 14 {
        return Err(format!(
            "raw Hello table has {} rows, expected 14",
            cases.len()
        ));
    }
    if oracle.len() != 14 {
        return Err(format!(
            "independent oracle has {} rows, expected 14",
            oracle.len()
        ));
    }

    let mut case_rules = BTreeSet::new();
    for case in cases {
        if case.id.is_empty() || !case_rules.insert(case.id) {
            return Err(format!("raw rule is empty or duplicated: {}", case.id));
        }
        validate_authority(case.id, case.authority)?;
    }

    let mut oracle_rules = BTreeSet::new();
    for row in oracle {
        if row.rule.is_empty() || !oracle_rules.insert(row.rule) {
            return Err(format!("oracle rule is empty or duplicated: {}", row.rule));
        }
        validate_authority(row.rule, row.authority)?;
    }

    for (index, (case, golden)) in cases.iter().zip(oracle).enumerate() {
        if case.id != golden.rule {
            return Err(format!(
                "row {index} rule drift: {} != {}",
                case.id, golden.rule
            ));
        }
        if case.authority != golden.authority {
            return Err(format!("row {index} authority drift: {}", case.id));
        }
        let request = render_request(case.request);
        if request.as_slice() != golden.request {
            return Err(format!("row {index} raw request drift: {}", case.id));
        }
        if case.expected != golden.expected {
            return Err(format!(
                "row {index} response or outcome drift: {}",
                case.id
            ));
        }
    }
    Ok(())
}

fn validate_authority(rule: &str, authority: &[SpecParagraph]) -> Result<(), String> {
    if authority.is_empty() {
        return Err(format!("{rule} has no authority"));
    }
    if !authority.contains(&SpecParagraph::S3D3CanonicalResponseEncoding) {
        return Err(format!("{rule} omits S3 D3 response authority"));
    }
    let mut unique = BTreeSet::new();
    if authority.iter().any(|paragraph| !unique.insert(*paragraph)) {
        return Err(format!("{rule} repeats an authority"));
    }
    Ok(())
}

fn render_request(request: RequestShape) -> Vec<u8> {
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: boxology\r\n\
         Content-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        request.0,
        request.1,
        REQUEST_BODY.len()
    )
    .into_bytes();
    request.extend_from_slice(REQUEST_BODY);
    request
}

async fn raw_exchange(address: std::net::SocketAddr, request: &[u8]) -> Vec<u8> {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address)
            .await
            .expect("raw Hello socket connects");
        stream
            .write_all(request)
            .await
            .expect("raw Hello request writes");
        let mut response = Vec::new();
        if let Err(error) = stream.read_to_end(&mut response).await {
            assert_eq!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset,
                "raw Hello response read failed"
            );
        }
        response
    })
    .await
    .expect("raw Hello exchange exceeded five seconds")
}

fn assert_response(raw: &[u8], expected: ExpectedResponse) {
    let line_end = raw
        .windows(2)
        .position(|window| window == b"\r\n")
        .expect("response is missing a CRLF status line");
    assert_eq!(&raw[..line_end], expected.status_line);
    let boundary = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response is missing the header/body boundary");
    assert!(boundary > line_end + 2, "response has no header block");
    let headers = parse_headers(&raw[line_end + 2..boundary]);
    let body = &raw[boundary + 4..];
    assert_eq!(body, expected.body);
    assert_eq!(
        header(&headers, b"content-type"),
        Some(&b"application/json"[..])
    );
    assert_eq!(header(&headers, b"connection"), Some(&b"close"[..]));
    let date = header(&headers, b"date").expect("HTTP/1.1 Date framing is required");
    assert!(valid_imf_fixdate(date), "Date is not IMF-fixdate: {date:?}");
    assert_eq!(header(&headers, b"transfer-encoding"), None);
    assert_eq!(header(&headers, b"allow"), expected.allow);
    let content_length = header(&headers, b"content-length").expect("content-length is required");
    assert_eq!(
        std::str::from_utf8(content_length)
            .expect("content-length is ASCII")
            .parse::<usize>()
            .expect("content-length is numeric"),
        body.len()
    );
    assert_eq!(
        headers.len(),
        4 + usize::from(expected.allow.is_some()),
        "response contains only canonical framing and contract headers"
    );
}

#[test]
fn imf_fixdate_edges_and_malformed_tokens_are_active() {
    const VALID: &[u8] = b"Wed, 29 Jul 2026 11:54:22 GMT|Thu, 29 Feb 2024 23:59:60 GMT|Wed, 28 Feb 1900 23:59:59 GMT|Tue, 29 Feb 2000 23:59:59 GMT";
    const INVALID: &[u8] = b"Mon, 31 Apr 2024 11:54:22 GMT|Sat, 29 Feb 2025 11:54:22 GMT|Wed, 29 Feb 1900 11:54:22 GMT|Tue, 28 Feb 2000 11:54:22 GMT|Thu, 29 Feb 2024 11:54:60 GMT|Thu, 29 Feb 2024 23:58:60 GMT|Thu, 29 Feb 2024 23:59:61 GMT|Xxx, 29 Jul 2026 11:54:22 GMT|Wed; 29 Jul 2026 11:54:22 GMT|Wed,29 Jul 2026 11:54:22 GMT|Wed, 00 Jul 2026 11:54:22 GMT|Wed, 32 Jul 2026 11:54:22 GMT|Wed, 29 Foo 2026 11:54:22 GMT|Wed, 29 Jul 20x6 11:54:22 GMT|Wed, 29 Jul 2026 24:54:22 GMT|Wed, 29 Jul 2026 11:60:22 GMT|Wed, 29 Jul 2026 11:54:22 UTC";
    for valid in VALID.split(|byte| *byte == b'|') {
        assert!(valid_imf_fixdate(valid), "rejected {valid:?}");
    }
    for invalid in INVALID.split(|byte| *byte == b'|') {
        assert!(!valid_imf_fixdate(invalid), "accepted {invalid:?}");
    }
}

fn valid_imf_fixdate(value: &[u8]) -> bool {
    const WEEKDAYS: [&[u8]; 7] = [b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat", b"Sun"];
    const MONTHS: [&[u8]; 12] = [
        b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov",
        b"Dec",
    ];
    if value.len() != 29
        || value[3..5] != *b", "
        || value[7] != b' '
        || value[11] != b' '
        || value[16] != b' '
        || value[19] != b':'
        || value[22] != b':'
        || value[25..29] != *b" GMT"
    {
        return false;
    }

    let weekday = match WEEKDAYS.iter().position(|name| *name == &value[..3]) {
        Some(weekday) => weekday as u8,
        None => return false,
    };
    let day = match decimal(&value[5..7]) {
        Some(day @ 1..=31) => day as u8,
        _ => return false,
    };
    let month = match MONTHS.iter().position(|name| *name == &value[8..11]) {
        Some(month) => month as u8 + 1,
        None => return false,
    };
    let year = match decimal(&value[12..16]) {
        Some(year) => year,
        None => return false,
    };
    if day > days_in_month(year, month) {
        return false;
    }
    let Some(hour) = decimal(&value[17..19]) else {
        return false;
    };
    let Some(minute) = decimal(&value[20..22]) else {
        return false;
    };
    let Some(second) = decimal(&value[23..25]) else {
        return false;
    };
    if hour > 23 || minute > 59 || second > 60 || second == 60 && (hour != 23 || minute != 59) {
        return false;
    }
    weekday == weekday_index(year, month, day)
}

fn decimal(digits: &[u8]) -> Option<u16> {
    digits.iter().all(u8::is_ascii_digit).then(|| {
        digits
            .iter()
            .fold(0, |value, digit| value * 10 + u16::from(*digit - b'0'))
    })
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn leap_year(year: u16) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn weekday_index(year: u16, month: u8, day: u8) -> u8 {
    let year = i64::from(year);
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    ((era * 146_097 + day_of_era - 719_468 + 3).rem_euclid(7)) as u8
}

fn parse_headers(block: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut headers = Vec::new();
    let mut lines = block.split(|byte| *byte == b'\n').peekable();
    while let Some(line) = lines.next() {
        let line = if lines.peek().is_some() {
            line.strip_suffix(b"\r")
                .unwrap_or_else(|| panic!("response headers use CRLF framing: {line:?}"))
        } else {
            line
        };
        headers.push({
            let colon = line
                .iter()
                .position(|byte| *byte == b':')
                .expect("response header has a colon");
            assert_eq!(
                line.get(colon + 1),
                Some(&b' '),
                "header uses canonical spacing"
            );
            (&line[..colon], &line[colon + 2..])
        });
    }
    headers
}

fn header<'a>(headers: &[(&'a [u8], &'a [u8])], name: &[u8]) -> Option<&'a [u8]> {
    let mut found = None;
    for &(header_name, value) in headers {
        if header_name.eq_ignore_ascii_case(name) {
            assert!(found.is_none(), "response contains duplicate {name:?}");
            found = Some(value);
        }
    }
    found
}
