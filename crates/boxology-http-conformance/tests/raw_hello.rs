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
enum Authority {
    S3D4RoutingCanonicality,
    S3D5StableWireCodes,
    S3D6MethodTable,
    S3D7PipelineOrder,
    RuntimeInvocationStatusTable,
    RuntimeStableCodeParagraph,
}

impl Authority {
    const ALL: [Self; 6] = [
        Self::S3D4RoutingCanonicality,
        Self::S3D5StableWireCodes,
        Self::S3D6MethodTable,
        Self::S3D7PipelineOrder,
        Self::RuntimeInvocationStatusTable,
        Self::RuntimeStableCodeParagraph,
    ];
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Rule {
    ExactSuccess,
    UnknownBox,
    UnknownCapability,
    PercentEncodedBox,
    PercentEncodedCapability,
    UppercasePrefix,
    UppercaseBox,
    UppercaseCapability,
    TrailingSlash,
    Query,
    GetMethod,
    OptionsMethod,
    UnknownRouteWrongMethod,
    QueryWrongMethod,
}

impl Rule {
    const ALL: [Self; 14] = [
        Self::ExactSuccess,
        Self::UnknownBox,
        Self::UnknownCapability,
        Self::PercentEncodedBox,
        Self::PercentEncodedCapability,
        Self::UppercasePrefix,
        Self::UppercaseBox,
        Self::UppercaseCapability,
        Self::TrailingSlash,
        Self::Query,
        Self::GetMethod,
        Self::OptionsMethod,
        Self::UnknownRouteWrongMethod,
        Self::QueryWrongMethod,
    ];

    fn authorities(self) -> &'static [Authority] {
        use Authority::{
            RuntimeInvocationStatusTable as RuntimeStatus,
            RuntimeStableCodeParagraph as RuntimeCode, S3D4RoutingCanonicality as D4,
            S3D5StableWireCodes as D5, S3D6MethodTable as D6, S3D7PipelineOrder as D7,
        };
        match self {
            Self::ExactSuccess => &[D4],
            Self::UnknownBox
            | Self::UnknownCapability
            | Self::PercentEncodedBox
            | Self::PercentEncodedCapability
            | Self::UppercasePrefix
            | Self::UppercaseBox
            | Self::UppercaseCapability
            | Self::TrailingSlash
            | Self::Query => &[D4, D5, RuntimeStatus, RuntimeCode],
            Self::GetMethod | Self::OptionsMethod => &[D5, D6, RuntimeStatus, RuntimeCode],
            Self::UnknownRouteWrongMethod => &[D4, D5, D7, RuntimeStatus, RuntimeCode],
            Self::QueryWrongMethod => &[D4, D5, D6, D7, RuntimeStatus, RuntimeCode],
        }
    }

    fn request(self) -> RequestShape {
        match self {
            Self::ExactSuccess => RequestShape::new("POST", "/rpc/hello/greet"),
            Self::UnknownBox => RequestShape::new("POST", "/rpc/ghost/greet"),
            Self::UnknownCapability => RequestShape::new("POST", "/rpc/hello/ghost"),
            Self::PercentEncodedBox => RequestShape::new("POST", "/rpc/hell%6F/greet"),
            Self::PercentEncodedCapability => RequestShape::new("POST", "/rpc/hello/gree%74"),
            Self::UppercasePrefix => RequestShape::new("POST", "/RPC/hello/greet"),
            Self::UppercaseBox => RequestShape::new("POST", "/rpc/Hello/greet"),
            Self::UppercaseCapability => RequestShape::new("POST", "/rpc/hello/Greet"),
            Self::TrailingSlash => RequestShape::new("POST", "/rpc/hello/greet/"),
            Self::Query => RequestShape::new("POST", "/rpc/hello/greet?probe=1"),
            Self::GetMethod => RequestShape::new("GET", "/rpc/hello/greet"),
            Self::OptionsMethod => RequestShape::new("OPTIONS", "/rpc/hello/greet"),
            Self::UnknownRouteWrongMethod => RequestShape::new("GET", "/rpc/ghost/greet"),
            Self::QueryWrongMethod => RequestShape::new("GET", "/rpc/hello/greet?probe=1"),
        }
    }

