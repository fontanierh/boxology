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
use boxology_http::HttpServerConfig;
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
const PAYLOAD_TOO_LARGE_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"payload_too_large","message":"payload too large"}}"#;
const DEADLINE_EXCEEDED_BODY: &[u8] =
    br#"{"error":{"kind":"call","code":"deadline_exceeded","message":"deadline exceeded"}}"#;

const EMPTY_BODY: &[u8] = b"";
const TRAILING_BYTES_BODY: &[u8] = b"\"Ada\" 0";
const BOM_PREFIXED_BODY: &[u8] = b"\xEF\xBB\xBF\"Ada\"";
const INVALID_UTF8_BODY: &[u8] = b"\"\xff\"";
const DEPTH_BOMB_BODY: &[u8] = b"[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[\"Ada\"]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]";
const OVERSIZED_STRING_BODY: &[u8] =
    br#""AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA""#;
const OVERSIZED_CHUNKED_BODY: &[u8] =
    b"43\r\n\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\r\n0\r\n\r\n";
const OVERSIZED_MALFORMED_BODY: &[u8] =
    b"!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!";
// Hello input is String: pipeline-classification only; shape isolation is semantic.rs.
const MALFORMED_JSON_BODY: &[u8] = b"{";
const DUPLICATE_KEY_OBJECT_BODY: &[u8] = br#"{"a":1,"a":1}"#;
const NONCANONICAL_INTEGER_BODY: &[u8] = b"007";

/// Hang budget for the 1 MiB default-limit boundary exchanges only.
///
/// Distinct from the shared 5s assertion timeout in `support/mod.rs`, which the
/// small-payload `RAW_CASES` suite still relies on. Under CI's parallel
/// `cargo test --workspace` load, a debug-build 1 MiB loopback round-trip
/// (collect + parse a JSON string + echo a ~1 MiB response) routinely exceeds
/// 5s even though the property under test is not timing-sensitive. Do not
/// "harmonise" this back to 5s.
const DEFAULT_BODY_LIMIT_BOUNDARY_TIMEOUT: Duration = Duration::from_secs(60);

