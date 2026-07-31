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
const UNSUPPORTED_MEDIA_TYPE_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#;

const ROW_COUNT: usize = 34;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SpecParagraph {
    S3D3CanonicalResponseEncoding,
    S3D4RoutingAndIdentifierCanonicality,
    S3D5StableWireErrorCodes,
    S3D6HeaderGrammars,
    S3D7RequestProcessingPipeline,
    RuntimeInvocationStatusTable,
    RuntimeStableWireCodes,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RequestShape {
    method: &'static str,
    path: &'static str,
    content_type: Option<&'static str>,
    extra: &'static [&'static str],
}

impl RequestShape {
    const fn simple(method: &'static str, path: &'static str) -> Self {
        Self {
            method,
            path,
            content_type: Some("application/json"),
            extra: &[],
        }
    }
}
// ORACLE independently expands each shape into complete raw request bytes.

type Authority = &'static [SpecParagraph];
type RawBytes = &'static [u8];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExpectedResponse {
    status_line: RawBytes,
    body: RawBytes,
    content_type: RawBytes,
    allow: Option<RawBytes>,
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
// rule, authority, raw request, expected response.
struct OracleRow(&'static str, Authority, RawBytes, ExpectedResponse);

const fn e(
    status_line: RawBytes,
    body: RawBytes,
    content_type: RawBytes,
    allow: Option<RawBytes>,
    dispatches: usize,
) -> ExpectedResponse {
    ExpectedResponse {
        status_line,
        body,
        content_type,
        allow,
        dispatches,
    }
}

const JSON: &[u8] = b"application/json";
const POST: &[u8] = b"POST";
const OK_STATUS: &[u8] = b"HTTP/1.1 200 OK";
const NOT_FOUND_STATUS: &[u8] = b"HTTP/1.1 404 Not Found";
const BAD_REQUEST_STATUS: &[u8] = b"HTTP/1.1 400 Bad Request";
const METHOD_NOT_ALLOWED_STATUS: &[u8] = b"HTTP/1.1 405 Method Not Allowed";
const UNSUPPORTED_MEDIA_TYPE_STATUS: &[u8] = b"HTTP/1.1 415 Unsupported Media Type";

const UB: &[u8] = UNKNOWN_BOX_BODY;
const UCB: &[u8] = UNKNOWN_CAPABILITY_BODY;
const IRB: &[u8] = INVALID_REQUEST_BODY;
const MNB: &[u8] = METHOD_NOT_ALLOWED_BODY;
const UMTB: &[u8] = UNSUPPORTED_MEDIA_TYPE_BODY;
const SU: ExpectedResponse = e(OK_STATUS, SUCCESS_BODY, JSON, None, 1);
const BX: ExpectedResponse = e(NOT_FOUND_STATUS, UB, JSON, None, 0);
const CAP: ExpectedResponse = e(NOT_FOUND_STATUS, UCB, JSON, None, 0);
const BAD: ExpectedResponse = e(BAD_REQUEST_STATUS, IRB, JSON, None, 0);
const NA: ExpectedResponse = e(METHOD_NOT_ALLOWED_STATUS, MNB, JSON, Some(POST), 0);
const UMT: ExpectedResponse = e(UNSUPPORTED_MEDIA_TYPE_STATUS, UMTB, JSON, None, 0);

use SpecParagraph::{
    RuntimeInvocationStatusTable as STATUS, RuntimeStableWireCodes as CODES,
    S3D3CanonicalResponseEncoding as D3, S3D4RoutingAndIdentifierCanonicality as D4,
    S3D5StableWireErrorCodes as D5, S3D6HeaderGrammars as D6, S3D7RequestProcessingPipeline as D7,
};
const SUCCESS_AUTHORITY: &[SpecParagraph] = &[D3, D4];
const ROUTING_AUTHORITY: &[SpecParagraph] = &[D3, D4, D5, STATUS, CODES];
const METHOD_AUTHORITY: &[SpecParagraph] = &[D3, D5, D6, STATUS, CODES];
const ROUTE_PIPELINE_AUTHORITY: &[SpecParagraph] = &[D3, D4, D5, D7, STATUS, CODES];
const QUERY_PIPELINE_AUTHORITY: &[SpecParagraph] = &[D3, D4, D5, D6, D7, STATUS, CODES];
const HEAD_ADMISSION_AUTHORITY: &[SpecParagraph] = &[D3, D5, D6, D7, STATUS, CODES];
const SA: &[SpecParagraph] = SUCCESS_AUTHORITY;
const RA: &[SpecParagraph] = ROUTING_AUTHORITY;
const MA: &[SpecParagraph] = METHOD_AUTHORITY;
const PA: &[SpecParagraph] = ROUTE_PIPELINE_AUTHORITY;
const QA: &[SpecParagraph] = QUERY_PIPELINE_AUTHORITY;
const HA: &[SpecParagraph] = HEAD_ADMISSION_AUTHORITY;

const EXACT_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/hello/greet");
const UNKNOWN_BOX_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/ghost/greet");
const UNKNOWN_CAPABILITY_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/hello/ghost");
const ESCAPED_BOX_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/hell%6F/greet");
const ESCAPED_CAPABILITY_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/hello/gree%74");
const UPPERCASE_PREFIX_REQUEST: RequestShape = RequestShape::simple("POST", "/RPC/hello/greet");
const UPPERCASE_BOX_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/Hello/greet");
const UPPERCASE_CAPABILITY_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/hello/Greet");
const TRAILING_SLASH_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/hello/greet/");
const QUERY_REQUEST: RequestShape = RequestShape::simple("POST", "/rpc/hello/greet?probe=1");
const GET_REQUEST: RequestShape = RequestShape::simple("GET", "/rpc/hello/greet");
const OPTIONS_REQUEST: RequestShape = RequestShape::simple("OPTIONS", "/rpc/hello/greet");
const UNKNOWN_ROUTE_GET_REQUEST: RequestShape = RequestShape::simple("GET", "/rpc/ghost/greet");
const PUT_REQUEST: RequestShape = RequestShape::simple("PUT", "/rpc/hello/greet");

const MISSING_CONTENT_TYPE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: None,
    extra: &[],
};
const TEXT_PLAIN_MEDIA_TYPE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("text/plain"),
    extra: &[],
};
const JSON_SUFFIX_MEDIA_TYPE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    // application/json + structured-suffix: subtype remains `json`, so only the
    // suffix rejection arm can refuse it (unlike application/vnd.api+json).
    content_type: Some("application/json+foo"),
    extra: &[],
};
const WRONG_CHARSET_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json; charset=latin-1"),
    extra: &[],
};
const CHARSET_UTF8_ACCEPTED_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json; charset=utf-8"),
    extra: &[],
};
const CHARSET_UTF8_CASE_ACCEPTED_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json; charset=UTF-8"),
    extra: &[],
};
const TRAILING_SEMICOLON_MEDIA_TYPE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json;"),
    extra: &[],
};
const DUPLICATE_CONTENT_TYPE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Content-Type: application/json"],
};
const COMMA_JOINED_CONTENT_TYPE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json, application/json"),
    extra: &[],
};
const CONTENT_ENCODING_IDENTITY_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Content-Encoding: identity"],
};
const CONTENT_ENCODING_GZIP_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Content-Encoding: gzip"],
};
const TIMEOUT_NON_DIGIT_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Boxology-Timeout-Ms: soon"],
};
const TIMEOUT_LEADING_ZERO_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Boxology-Timeout-Ms: 01"],
};
const TIMEOUT_ELEVEN_DIGITS_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    // Eleven digits (`10000000000`) is rejected by two independent checks in
    // `parse_timeout`: the digit-length cap (`bytes.len() > 10`) and the
    // millisecond ceiling (`millis > 9_999_999_999`). A single-site mutant of
    // either arm stays green; proving this row requires the compound mutant
    // that relaxes both at once.
    extra: &["Boxology-Timeout-Ms: 10000000000"],
};
const TIMEOUT_EMBEDDED_SPACE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Boxology-Timeout-Ms: 60 000"],
};
const TIMEOUT_DUPLICATE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Boxology-Timeout-Ms: 1000", "Boxology-Timeout-Ms: 2000"],
};
const TIMEOUT_MAX_VALID_ACCEPTED_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Boxology-Timeout-Ms: 9999999999"],
};
const IDEMPOTENCY_DUPLICATE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Idempotency-Key: alpha", "Idempotency-Key: beta"],
};
const IDEMPOTENCY_EMPTY_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    // Empty `Idempotency-Key` is rejected by two independent checks: the
    // `bytes.is_empty()` arm in `parse_idempotency_key` (`server.rs` idempotency
    // path, not the timeout `bytes.is_empty()`), and `IdempotencyKey::new("")`.
    // A single-site mutant of either stays green; proving this row requires the
    // compound mutant that disables both at once.
    extra: &["Idempotency-Key:"],
};
const IDEMPOTENCY_OBS_TEXT_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    content_type: Some("application/json"),
    extra: &["Idempotency-Key: café"],
};