    fn expected(self) -> ExpectedResponse {
        match self {
            Self::ExactSuccess => SUCCESS,
            Self::UnknownBox
            | Self::PercentEncodedBox
            | Self::UppercasePrefix
            | Self::UppercaseBox
            | Self::UnknownRouteWrongMethod => UNKNOWN_BOX,
            Self::UnknownCapability
            | Self::PercentEncodedCapability
            | Self::UppercaseCapability
            | Self::TrailingSlash => UNKNOWN_CAPABILITY,
            Self::Query | Self::QueryWrongMethod => INVALID_REQUEST,
            Self::GetMethod | Self::OptionsMethod => METHOD_NOT_ALLOWED,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RequestShape {
    method: &'static str,
    target: &'static str,
}

impl RequestShape {
    const fn new(method: &'static str, target: &'static str) -> Self {
        Self { method, target }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExpectedResponse {
    status_line: &'static [u8],
    body: &'static [u8],
    allow: Option<&'static [u8]>,
    dispatches: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawCase {
    rule: Rule,
    request: RequestShape,
}

const SUCCESS: ExpectedResponse = ExpectedResponse {
    status_line: b"HTTP/1.1 200 OK",
    body: SUCCESS_BODY,
    allow: None,
    dispatches: 1,
};
const UNKNOWN_BOX: ExpectedResponse = ExpectedResponse {
    status_line: b"HTTP/1.1 404 Not Found",
    body: UNKNOWN_BOX_BODY,
    allow: None,
    dispatches: 0,
};
const UNKNOWN_CAPABILITY: ExpectedResponse = ExpectedResponse {
    status_line: b"HTTP/1.1 404 Not Found",
    body: UNKNOWN_CAPABILITY_BODY,
    allow: None,
    dispatches: 0,
};
const INVALID_REQUEST: ExpectedResponse = ExpectedResponse {
    status_line: b"HTTP/1.1 400 Bad Request",
    body: INVALID_REQUEST_BODY,
    allow: None,
    dispatches: 0,
};
const METHOD_NOT_ALLOWED: ExpectedResponse = ExpectedResponse {
    status_line: b"HTTP/1.1 405 Method Not Allowed",
    body: METHOD_NOT_ALLOWED_BODY,
    allow: Some(b"POST"),
    dispatches: 0,
};

const RAW_CASES: [RawCase; 14] = [
    RawCase {
        rule: Rule::ExactSuccess,
        request: RequestShape::new("POST", "/rpc/hello/greet"),
    },
    RawCase {
        rule: Rule::UnknownBox,
        request: RequestShape::new("POST", "/rpc/ghost/greet"),
    },
    RawCase {
        rule: Rule::UnknownCapability,
        request: RequestShape::new("POST", "/rpc/hello/ghost"),
    },
    RawCase {
        rule: Rule::PercentEncodedBox,
        request: RequestShape::new("POST", "/rpc/hell%6F/greet"),
    },
    RawCase {
        rule: Rule::PercentEncodedCapability,
        request: RequestShape::new("POST", "/rpc/hello/gree%74"),
    },
    RawCase {
        rule: Rule::UppercasePrefix,
        request: RequestShape::new("POST", "/RPC/hello/greet"),
    },
    RawCase {
        rule: Rule::UppercaseBox,
        request: RequestShape::new("POST", "/rpc/Hello/greet"),
    },
    RawCase {
        rule: Rule::UppercaseCapability,
        request: RequestShape::new("POST", "/rpc/hello/Greet"),
    },
    RawCase {
        rule: Rule::TrailingSlash,
        request: RequestShape::new("POST", "/rpc/hello/greet/"),
    },
    RawCase {
        rule: Rule::Query,
        request: RequestShape::new("POST", "/rpc/hello/greet?probe=1"),
    },
    RawCase {
        rule: Rule::GetMethod,
        request: RequestShape::new("GET", "/rpc/hello/greet"),
    },
    RawCase {
        rule: Rule::OptionsMethod,
        request: RequestShape::new("OPTIONS", "/rpc/hello/greet"),
    },
    RawCase {
        rule: Rule::UnknownRouteWrongMethod,
        request: RequestShape::new("GET", "/rpc/ghost/greet"),
    },
    RawCase {
        rule: Rule::QueryWrongMethod,
        request: RequestShape::new("GET", "/rpc/hello/greet?probe=1"),
    },
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
    traceability_gate(&RAW_CASES);
    for case in RAW_CASES {
        let dispatches = Arc::new(AtomicUsize::new(0));
        let running = RunningHello::start(CountingHello {
            dispatches: Arc::clone(&dispatches),
        });
        let address = running.local_addr();
        let request = render_request(case.request);
        let expected = case.rule.expected();

        running
            .assert_then_shutdown(async move {
                let response = raw_exchange(address, &request).await;
                assert_response(&response, expected);
                assert_eq!(
                    dispatches.load(Ordering::SeqCst),
                    expected.dispatches,
                    "{:?} dispatch count",
                    case.rule
                );
            })
            .await;
    }
}

#[test]
fn raw_hello_traceability_is_family_scoped_and_mutations_are_active() {
    traceability_gate(&RAW_CASES);
}

fn traceability_gate(cases: &[RawCase]) {
    let expected_rules: BTreeSet<_> = Rule::ALL.into_iter().collect();
    let expected_authorities: BTreeSet<_> = Authority::ALL.into_iter().collect();
    let mut mapped_rules = BTreeSet::new();
    let mut mapped_authorities = BTreeSet::new();

    for case in cases {
        assert!(
            mapped_rules.insert(case.rule),
            "duplicate raw Hello rule: {:?}",
            case.rule
        );
        assert!(
            !case.rule.authorities().is_empty(),
            "{:?} has no S3/Runtime authority",
            case.rule
        );
        mapped_authorities.extend(case.rule.authorities().iter().copied());
        assert!(
            request_matches(case.rule, case.request),
            "{:?} request weakened to {:?}",
            case.rule,
            case.request
        );
    }

    assert_eq!(
        mapped_rules, expected_rules,
        "raw Hello rules are incomplete"
    );
    assert_eq!(
        mapped_authorities, expected_authorities,
        "raw Hello authority coverage drifted"
    );
}

#[test]
fn exact_request_predicates_reject_semantically_weaker_mutants() {
    let mutants = [
        (Rule::PercentEncodedBox, Rule::UnknownBox.request()),
        (
            Rule::PercentEncodedCapability,
            Rule::UnknownCapability.request(),
        ),
        (Rule::UppercasePrefix, Rule::UnknownBox.request()),
        (Rule::UppercaseBox, Rule::UnknownBox.request()),
        (Rule::UppercaseCapability, Rule::UnknownCapability.request()),
        (Rule::TrailingSlash, Rule::UnknownCapability.request()),
        (Rule::Query, Rule::ExactSuccess.request()),
        (Rule::GetMethod, Rule::ExactSuccess.request()),
        (Rule::OptionsMethod, Rule::GetMethod.request()),
        (Rule::UnknownRouteWrongMethod, Rule::UnknownBox.request()),
        (Rule::QueryWrongMethod, Rule::Query.request()),
    ];
    for (rule, mutant) in mutants {
        assert!(
            !request_matches(rule, mutant),
            "{rule:?} accepted {mutant:?}"
        );
    }
}

fn request_matches(rule: Rule, request: RequestShape) -> bool {
    request == rule.request()
}

fn render_request(request: RequestShape) -> Vec<u8> {
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: boxology\r\n\
         Content-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        request.method,
        request.target,
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
fn imf_fixdate_validator_rejects_structural_and_range_mutants() {
    assert!(valid_imf_fixdate(b"Wed, 29 Jul 2026 11:54:22 GMT"));
    for invalid in [
        b"Xxx, 29 Jul 2026 11:54:22 GMT".as_slice(),
        b"Wed; 29 Jul 2026 11:54:22 GMT",
        b"Wed, 00 Jul 2026 11:54:22 GMT",
        b"Wed, 32 Jul 2026 11:54:22 GMT",
        b"Wed, 29 Foo 2026 11:54:22 GMT",
        b"Wed, 29 Jul 20x6 11:54:22 GMT",
        b"Wed, 29 Jul 2026 24:54:22 GMT",
        b"Wed, 29 Jul 2026 11:60:22 GMT",
        b"Wed, 29 Jul 2026 11:54:61 GMT",
        b"Wed, 29 Jul 2026 11:54:22 UTC",
    ] {
        assert!(!valid_imf_fixdate(invalid), "accepted {invalid:?}");
    }
}

fn valid_imf_fixdate(value: &[u8]) -> bool {
    const WEEKDAYS: [&[u8]; 7] = [b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat", b"Sun"];
    const MONTHS: [&[u8]; 12] = [
        b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov",
        b"Dec",
    ];
    value.len() == 29
        && WEEKDAYS.contains(&&value[0..3])
        && value[3..5] == *b", "
        && decimal(&value[5..7]).is_some_and(|day| (1..=31).contains(&day))
        && value[7] == b' '
        && MONTHS.contains(&&value[8..11])
        && value[11] == b' '
        && value[12..16].iter().all(u8::is_ascii_digit)
        && value[16] == b' '
        && decimal(&value[17..19]).is_some_and(|hour| hour <= 23)
        && value[19] == b':'
        && decimal(&value[20..22]).is_some_and(|minute| minute <= 59)
        && value[22] == b':'
        && decimal(&value[23..25]).is_some_and(|second| second <= 60)
        && value[25..29] == *b" GMT"
}

fn decimal(digits: &[u8]) -> Option<u8> {
    digits.iter().all(u8::is_ascii_digit).then(|| {
        digits
            .iter()
            .fold(0, |value, digit| value * 10 + *digit - b'0')
    })
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