const ROW_COUNT: usize = 50;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SpecParagraph {
    S3D2Codec,
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
struct ServerTuning {
    max_body_bytes: Option<usize>,
    default_timeout: Option<Duration>,
}

impl ServerTuning {
    const DEFAULT: Self = Self {
        max_body_bytes: None,
        default_timeout: None,
    };
}

const BODY_CAP_64: ServerTuning = ServerTuning {
    max_body_bytes: Some(64),
    default_timeout: None,
};
const TRICKLE_BUDGET: ServerTuning = ServerTuning {
    max_body_bytes: None,
    default_timeout: Some(Duration::from_millis(100)),
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Exchange {
    Whole,
    Trickle { stall: Duration },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawCase {
    id: &'static str,
    request: RequestShape,
    authority: &'static [SpecParagraph],
    expected: ExpectedResponse,
    body: Option<&'static [u8]>,
    content_length: Option<usize>,
    chunked: bool,
    tuning: ServerTuning,
    exchange: Exchange,
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
            body: None,
            content_length: None,
            chunked: false,
            tuning: ServerTuning::DEFAULT,
            exchange: Exchange::Whole,
        }
    }

    const fn with_body(mut self, body: &'static [u8]) -> Self {
        self.body = Some(body);
        self
    }

    const fn with_chunked_framing(mut self) -> Self {
        self.chunked = true;
        self
    }

    const fn with_content_length(mut self, content_length: usize) -> Self {
        self.content_length = Some(content_length);
        self
    }

    const fn with_tuning(mut self, tuning: ServerTuning) -> Self {
        self.tuning = tuning;
        self
    }

    const fn with_trickle(mut self, stall: Duration) -> Self {
        self.exchange = Exchange::Trickle { stall };
        self
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
const PAYLOAD_TOO_LARGE_STATUS: &[u8] = b"HTTP/1.1 413 Payload Too Large";
const GATEWAY_TIMEOUT_STATUS: &[u8] = b"HTTP/1.1 504 Gateway Timeout";

const UB: &[u8] = UNKNOWN_BOX_BODY;
const UCB: &[u8] = UNKNOWN_CAPABILITY_BODY;
const IRB: &[u8] = INVALID_REQUEST_BODY;
const MNB: &[u8] = METHOD_NOT_ALLOWED_BODY;
const UMTB: &[u8] = UNSUPPORTED_MEDIA_TYPE_BODY;
const PTLB: &[u8] = PAYLOAD_TOO_LARGE_BODY;
const DEB: &[u8] = DEADLINE_EXCEEDED_BODY;
const SU: ExpectedResponse = e(OK_STATUS, SUCCESS_BODY, JSON, None, 1);
const BX: ExpectedResponse = e(NOT_FOUND_STATUS, UB, JSON, None, 0);
const CAP: ExpectedResponse = e(NOT_FOUND_STATUS, UCB, JSON, None, 0);
const BAD: ExpectedResponse = e(BAD_REQUEST_STATUS, IRB, JSON, None, 0);
const NA: ExpectedResponse = e(METHOD_NOT_ALLOWED_STATUS, MNB, JSON, Some(POST), 0);
const UMT: ExpectedResponse = e(UNSUPPORTED_MEDIA_TYPE_STATUS, UMTB, JSON, None, 0);
const PTL: ExpectedResponse = e(PAYLOAD_TOO_LARGE_STATUS, PTLB, JSON, None, 0);
const DE: ExpectedResponse = e(GATEWAY_TIMEOUT_STATUS, DEB, JSON, None, 0);

use SpecParagraph::{
    RuntimeInvocationStatusTable as STATUS, RuntimeStableWireCodes as CODES, S3D2Codec as D2,
    S3D3CanonicalResponseEncoding as D3, S3D4RoutingAndIdentifierCanonicality as D4,
    S3D5StableWireErrorCodes as D5, S3D6HeaderGrammars as D6, S3D7RequestProcessingPipeline as D7,
};
const SUCCESS_AUTHORITY: &[SpecParagraph] = &[D3, D4];
const ROUTING_AUTHORITY: &[SpecParagraph] = &[D3, D4, D5, STATUS, CODES];
const METHOD_AUTHORITY: &[SpecParagraph] = &[D3, D5, D6, STATUS, CODES];
const ROUTE_PIPELINE_AUTHORITY: &[SpecParagraph] = &[D3, D4, D5, D7, STATUS, CODES];
const HEAD_ADMISSION_AUTHORITY: &[SpecParagraph] = &[D3, D5, D6, D7, STATUS, CODES];
const HEAD_ADMISSION_ACCEPTED_AUTHORITY: &[SpecParagraph] = &[D3, D6];
const SYNTAX_AUTHORITY: &[SpecParagraph] = &[D3, D2, D5, D7, STATUS, CODES];
const CAP_MEDIA_AUTHORITY: &[SpecParagraph] = &[D3, D2, D5, D6, D7, STATUS, CODES];
const DEADLINE_AUTHORITY: &[SpecParagraph] = &[D3, D5, D7, STATUS, CODES];
const SA: &[SpecParagraph] = SUCCESS_AUTHORITY;
const RA: &[SpecParagraph] = ROUTING_AUTHORITY;
const MA: &[SpecParagraph] = METHOD_AUTHORITY;
const PA: &[SpecParagraph] = ROUTE_PIPELINE_AUTHORITY;
const HA: &[SpecParagraph] = HEAD_ADMISSION_AUTHORITY;
const HAA: &[SpecParagraph] = HEAD_ADMISSION_ACCEPTED_AUTHORITY;
const SX: &[SpecParagraph] = SYNTAX_AUTHORITY;
const CM: &[SpecParagraph] = CAP_MEDIA_AUTHORITY;
const DA: &[SpecParagraph] = DEADLINE_AUTHORITY;

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
const APPLICATION_XML_MEDIA_TYPE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    // application/* with a non-json subtype: type is accepted, so only the
    // `subty != JSON` arm can refuse it (unlike text/plain, which fails both).
    content_type: Some("application/xml"),
    extra: &[],
};
const TEXT_JSON_MEDIA_TYPE_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    // text/json: subtype is accepted, so only the `ty != APPLICATION` arm can
    // refuse it (unlike text/plain, which fails both).
    content_type: Some("text/json"),
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
const BAD_MEDIA_EXPIRED_REQUEST: RequestShape = RequestShape {
    method: "POST",
    path: "/rpc/hello/greet",
    // Compound D7 row: rejected media together with a defective timeout header.
    // Media (stage 3 → 415) must win over header-grammar (stage 4 → 400).
    content_type: Some("text/plain"),
    extra: &["Boxology-Timeout-Ms: soon"],
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
    RawCase::new("query-string", QUERY_REQUEST, PA, BAD),
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
        "application-xml-media-type",
        APPLICATION_XML_MEDIA_TYPE_REQUEST,
        HA,
        UMT,
    ),
    RawCase::new(
        "text-json-media-type",
        TEXT_JSON_MEDIA_TYPE_REQUEST,
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
        HAA,
        SU,
    ),
    RawCase::new(
        "charset-utf8-case-accepted",
        CHARSET_UTF8_CASE_ACCEPTED_REQUEST,
        HAA,
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
    RawCase::new("bad-media-expired", BAD_MEDIA_EXPIRED_REQUEST, HA, UMT),
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
        HAA,
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
    // Traceability-only classification row: `Parser::value` no-value →
    // `MalformedJson` → 400. Semantic backstop would also reject a fabricated
    // non-String tree; M6 (catch-all → Internal) goes red, but no unique
    // deletion mutant is attributable to this row.
    RawCase::new("empty-body", EXACT_REQUEST, HA, BAD).with_body(EMPTY_BODY),
    // Isolates exhaustion `then_some` (syntax.rs). M1 → 200. No other check.
    RawCase::new("trailing-bytes", EXACT_REQUEST, HA, BAD).with_body(TRAILING_BYTES_BODY),
    // Isolates BOM rejection after `whitespace()`. M2 → 200. No other check;
    // authority is D2's explicit rejection of a leading U+FEFF.
    RawCase::new("bom-prefixed-body", EXACT_REQUEST, SX, BAD).with_body(BOM_PREFIXED_BODY),
    // Isolates whole-input `from_utf8`. M3 → 200. No other check.
    RawCase::new("invalid-utf8-body", EXACT_REQUEST, SX, BAD).with_body(INVALID_UTF8_BODY),
    // Classification row: depth guard → `DepthLimitExceeded` → InvalidRequest.
    // Guard-removal stays green (nested list vs String → 400), so deletion is
    // invisible here and isolation is owed to a fixture whose capability input
    // accepts lists. M5 (classify as PayloadTooLarge) → 413. Pins taxonomy +
    // zero dispatch, not the guard.
    RawCase::new("depth-bomb", EXACT_REQUEST, SX, BAD).with_body(DEPTH_BOMB_BODY),
    // `{` → MalformedJson → 400; pipeline classification for String input.
    RawCase::new("malformed-json", EXACT_REQUEST, SX, BAD).with_body(MALFORMED_JSON_BODY),
    // Object vs String → 400; syntax keeps duplicate keys (semantic.rs owns key shape).
    RawCase::new("duplicate-key-object", EXACT_REQUEST, SX, BAD)
        .with_body(DUPLICATE_KEY_OBJECT_BODY),
    // `007` is syntax-malformed leading zeros, not semantic NonCanonicalInteger.
    RawCase::new("noncanonical-integer", EXACT_REQUEST, SX, BAD)
        .with_body(NONCANONICAL_INTEGER_BODY),
    // Byte cap defended by three layers (size_hint, accumulation, parse). M4
    // (drop decode PayloadTooLarge arm) stays green — collection rejects first;
    // that arm is only reachable via direct `decode_request_body` unit tests.
    // M8 compound (all three layers off) → 200. Wiring: drop tuning → 200.
    RawCase::new("oversized-content-length", EXACT_REQUEST, SX, PTL)
        .with_body(OVERSIZED_STRING_BODY)
        .with_tuning(BODY_CAP_64),
    // Isolates accumulation (chunked size_hint.lower() is 0). Parse cap also
    // rejects. M8-chunked (accumulation + parse off) → 200; CL-only caps fail here.
    RawCase::new("oversized-chunked", EXACT_REQUEST, SX, PTL)
        .with_body(OVERSIZED_CHUNKED_BODY)
        .with_chunked_framing()
        .with_tuning(BODY_CAP_64),
    // Cap-before-grammar intent. M7 (reorder parse cap after grammar) stays
    // green on the live path — collection size_hint/accumulation reject with
    // 413 before `parse` runs. Proves wire 413 for oversized+malformed, not
    // the parse-site ordering alone (that ordering stays with unit tests).
    RawCase::new("oversized-plus-malformed", EXACT_REQUEST, SX, PTL)
        .with_body(OVERSIZED_MALFORMED_BODY)
        .with_tuning(BODY_CAP_64),
    // Precedence: head admission (415) before body cap (413). M10 (collect
    // before admit) → 413. Classification mutants stay green.
    RawCase::new(
        "oversized-plus-bad-media",
        APPLICATION_XML_MEDIA_TYPE_REQUEST,
        CM,
        UMT,
    )
    .with_body(OVERSIZED_STRING_BODY)
    .with_tuning(BODY_CAP_64),
    // Pre-dispatch deadline. Four stages cover the stall: collect timeout,
    // pre-dispatch check, `await_dispatch`'s timeout arm, and `invoke_if_live`'s
    // pre-invoke check. No single-arm mutant is observable here. Wiring: drop
    // trickle → 200. Compound needed to isolate the collect arm alone.
    RawCase::new("trickled-body-vs-budget", EXACT_REQUEST, DA, DE)
        .with_tuning(TRICKLE_BUDGET)
        .with_trickle(Duration::from_millis(250)),
    // Declared length is far above the cap, but the client sends only this
    // head and never a body. The size-hint pre-check gives 413 immediately;
    // without it, collection waits for the absent body and the short budget
    // gives 504.
    RawCase::new("oversized-content-length-head-only", EXACT_REQUEST, SX, PTL)
        .with_body(EMPTY_BODY)
        .with_content_length(1_000_000)
        .with_tuning(ServerTuning {
            max_body_bytes: Some(64),
            default_timeout: Some(Duration::from_millis(100)),
        })
        .with_trickle(Duration::from_millis(250)),
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
    o("query-string", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet?probe=1 HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("get-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"GET /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 405 Method Not Allowed", br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#, b"application/json", Some(b"POST"), 0)),
    o("options-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"OPTIONS /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 405 Method Not Allowed", br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#, b"application/json", Some(b"POST"), 0)),
    o("unknown-route-wrong-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D4RoutingAndIdentifierCanonicality, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"GET /rpc/ghost/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 404 Not Found", br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#, b"application/json", None, 0)),
    o("put-method", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"PUT /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 405 Method Not Allowed", br#"{"error":{"kind":"call","code":"method_not_allowed","message":"method not allowed"}}"#, b"application/json", Some(b"POST"), 0)),
    o("missing-content-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("application-xml-media-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/xml\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("text-json-media-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: text/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("json-suffix-media-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json+foo\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("wrong-charset", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json; charset=latin-1\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("charset-utf8-accepted", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D6HeaderGrammars], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json; charset=utf-8\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 200 OK", br#"{"result":{"value":"Hello, Ada!"}}"#, b"application/json", None, 1)),
    o("charset-utf8-case-accepted", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D6HeaderGrammars], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json; charset=UTF-8\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 200 OK", br#"{"result":{"value":"Hello, Ada!"}}"#, b"application/json", None, 1)),
    o("trailing-semicolon-media-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json;\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("duplicate-content-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("comma-joined-content-type", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json, application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("content-encoding-identity", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nContent-Encoding: identity\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("content-encoding-gzip", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("bad-media-expired", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: text/plain\r\nBoxology-Timeout-Ms: soon\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("timeout-non-digit", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: soon\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-leading-zero", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 01\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-eleven-digits", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 10000000000\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-embedded-space", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 60 000\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-duplicate", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 1000\r\nBoxology-Timeout-Ms: 2000\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("timeout-max-valid-accepted", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D6HeaderGrammars], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nBoxology-Timeout-Ms: 9999999999\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 200 OK", br#"{"result":{"value":"Hello, Ada!"}}"#, b"application/json", None, 1)),
    o("idempotency-duplicate", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nIdempotency-Key: alpha\r\nIdempotency-Key: beta\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("idempotency-empty", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nIdempotency-Key:\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("idempotency-obs-text", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nIdempotency-Key: caf\xc3\xa9\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("empty-body", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 0\r\n\r\n", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("trailing-bytes", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 7\r\n\r\n\"Ada\" 0", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("bom-prefixed-body", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 8\r\n\r\n\xef\xbb\xbf\"Ada\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("invalid-utf8-body", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 3\r\n\r\n\"\xff\"", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("depth-bomb", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 265\r\n\r\n[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[\"Ada\"]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("malformed-json", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 1\r\n\r\n{", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("duplicate-key-object", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 13\r\n\r\n{\"a\":1,\"a\":1}", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("noncanonical-integer", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 3\r\n\r\n007", e(b"HTTP/1.1 400 Bad Request", br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#, b"application/json", None, 0)),
    o("oversized-content-length", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 67\r\n\r\n\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"", e(b"HTTP/1.1 413 Payload Too Large", br#"{"error":{"kind":"call","code":"payload_too_large","message":"payload too large"}}"#, b"application/json", None, 0)),
    o("oversized-chunked", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n43\r\n\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"\r\n0\r\n\r\n", e(b"HTTP/1.1 413 Payload Too Large", br#"{"error":{"kind":"call","code":"payload_too_large","message":"payload too large"}}"#, b"application/json", None, 0)),
    o("oversized-plus-malformed", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 67\r\n\r\n!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!", e(b"HTTP/1.1 413 Payload Too Large", br#"{"error":{"kind":"call","code":"payload_too_large","message":"payload too large"}}"#, b"application/json", None, 0)),
    o("oversized-plus-bad-media", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D6HeaderGrammars, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/xml\r\nConnection: close\r\nContent-Length: 67\r\n\r\n\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"", e(b"HTTP/1.1 415 Unsupported Media Type", br#"{"error":{"kind":"call","code":"unsupported_media_type","message":"unsupported media type"}}"#, b"application/json", None, 0)),
    o("trickled-body-vs-budget", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"", e(b"HTTP/1.1 504 Gateway Timeout", br#"{"error":{"kind":"call","code":"deadline_exceeded","message":"deadline exceeded"}}"#, b"application/json", None, 0)),
    o("oversized-content-length-head-only", &[SpecParagraph::S3D3CanonicalResponseEncoding, SpecParagraph::S3D2Codec, SpecParagraph::S3D5StableWireErrorCodes, SpecParagraph::S3D7RequestProcessingPipeline, SpecParagraph::RuntimeInvocationStatusTable, SpecParagraph::RuntimeStableWireCodes], b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 1000000\r\n\r\n", e(b"HTTP/1.1 413 Payload Too Large", br#"{"error":{"kind":"call","code":"payload_too_large","message":"payload too large"}}"#, b"application/json", None, 0)),
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

#[test]
fn evidence_inventory_matches_raw_cases() {
    let ids: Vec<&str> = RAW_CASES.iter().map(|case| case.id).collect();
    boxology_http_conformance::assert_ordered_case_ids(
        &ids,
        boxology_http_conformance::RAW_HELLO_CASE_IDS,
        "raw_hello",
    );
}

#[test]
fn named_evidence_resolves_through_inventory() {
    boxology_http_conformance::assert_named_evidence_resolution(
        "raw_hello",
        &[
            (
                "raw_hello_cases_are_canonical",
                raw_hello_cases_are_canonical as *const (),
            ),
            (
                "default_request_body_limit_boundary_is_one_mib",
                default_request_body_limit_boundary_is_one_mib as *const (),
            ),
            (
                "malformed_request_line_is_bare_http_400",
                malformed_request_line_is_bare_http_400 as *const (),
            ),
            (
                "request_head_over_default_16_kib_cap_is_bare_http_431",
                request_head_over_default_16_kib_cap_is_bare_http_431 as *const (),
            ),
            (
                "overlong_request_target_is_bare_http_414",
                overlong_request_target_is_bare_http_414 as *const (),
            ),
        ],
    );
}

#[tokio::test]
async fn raw_hello_cases_are_canonical() {
    traceability_gate(&RAW_CASES, &ORACLE);
    for case in RAW_CASES {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let mut config =
            HttpServerConfig::new("127.0.0.1:0".parse().expect("loopback address is valid"));
        if let Some(cap) = case.tuning.max_body_bytes {
            // Must match boxology-http's private DEFAULT_DEPTH_LIMIT; capped
            // rows rely on this fixture coupling.
            config = config.with_request_limits(cap, 128);
        }
        if let Some(deadline) = case.tuning.default_timeout {
            config = config.with_default_timeout(deadline);
        }
        let running = RunningHello::start_with_config(
            CountingHello {
                dispatches: Arc::clone(&dispatches),
            },
            config,
        );
        let address = running.local_addr();
        let request = render_request(case);

        running
            .assert_then_shutdown(async move {
                let response =
                    raw_exchange(address, &request, case.exchange, Duration::from_secs(5)).await;
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

/// Pins the composition-default request-body limit from `03-runtime.md` (1 MiB).
///
/// Not folded into `RAW_CASES` / `ORACLE`: those require independently pinned
/// exact raw request byte literals, and embedding a 1 MiB body would distort
/// the table and blow the review-line budget. This test uses the same raw
/// exchange helpers against a server with **no** `with_request_limits` tuning,
/// so it is sensitive to `DEFAULT_MAX_BODY_BYTES` alone (existing oversized
/// rows all configure `BODY_CAP_64` and never exercise the default).
#[tokio::test]
async fn default_request_body_limit_boundary_is_one_mib() {
    const DEFAULT_LIMIT: usize = 1024 * 1024;

    // Inclusive boundary: a body of exactly the default is accepted.
    {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let config =
            HttpServerConfig::new("127.0.0.1:0".parse().expect("loopback address is valid"));
        let running = RunningHello::start_with_config(
            CountingHello {
                dispatches: Arc::clone(&dispatches),
            },
            config,
        );
        let address = running.local_addr();
        let body = json_string_body(DEFAULT_LIMIT);
        let request = content_length_greet_request(&body);
        let name = std::str::from_utf8(&body[1..body.len() - 1]).expect("ASCII JSON string");
        let expected_body = format!(r#"{{"result":{{"value":"Hello, {name}!"}}}}"#);

        running
            .assert_then_shutdown_with(
                async move {
                    let response = raw_exchange(
                        address,
                        &request,
                        Exchange::Whole,
                        DEFAULT_BODY_LIMIT_BOUNDARY_TIMEOUT,
                    )
                    .await;
                    let (head, response_body) = split_response(&response);
                    let line_end = head
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .expect("valid HTTP response has a status line");
                    let status_line = head[..line_end]
                        .strip_suffix(b"\r")
                        .unwrap_or(&head[..line_end]);
                    assert_eq!(status_line, OK_STATUS);
                    assert_eq!(response_body, expected_body.as_bytes());
                    let headers = parse_headers(&head[line_end + 1..]);
                    assert_header(&headers, b"content-type", Some(JSON));
                    assert_eq!(
                        dispatches.load(Ordering::SeqCst),
                        1,
                        "at-limit dispatch count"
                    );
                },
                DEFAULT_BODY_LIMIT_BOUNDARY_TIMEOUT,
                |result| result,
            )
            .await;
    }

    // One byte over the default is rejected with the canonical 413 body.
    // The server rejects on the size-hint check before reading the body, emits
    // 413, and closes with unread data still buffered — which produces RST while
    // the client is still writing. Read concurrently with the write and ignore
    // write errors so a reset or stalled write cannot prevent draining the 413.
    // A raised default (e.g. 8 MiB) accepts this body and fails the oracle.
    {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let config =
            HttpServerConfig::new("127.0.0.1:0".parse().expect("loopback address is valid"));
        let running = RunningHello::start_with_config(
            CountingHello {
                dispatches: Arc::clone(&dispatches),
            },
            config,
        );
        let address = running.local_addr();
        let body = json_string_body(DEFAULT_LIMIT + 1);
        let request = content_length_greet_request(&body);

        running
            .assert_then_shutdown_with(
                async move {
                    let response = raw_exchange_allow_early_close(
                        address,
                        &request,
                        DEFAULT_BODY_LIMIT_BOUNDARY_TIMEOUT,
                    )
                    .await;
                    assert_response(&response, PTL);
                    assert_eq!(
                        dispatches.load(Ordering::SeqCst),
                        0,
                        "over-default dispatch count"
                    );
                },
                DEFAULT_BODY_LIMIT_BOUNDARY_TIMEOUT,
                |result| result,
            )
            .await;
    }
}

/// Bare framing: malformed request line → HTTP 400, empty body, no JSON envelope.
/// Status/body only; Date/Connection/Content-Length bytes are not policy.
#[tokio::test]
async fn malformed_request_line_is_bare_http_400() {
    let (running, dispatches) = counting_hello();
    let address = running.local_addr();
    let malformed = b"POST /rpc/hello/greet\r\nHost: boxology\r\nConnection: close\r\n\r\n";
    let well_formed = b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 5\r\n\r\n\"Ada\"";
    running
        .assert_then_shutdown(async move {
            assert_bare_status_response(
                &raw_exchange(address, malformed, Exchange::Whole, Duration::from_secs(5)).await,
                400,
            );
            // Negative: well-formed request still yields the JSON call envelope.
            assert_response(
                &raw_exchange(
                    address,
                    well_formed,
                    Exchange::Whole,
                    Duration::from_secs(5),
                )
                .await,
                SU,
            );
            assert_eq!(dispatches.load(Ordering::SeqCst), 1);
        })
        .await;
}

/// Bare framing: head over default 16 KiB cap → HTTP 431, empty body, no JSON envelope.
/// Empty body + no Content-Type isolate head admission; status/body only are policy.
#[tokio::test]
async fn request_head_over_default_16_kib_cap_is_bare_http_431() {
    const CAP: usize = 16 * 1024;
    let (running, dispatches) = counting_hello();
    let address = running.local_addr();
    let at = empty_body_padded_head(CAP);
    let over = empty_body_padded_head(CAP + 1);
    running
        .assert_then_shutdown(async move {
            // Negative: at-cap head is admitted; missing Content-Type → JSON 415, not bare 431.
            assert_response(
                &raw_exchange(address, &at, Exchange::Whole, Duration::from_secs(5)).await,
                UMT,
            );
            assert_bare_status_response(
                &raw_exchange(address, &over, Exchange::Whole, Duration::from_secs(5)).await,
                431,
            );
            assert_eq!(dispatches.load(Ordering::SeqCst), 0);
        })
        .await;
}

/// Bare framing: over-long request-target → HTTP 414, empty body, no JSON envelope.
/// Hyper independently bounds request-target length before service dispatch.
/// Parse buffer is raised only so the target can complete-parse; without that,
/// the default 16 KiB head cap returns bare 431 first.
#[tokio::test]
async fn overlong_request_target_is_bare_http_414() {
    const OVERLONG_URI_LEN: usize = 65_535;
    let dispatches = Arc::new(AtomicUsize::new(0));
    let config = HttpServerConfig::new("127.0.0.1:0".parse().expect("loopback address is valid"))
        .with_max_request_head_bytes(128 * 1024);
    let running = RunningHello::start_with_config(
        CountingHello {
            dispatches: Arc::clone(&dispatches),
        },
        config,
    );
    let address = running.local_addr();
    let overlong = long_request_target_head(OVERLONG_URI_LEN);
    running
        .assert_then_shutdown(async move {
            assert_bare_status_response(
                &raw_exchange(address, &overlong, Exchange::Whole, Duration::from_secs(5)).await,
                414,
            );
            assert_eq!(dispatches.load(Ordering::SeqCst), 0);
        })
        .await;
}

fn json_string_body(total_len: usize) -> Vec<u8> {
    assert!(total_len >= 2, "JSON string body needs surrounding quotes");
    let mut body = Vec::with_capacity(total_len);
    body.push(b'"');
    body.resize(total_len - 1, b'A');
    body.push(b'"');
    body
}

fn content_length_greet_request(body: &[u8]) -> Vec<u8> {
    let mut rendered = format!(
        "POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    rendered.extend_from_slice(body);
    rendered
}

async fn raw_exchange_allow_early_close(
    address: std::net::SocketAddr,
    request: &[u8],
    limit: Duration,
) -> Vec<u8> {
    timeout(limit, async {
        let stream = TcpStream::connect(address).await.expect("connect");
        let (mut reader, mut writer) = stream.into_split();
        let request = request.to_vec();
        // Write outcome is not under test: size-hint rejection closes with unread
        // body data and the kernel may RST mid-write (ECONNRESET) or stall the
        // send window. Concurrent read drains the 413 before RST can discard it.
        // Return the write half so a finished write does not FIN until after the
        // read — an early half-close cancels an accepted request with no response.
        let write_task = tokio::spawn(async move {
            let _ = writer.write_all(&request).await;
            writer
        });
        let mut response = Vec::new();
        if let Err(error) = reader.read_to_end(&mut response).await {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        }
        write_task.abort();
        let _ = write_task.await;
        response
    })
    .await
    .expect("timeout")
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
    extra_header_drift[24].request.extra = &["Content-Encoding: br"];
    assert_traceability_rejects(&extra_header_drift, &ORACLE, "extra-header-line drift");

    let mut umt_status_drift = RAW_CASES.to_vec();
    umt_status_drift[14].expected.status_line = b"HTTP/1.1 415 Mismatched Reason";
    assert_traceability_rejects(&umt_status_drift, &ORACLE, "415 status-line drift");

    let mut umt_body_drift = RAW_CASES.to_vec();
    umt_body_drift[14].expected.body = INVALID_REQUEST_BODY;
    assert_traceability_rejects(&umt_body_drift, &ORACLE, "UMT body drift");

    let mut accepted_dispatch_drift = RAW_CASES.to_vec();
    accepted_dispatch_drift[32].expected.dispatches = 0;
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

    let mut body_override_drift = RAW_CASES.to_vec();
    body_override_drift[37].body = Some(b"\"Ada\"");
    assert_traceability_rejects(&body_override_drift, &ORACLE, "body-override drift");

    let mut framing_drift = RAW_CASES.to_vec();
    framing_drift[45].chunked = false;
    assert_traceability_rejects(&framing_drift, &ORACLE, "framing drift");

    let mut payload_status_drift = RAW_CASES.to_vec();
    payload_status_drift[44].expected.status_line = BAD_REQUEST_STATUS;
    assert_traceability_rejects(&payload_status_drift, &ORACLE, "413 status-line drift");

    for (idx, label) in [
        (41, "malformed-json body drift"),
        (42, "duplicate-key-object body drift"),
        (43, "noncanonical-integer body drift"),
    ] {
        let mut body_drift = RAW_CASES.to_vec();
        body_drift[idx].body = Some(b"\"Ada\"");
        assert_traceability_rejects(&body_drift, &ORACLE, label);
    }
    let mut malformed_json_dispatch_drift = RAW_CASES.to_vec();
    malformed_json_dispatch_drift[41].expected.dispatches = 1;
    assert_traceability_rejects(
        &malformed_json_dispatch_drift,
        &ORACLE,
        "malformed-json dispatch drift",
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
        if render_request(*case).as_slice() != golden.2 {
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

fn render_request(case: RawCase) -> Vec<u8> {
    let request = case.request;
    let body = case.body.unwrap_or(REQUEST_BODY);
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
    rendered.extend_from_slice(b"Connection: close\r\n");
    if case.chunked {
        rendered.extend_from_slice(b"Transfer-Encoding: chunked\r\n\r\n");
    } else {
        let content_length = case.content_length.unwrap_or(body.len());
        rendered.extend_from_slice(format!("Content-Length: {content_length}\r\n\r\n").as_bytes());
    }
    rendered.extend_from_slice(body);
    rendered
}

async fn raw_exchange(
    address: std::net::SocketAddr,
    request: &[u8],
    exchange: Exchange,
    limit: Duration,
) -> Vec<u8> {
    timeout(limit, async {
        let mut stream = TcpStream::connect(address).await.expect("connect");
        match exchange {
            Exchange::Whole => {
                stream.write_all(request).await.expect("write");
            }
            Exchange::Trickle { stall } => {
                let split = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .expect("request has a header/body boundary")
                    + 4;
                stream
                    .write_all(&request[..split])
                    .await
                    .expect("write head");
                tokio::time::sleep(stall).await;
                let _ = stream.write_all(&request[split..]).await;
            }
        }
        let mut response = Vec::new();
        if let Err(error) = stream.read_to_end(&mut response).await {
            assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        }
        response
    })
    .await
    .expect("timeout")
}

fn counting_hello() -> (RunningHello, Arc<AtomicUsize>) {
    let dispatches = Arc::new(AtomicUsize::new(0));
    let running = RunningHello::start_with_config(
        CountingHello {
            dispatches: Arc::clone(&dispatches),
        },
        HttpServerConfig::new("127.0.0.1:0".parse().expect("loopback address is valid")),
    );
    (running, dispatches)
}

fn empty_body_padded_head(length: usize) -> Vec<u8> {
    // No Content-Type + CL:0: over-cap → bare 431; at-cap → JSON 415 missing media.
    let prefix = b"POST /rpc/hello/greet HTTP/1.1\r\nHost: boxology\r\nConnection: close\r\nContent-Length: 0\r\nX-Padding: ";
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

fn long_request_target_head(uri_len: usize) -> Vec<u8> {
    // Request-target is exactly `uri_len` bytes. Hyper independently rejects
    // over-long targets with bare 414 before dispatch.
    let prefix = b"POST ";
    let suffix = b" HTTP/1.1\r\nHost: boxology\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
    assert!(uri_len >= 1);
    let mut head = Vec::with_capacity(prefix.len() + uri_len + suffix.len());
    head.extend_from_slice(prefix);
    head.push(b'/');
    head.extend(std::iter::repeat_n(b'x', uri_len - 1));
    head.extend_from_slice(suffix);
    head
}

/// Status code from a raw HTTP response; used by bare-status assertions.
fn status_code(raw: &[u8]) -> u16 {
    let (head, _) = split_response(raw);
    let line_end = head.iter().position(|b| *b == b'\n').expect("status line");
    let status_line = head[..line_end]
        .strip_suffix(b"\r")
        .unwrap_or(&head[..line_end]);
    std::str::from_utf8(status_line)
        .expect("utf8")
        .split(' ')
        .nth(1)
        .and_then(|t| t.parse::<u16>().ok())
        .expect("status code")
}

/// Bare pre-service framing: exact status, empty body, no Content-Type, no error envelope.
fn assert_bare_status_response(raw: &[u8], status: u16) {
    let (head, body) = split_response(raw);
    assert_eq!(status_code(raw), status);
    assert!(body.is_empty(), "bare body must be empty");
    let line_end = head.iter().position(|b| *b == b'\n').expect("status line");
    assert_header(&parse_headers(&head[line_end + 1..]), b"content-type", None);
    assert!(
        !raw.windows(8).any(|w| w == b"{\"error\""),
        "bare response must not carry a JSON call envelope"
    );
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