const RAW_CASES: [RawCase; ROW_COUNT] = [
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
    RawCase::new("put-method", PUT_REQUEST, MA, NA),
    RawCase::new(
        "missing-content-type",
        MISSING_CONTENT_TYPE_REQUEST,
        HA,
        UMT,
    ),
    RawCase::new(
        "text-plain-media-type",
        TEXT_PLAIN_MEDIA_TYPE_REQUEST,
        HA,
        UMT,
    ),
    RawCase::new(
        "json-suffix-media-type",
        JSON_SUFFIX_MEDIA_TYPE_REQUEST,
        HA,
        UMT,
    ),
    RawCase::new("wrong-charset", WRONG_CHARSET_REQUEST, HA, UMT),
    RawCase::new(
        "charset-utf8-accepted",
        CHARSET_UTF8_ACCEPTED_REQUEST,
        HA,
        SU,
    ),
    RawCase::new(
        "charset-utf8-case-accepted",
        CHARSET_UTF8_CASE_ACCEPTED_REQUEST,
        HA,
        SU,
    ),
    RawCase::new(
        "trailing-semicolon-media-type",
        TRAILING_SEMICOLON_MEDIA_TYPE_REQUEST,
        HA,
        UMT,
    ),
    RawCase::new(
        "duplicate-content-type",
        DUPLICATE_CONTENT_TYPE_REQUEST,
        HA,
        BAD,
    ),
    RawCase::new(
        "comma-joined-content-type",
        COMMA_JOINED_CONTENT_TYPE_REQUEST,
        HA,
        BAD,
    ),
    RawCase::new(
        "content-encoding-identity",
        CONTENT_ENCODING_IDENTITY_REQUEST,
        HA,
        UMT,
    ),
    RawCase::new(
        "content-encoding-gzip",
        CONTENT_ENCODING_GZIP_REQUEST,
        HA,
        UMT,
    ),
    RawCase::new("timeout-non-digit", TIMEOUT_NON_DIGIT_REQUEST, HA, BAD),
    RawCase::new(
        "timeout-leading-zero",
        TIMEOUT_LEADING_ZERO_REQUEST,
        HA,
        BAD,
    ),
    RawCase::new(
        "timeout-eleven-digits",
        TIMEOUT_ELEVEN_DIGITS_REQUEST,
        HA,
        BAD,
    ),
    RawCase::new(
        "timeout-embedded-space",
        TIMEOUT_EMBEDDED_SPACE_REQUEST,
        HA,
        BAD,
    ),
    RawCase::new("timeout-duplicate", TIMEOUT_DUPLICATE_REQUEST, HA, BAD),
    RawCase::new(
        "timeout-max-valid-accepted",
        TIMEOUT_MAX_VALID_ACCEPTED_REQUEST,
        HA,
        SU,
    ),
    RawCase::new(
        "idempotency-duplicate",
        IDEMPOTENCY_DUPLICATE_REQUEST,
        HA,
        BAD,
    ),
    RawCase::new("idempotency-empty", IDEMPOTENCY_EMPTY_REQUEST, HA, BAD),
    RawCase::new(
        "idempotency-obs-text",
        IDEMPOTENCY_OBS_TEXT_REQUEST,
        HA,
        BAD,
    ),
];

const fn o(r: &'static str, a: Authority, q: RawBytes, e: ExpectedResponse) -> OracleRow {
    OracleRow(r, a, q, e)
}

// Independently pinned: do not derive this oracle from RAW_CASES, its request renderer, or its
// response constants. Every row carries exact raw bytes, normative response expectations, dispatch
// count, and fully qualified authority.
const ORACLE: [OracleRow; ROW_COUNT] = [
    o("exact-success", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 200 OK", br#"{"result":{"value":"Hello, Ada!"}}"#, b"application/json", None, 1)),
    o("unknown-box", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/ghost/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, b"application/json", None, 0)),
    o("unknown-capability", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/ghost HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#, b"application/json", None, 0)),
    o("percent-encoded-box", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hell%6F/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, b"application/json", None, 0)),
    o("percent-encoded-capability", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/gree%74 HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#, b"application/json", None, 0)),
    o("uppercase-prefix", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /RPC/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, b"application/json", None, 0)),
    o("uppercase-box", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/Hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, b"application/json", None, 0)),
    o("uppercase-capability", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/Greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#, b"application/json", None, 0)),
    o("trailing-slash", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet/ HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#, b"application/json", None, 0)),
    o("query-string", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet?probe=1 HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("get-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"GET /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 405 Method Not Allowed", br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#, b"application/json", Some(b"POST"), 0)),
    o("options-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"OPTIONS /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 405 Method Not Allowed", br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#, b"application/json", Some(b"POST"), 0)),
    o("unknown-route-wrong-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"GET /rpc/ghost/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, b"application/json", None, 0)),
    o("put-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"PUT /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 405 Method Not Allowed", br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#, b"application/json", Some(b"POST"), 0)),
    o("missing-content-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("text-plain-media-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("json-suffix-media-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json+foo\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("wrong-charset", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json; charset=latin-1\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("charset-utf8-accepted", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json; charset=utf-8\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 200 OK", br#"{"result":{"value":"Hello, Ada!"}}"#, b"application/json", None, 1)),
    o("charset-utf8-case-accepted", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json; charset=UTF-8\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 200 OK", br#"{"result":{"value":"Hello, Ada!"}}"#, b"application/json", None, 1)),
    o("trailing-semicolon-media-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json;\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("duplicate-content-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("comma-joined-content-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json, application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("content-encoding-identity", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nContent-Encoding: identity\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("content-encoding-gzip", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("timeout-non-digit", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: soon\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-leading-zero", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 01\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-eleven-digits", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 10000000000\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-embedded-space", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 60 000\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-duplicate", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 1000\r\nBoxology-Timeout-Ms: 2000\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-max-valid-accepted", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 9999999999\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 200 OK", br#"{"result":{"value":"Hello, Ada!"}}"#, b"application/json", None, 1)),
    o("idempotency-duplicate", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nIdempotency-Key: alpha\r\nIdempotency-Key: beta\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("idempotency-empty", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nIdempotency-Key:\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("idempotency-obs-text", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nIdempotency-Key: caf\xc3\xa9\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
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
    percent_escape_drift[3].request = RequestShape::simple("POST", "/rpc/ghost/greet");
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

    let mut content_type_drift = ORACLE;
    content_type_drift[0].3.content_type = b"text/plain";
    assert_traceability_rejects(&RAW_CASES, &content_type_drift, "oracle content-type drift");

    let mut allow_drift = ORACLE;
    allow_drift[10].3.allow = Some(b"GET");
    assert_traceability_rejects(&RAW_CASES, &allow_drift, "oracle Allow value drift");

    let mut weakened_allow = ORACLE;
    weakened_allow[10].3.allow = None;
    assert_traceability_rejects(&RAW_CASES, &weakened_allow, "weakened Allow oracle");

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

    let mut extra_header_drift = RAW_CASES.to_vec();
    extra_header_drift[23].request.extra = &["Content-Encoding: br"];
    assert_traceability_rejects(&extra_header_drift, &ORACLE, "extra-header-line drift");

    let mut umt_status_drift = RAW_CASES.to_vec();
    umt_status_drift[14].expected.status_line = b"HTTP/1.1 415 Mismatched Reason";
    assert_traceability_rejects(&umt_status_drift, &ORACLE, "415 status-line drift");

    let mut umt_body_drift = RAW_CASES.to_vec();
    umt_body_drift[14].expected.body = INVALID_REQUEST_BODY;
    assert_traceability_rejects(&umt_body_drift, &ORACLE, "UMT body drift");

    let mut accepted_dispatch_drift = RAW_CASES.to_vec();
    accepted_dispatch_drift[30].expected.dispatches = 0;
    assert_traceability_rejects(
        &accepted_dispatch_drift,
        &ORACLE,
        "accepted-row dispatch drift",
    );

    let mut media_authority_deletion = RAW_CASES.to_vec();
    media_authority_deletion[14].authority = &[
        SpecParagraph::S3D3CanonicalResponseEncoding,
        SpecParagraph::S3D5StableWireErrorCodes,
        SpecParagraph::S3D6HeaderGrammars,
        SpecParagraph::RuntimeInvocationStatusTable,
        SpecParagraph::RuntimeStableWireCodes,
    ];
    assert_traceability_rejects(
        &media_authority_deletion,
        &ORACLE,
        "media-row authority D7 deletion",
    );
}

fn assert_traceability_rejects(cases: &[RawCase], oracle: &[OracleRow], mutant: &str) {
    assert!(
        traceability_check(cases, oracle).is_err(),
        "{mutant} mutation stayed green"
    );
}

fn traceability_gate(cases: &[RawCase], oracle: &[OracleRow]) {
    traceability_check(cases, oracle).unwrap_or_else(|error| panic!("{error}"));
}

fn traceability_check(cases: &[RawCase], oracle: &[OracleRow]) -> Result<(), String> {
    if cases.len() != ROW_COUNT {
        return Err(format!(
            "raw Hello table has {} rows, expected {ROW_COUNT}",
            cases.len()
        ));
    }
    if oracle.len() != ROW_COUNT {
        return Err(format!(
            "independent oracle has {} rows, expected {ROW_COUNT}",
            oracle.len()
        ));
    }

    let mut case_rules = BTreeSet::new();
    for case in cases {
        validate_rule(case.id, case.authority, &mut case_rules, "raw")?;
    }

    let mut oracle_rules = BTreeSet::new();
    for row in oracle {
        validate_rule(row.0, row.1, &mut oracle_rules, "oracle")?;
    }

    for (index, (case, golden)) in cases.iter().zip(oracle).enumerate() {
        if case.id != golden.0 {
            return Err(format!(
                "row {index} rule drift: {} != {}",
                case.id, golden.0
            ));
        }
        if case.authority != golden.1 {
            return Err(format!("row {index} authority drift: {}", case.id));
        }
        if render_request(case.request).as_slice() != golden.2 {
            return Err(format!("row {index} raw request drift: {}", case.id));
        }
        if case.expected != golden.3 {
            return Err(format!(
                "row {index} response or outcome drift: {}",
                case.id
            ));
        }
    }
    Ok(())
}

fn validate_rule(
    rule: &'static str,
    authority: &[SpecParagraph],
    seen: &mut BTreeSet<&'static str>,
    kind: &str,
) -> Result<(), String> {
    if rule.is_empty() || !seen.insert(rule) {
        return Err(format!("{kind} rule is empty or duplicated: {rule}"));
    }
    validate_authority(rule, authority)
}

fn validate_authority(rule: &str, authority: &[SpecParagraph]) -> Result<(), String> {
    let mut unique = BTreeSet::new();
    if authority.is_empty()
        || !authority.contains(&SpecParagraph::S3D3CanonicalResponseEncoding)
        || authority.iter().any(|paragraph| !unique.insert(*paragraph))
    {
        return Err(format!("{rule} has invalid or repeated authority"));
    }
    Ok(())
}

fn render_request(request: RequestShape) -> Vec<u8> {
    let mut rendered = format!(
        "{} {} HTTP/1.1\r\nHost: boxology\r\n",
        request.method, request.path
    )
    .into_bytes();
    if let Some(content_type) = request.content_type {
        rendered.extend_from_slice(b"Content-Type: ");
        rendered.extend_from_slice(content_type.as_bytes());
        rendered.extend_from_slice(b"\r\n");
    }
    for line in request.extra {
        rendered.extend_from_slice(line.as_bytes());
        rendered.extend_from_slice(b"\r\n");
    }
    rendered.extend_from_slice(
        format!(
            "Connection: close\r\nContent-Length: {}\r\n\r\n",
            REQUEST_BODY.len()
        )
        .as_bytes(),
    );
    rendered.extend_from_slice(REQUEST_BODY);
    rendered
}

async fn raw_exchange(address: std::net::SocketAddr, request: &[u8]) -> Vec<u8> {
    timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.expect("connect");
        stream.write_all(request).await.expect("write");
        let mut response = Vec::new();
        if let Err(error) = stream.read_to_end(&mut response).await {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        }
        response
    })
    .await
    .expect("timeout")
}

fn assert_response(raw: &[u8], expected: ExpectedResponse) {
    let (head, body) = split_response(raw);
    let line_end = head
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("valid HTTP response has a status line");
    let status_line = head[..line_end]
        .strip_suffix(b"\r")
        .unwrap_or(&head[..line_end]);
    assert_eq!(status_line, expected.status_line);
    assert_eq!(body, expected.body);

    let headers = parse_headers(&head[line_end + 1..]);
    assert_header(&headers, b"content-type", Some(expected.content_type));
    assert_header(&headers, b"allow", expected.allow);
}

fn split_response(raw: &[u8]) -> (&[u8], &[u8]) {
    let boundary = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| {
            raw.windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2))
        })
        .expect("valid HTTP response has a header/body boundary");
    (&raw[..boundary.0], &raw[boundary.0 + boundary.1..])
}

fn parse_headers(block: &[u8]) -> Vec<(&[u8], &[u8])> {
    block
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let colon = line
                .iter()
                .position(|byte| *byte == b':')
                .expect("valid HTTP response header has a name and value");
            (&line[..colon], line[colon + 1..].trim_ascii())
        })
        .collect()
}

fn assert_header(headers: &[(&[u8], &[u8])], name: &[u8], expected: Option<&[u8]>) {
    let actual: Vec<_> = headers
        .iter()
        .filter(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
        .collect();
    match expected {
        Some(value) => assert!(
            !actual.is_empty() && actual.iter().all(|actual| *actual == value),
            "{name:?} value drift: {actual:?}"
        ),
        None => assert!(actual.is_empty(), "unexpected {name:?}: {actual:?}"),
    }
}
