use std::time::Instant;

use boxology_contract::{
    CallContext, CapabilityDescriptor, DecodeRole, Detail, ErasedCallError, OpaqueTree, SlotValue,
    TypeDescriptor, ValueRef,
};
use http::{HeaderValue, Method, Request, Uri, Version, header::CONTENT_TYPE, uri::Authority};

use crate::{
    encoder::{WireCallError, encode_request},
    semantic::decode_tree,
    syntax::{DEFAULT_DEPTH_LIMIT, SyntaxLimits, parse},
};

#[derive(Clone)]
pub(crate) struct ClientOrigin(Authority);

impl ClientOrigin {
    pub(crate) fn parse(value: &str) -> Result<Self, Detail> {
        let invalid = || Detail::new("http_base_url");
        if value.contains(['@', '?', '#']) {
            return Err(invalid());
        }
        let uri: Uri = value.parse().map_err(|_| invalid())?;
        if !uri
            .scheme_str()
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http"))
            || !matches!(uri.path(), "" | "/")
        {
            return Err(invalid());
        }
        let authority = uri.authority().cloned().ok_or_else(invalid)?;
        let text = authority.as_str();
        let (port, valid_host) = if let Some(bracketed) = text.strip_prefix('[') {
            let (host, tail) = bracketed.split_once(']').ok_or_else(invalid)?;
            (
                tail.strip_prefix(':'),
                host.parse::<std::net::Ipv6Addr>().is_ok()
                    && (tail.is_empty() || tail.starts_with(':')),
            )
        } else {
            let (host, port) = text
                .rsplit_once(':')
                .map_or((text, None), |(host, port)| (host, Some(port)));
            (
                port,
                host.parse::<std::net::Ipv4Addr>().is_ok() || valid_dns(host),
            )
        };
        if !valid_host
            || port.is_some_and(|port| {
                port.is_empty()
                    || !port.bytes().all(|byte| byte.is_ascii_digit())
                    || port.parse::<u16>().is_err()
            })
            || text.contains(':') && authority.host().contains(':') && !text.starts_with('[')
        {
            return Err(invalid());
        }
        Ok(Self(authority))
    }
}

fn valid_dns(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.is_ascii()
        && host.split('.').all(|label| {
            let bytes = label.as_bytes();
            (1..=63).contains(&bytes.len())
                && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
                && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
                && bytes
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        })
}

pub(crate) fn prepare_request(
    origin: &ClientOrigin,
    capability: &CapabilityDescriptor,
    context: &CallContext,
    input: &SlotValue,
    now: Instant,
) -> Result<Request<Vec<u8>>, ErasedCallError> {
    let uri: Uri = format!(
        "http://{}/rpc/{}/{}",
        origin.0,
        capability.id().box_id(),
        capability.name()
    )
    .parse()
    .map_err(request_error)?;

    let timeout = match context.deadline() {
        None => None,
        Some(deadline) => {
            let remaining = deadline.remaining_at(now);
            if remaining.is_zero() {
                return Err(ErasedCallError::Deadline);
            }
            let millis = remaining.as_nanos().div_ceil(1_000_000).min(9_999_999_999);
            Some(HeaderValue::from_str(&millis.to_string()).map_err(request_error)?)
        }
    };
    let idempotency = context
        .idempotency_key()
        .map(|key| {
            let value = key.as_str().as_bytes();
            if !(1..=256).contains(&value.len())
                || !value
                    .iter()
                    .all(|byte| matches!(byte, 0x21..=0x7e) && *byte != b',')
            {
                return Err(ErasedCallError::ContractViolation(Detail::new(
                    "http_idempotency_key",
                )));
            }
            HeaderValue::from_bytes(value).map_err(request_error)
        })
        .transpose()?;
    let traceparent = context
        .trace()
        .traceparent()
        .and_then(|value| HeaderValue::from_str(value).ok());
    let tracestate = traceparent.as_ref().and_then(|_| {
        context
            .trace()
            .tracestate()
            .and_then(|value| HeaderValue::from_str(value).ok())
    });
    let body = encode_request(input, capability.input()).map_err(request_error)?;
    let mut request = Request::new(body);
    *request.method_mut() = Method::POST;
    *request.version_mut() = Version::HTTP_11;
    *request.uri_mut() = uri;
    request
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    for (name, value) in [
        ("boxology-timeout-ms", timeout),
        ("traceparent", traceparent),
        ("tracestate", tracestate),
        ("idempotency-key", idempotency),
    ] {
        if let Some(value) = value {
            request.headers_mut().insert(name, value);
        }
    }
    Ok(request)
}

fn request_error(_: impl std::fmt::Debug) -> ErasedCallError {
    ErasedCallError::ContractViolation(Detail::new("http_request"))
}

#[derive(Clone, Copy)]
pub(crate) struct ResponseLimits {
    pub(crate) max_bytes: usize,
    pub(crate) max_depth: usize,
}

impl Default for ResponseLimits {
    fn default() -> Self {
        Self {
            max_bytes: 8 * 1024 * 1024,
            max_depth: DEFAULT_DEPTH_LIMIT,
        }
    }
}

pub(crate) fn classify_response(
    status: u16,
    content_types: &[&[u8]],
    body: &[u8],
    output: &TypeDescriptor,
    domain_error: &TypeDescriptor,
    limits: ResponseLimits,
) -> Result<SlotValue, ErasedCallError> {
    if content_types.len() != 1 || !content_types[0].eq_ignore_ascii_case(b"application/json") {
        return invalid();
    }
    let tree = parse(body, SyntaxLimits(limits.max_bytes, limits.max_depth))
        .map_err(|_| invalid_error())?;
    match status {
        200 => decode_result(tree, output),
        422 => decode_domain(tree, domain_error),
        _ => decode_call(status, tree),
    }
}

fn decode_result(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
) -> Result<SlotValue, ErasedCallError> {
    let value = one(one(tree, "result")?, "value")?;
    decode_tree(value, descriptor, DecodeRole::ConsumerOutput).map_err(|_| invalid_error())
}

fn decode_domain(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
) -> Result<SlotValue, ErasedCallError> {
    let error = one(tree, "error")?;
    let OpaqueTree::Object(mut fields) = error else {
        return invalid();
    };
    if fields.len() != 2 {
        return invalid();
    }
    let kind = take(&mut fields, "kind")?;
    if kind != OpaqueTree::String("domain".into()) {
        return invalid();
    }
    let value = take(&mut fields, "value")?;
    let decoded =
        decode_tree(value, descriptor, DecodeRole::ConsumerOutput).map_err(|_| invalid_error())?;
    let SlotValue::Value(value) = decoded else {
        return invalid();
    };
    let ValueRef::Enum { tag, payload } = value.view() else {
        return invalid();
    };
    Err(ErasedCallError::Domain {
        error_tag: tag.into(),
        payload: payload.clone(),
    })
}

fn decode_call(status: u16, tree: OpaqueTree) -> Result<SlotValue, ErasedCallError> {
    let error = one(tree, "error")?;
    let OpaqueTree::Object(mut fields) = error else {
        return invalid();
    };
    if fields.len() != 3 {
        return invalid();
    }
    if take(&mut fields, "kind")? != OpaqueTree::String("call".into()) {
        return invalid();
    }
    let OpaqueTree::String(code) = take(&mut fields, "code")? else {
        return invalid();
    };
    let OpaqueTree::String(message) = take(&mut fields, "message")? else {
        return invalid();
    };
    let Some(wire) = WireCallError::ALL
        .into_iter()
        .find(|wire| wire.spec().1 == code)
    else {
        return invalid();
    };
    let (expected_status, pinned_code, pinned_message) = wire.spec();
    if status != expected_status || message != pinned_message {
        return invalid();
    }
    let detail = || Detail::new(pinned_code).with_message(pinned_message);
    Err(match wire {
        WireCallError::InvalidRequest | WireCallError::PayloadTooLarge => {
            ErasedCallError::ContractViolation(detail())
        }
        WireCallError::UnknownBox
        | WireCallError::UnknownCapability
        | WireCallError::Unavailable => ErasedCallError::Unavailable(detail()),
        WireCallError::DeadlineExceeded => ErasedCallError::Deadline,
        WireCallError::MethodNotAllowed
        | WireCallError::UnsupportedMediaType
        | WireCallError::InvalidUpstreamResponse => ErasedCallError::InvalidResponse(detail()),
        WireCallError::Internal => ErasedCallError::Internal(detail()),
    })
}

fn one(tree: OpaqueTree, key: &str) -> Result<OpaqueTree, ErasedCallError> {
    let OpaqueTree::Object(mut fields) = tree else {
        return invalid();
    };
    if fields.len() != 1 {
        return invalid();
    }
    take(&mut fields, key)
}

fn take(fields: &mut Vec<(String, OpaqueTree)>, key: &str) -> Result<OpaqueTree, ErasedCallError> {
    let Some(index) = fields.iter().position(|(name, _)| name == key) else {
        return invalid();
    };
    Ok(fields.swap_remove(index).1)
}

fn invalid<T>() -> Result<T, ErasedCallError> {
    Err(invalid_error())
}
fn invalid_error() -> ErasedCallError {
    ErasedCallError::InvalidResponse(Detail::new("http_response"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use boxology_contract::{
        BoxId, Caller, CancelToken, CapabilityId, CapabilityName, CapabilityShape, ContractValue,
        Deadline, ExposureLevel, FieldDescriptor, Idempotency, IdempotencyKey, TraceContext,
        VariantDescriptor, VariantPayload,
    };

    fn capability() -> CapabilityDescriptor {
        CapabilityDescriptor::new(
            CapabilityId::new(
                BoxId::new("box-1").unwrap(),
                CapabilityName::new("cap_name").unwrap(),
            ),
            TypeDescriptor::string(),
            TypeDescriptor::string(),
            TypeDescriptor::enumeration([]).unwrap(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        )
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
    fn request(context: &CallContext, now: Instant) -> Result<Request<Vec<u8>>, ErasedCallError> {
        prepare_request(
            &ClientOrigin::parse("http://example.com").unwrap(),
            &capability(),
            context,
            &SlotValue::Value(ContractValue::string("hello")),
            now,
        )
    }

    fn limits(body: &[u8]) -> ResponseLimits {
        ResponseLimits {
            max_bytes: body.len(),
            max_depth: 128,
        }
    }

    #[test]
    fn origins_accept_and_normalize_only_http_origins() {
        for accepted in [
            "http://example.com",
            "http://example.com/",
            "http://localhost",
            "HTTP://example.com:0",
            "http://127.0.0.1:65535",
            "http://[::1]",
            "http://[2001:db8::1]:8080/",
        ] {
            assert!(ClientOrigin::parse(accepted).is_ok(), "{accepted}");
        }
        for rejected in [
            "https://example.com",
            "ftp://example.com",
            "example.com",
            "//example.com",
            "http://",
            "http://user@example.com",
            "http://user:pass@example.com",
            "http://example.com/prefix",
            "http://example.com/api/",
            "http://example.com?",
            "http://example.com#",
            "http://example.com:65536",
            "http://example.com:",
            "http://example.com:+80",
            "http://example.com:-80",
            "http://2001:db8::1",
            "http://[not-ip]",
            "http://[gggg::1]",
            "http://[::1]evil",
            "http://exa!mple.com",
            "http://foo_bar",
            "http://-",
            "http://-example.com",
            "http://example-.com",
            "http://example..com",
        ] {
            let error = ClientOrigin::parse(rejected)
                .err()
                .unwrap_or_else(|| panic!("accepted {rejected}"));
            assert_eq!(error, Detail::new("http_base_url"), "{rejected}");
            assert!(!format!("{error:?}").contains(rejected));
        }
        assert!(ClientOrigin::parse(&format!("http://{}.com", "a".repeat(63))).is_ok());
        assert!(ClientOrigin::parse(&format!("http://{}.com", "a".repeat(64))).is_err());
    }

    #[test]
    fn request_line_headers_and_body_are_exact() {
        let now = Instant::now();
        let request = prepare_request(
            &ClientOrigin::parse("http://example.com:8042/").unwrap(),
            &capability(),
            &context(None, None, Some("orphan=state"), None),
            &SlotValue::Value(ContractValue::string("hello")),
            now,
        )
        .unwrap();
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.version(), Version::HTTP_11);
        assert_eq!(request.uri(), "http://example.com:8042/rpc/box-1/cap_name");
        assert_eq!(request.headers().len(), 1);
        assert_eq!(request.headers()[CONTENT_TYPE], "application/json");
        assert_eq!(request.body(), br#""hello""#);
        assert_eq!(request.uri().path(), "/rpc/box-1/cap_name");
        assert_eq!(request.uri().query(), None);
    }

    #[test]
    fn deadlines_are_ceiled_nonzero_and_clamped() {
        let now = Instant::now();
        assert!(
            request(&context(None, None, None, None), now)
                .unwrap()
                .headers()
                .get("boxology-timeout-ms")
                .is_none()
        );
        assert!(matches!(
            request(&context(Some(Deadline::at(now)), None, None, None), now),
            Err(ErasedCallError::Deadline)
        ));
        for (nanos, expected) in [
            (1, "1"),
            (999_999, "1"),
            (1_000_000, "1"),
            (1_000_001, "2"),
            (9_999_999_999_000_000, "9999999999"),
            (9_999_999_999_000_001, "9999999999"),
            (31_536_000_000_000_000, "9999999999"),
        ] {
            let ctx = context(
                Some(Deadline::at(now + Duration::from_nanos(nanos))),
                None,
                None,
                None,
            );
            let value = request(&ctx, now).unwrap().headers()["boxology-timeout-ms"]
                .to_str()
                .unwrap()
                .to_owned();
            assert_eq!(value, expected);
            assert_ne!(value, "0");
            assert!(!value.starts_with('0'));
        }
    }

    #[test]
    fn tracing_is_best_effort_and_parent_gates_state() {
        let now = Instant::now();
        for (parent, state, expected_parent, expected_state) in [
            (None, None, None, None),
            (None, Some("state=x"), None, None),
            (Some("opaque-parent"), None, Some("opaque-parent"), None),
            (
                Some("opaque-parent"),
                Some("state=x"),
                Some("opaque-parent"),
                Some("state=x"),
            ),
            (Some("bad\nparent"), Some("state=x"), None, None),
            (
                Some("opaque-parent"),
                Some("bad\nstate"),
                Some("opaque-parent"),
                None,
            ),
        ] {
            let request = request(&context(None, parent, state, None), now).unwrap();
            assert_eq!(
                request
                    .headers()
                    .get("traceparent")
                    .and_then(|v| v.to_str().ok()),
                expected_parent
            );
            assert_eq!(
                request
                    .headers()
                    .get("tracestate")
                    .and_then(|v| v.to_str().ok()),
                expected_state
            );
        }
    }

    #[test]
    fn idempotency_and_encoding_failures_are_redacted_contract_errors() {
        let now = Instant::now();
        assert!(IdempotencyKey::new("").is_err());
        for valid in ["!", &"x".repeat(256)] {
            let request = request(&context(None, None, None, Some(valid)), now).unwrap();
            assert_eq!(request.headers()["idempotency-key"], valid);
        }
        for invalid in [
            "x".repeat(257),
            "has space".into(),
            "control\n".into(),
            "has,comma".into(),
            "nonascii-é".into(),
        ] {
            let error = request(&context(None, None, None, Some(&invalid)), now).unwrap_err();
            assert!(
                matches!(error, ErasedCallError::ContractViolation(ref detail) if detail.code() == "http_idempotency_key")
            );
            assert!(!format!("{error:?}").contains(&invalid));
        }
        let error = prepare_request(
            &ClientOrigin::parse("http://example.com").unwrap(),
            &capability(),
            &context(None, None, None, None),
            &SlotValue::Missing,
            now,
        )
        .unwrap_err();
        assert!(
            matches!(error, ErasedCallError::ContractViolation(ref detail) if detail == &Detail::new("http_request"))
        );
        assert!(!format!("{error:?}").contains("missing"));
    }
    fn classify(status: u16, body: &[u8]) -> Result<SlotValue, ErasedCallError> {
        classify_response(
            status,
            &[b"application/json"],
            body,
            &output(),
            &domain(),
            limits(body),
        )
    }
    fn output() -> TypeDescriptor {
        TypeDescriptor::structure(vec![FieldDescriptor::new(
            "known",
            TypeDescriptor::string(),
            None,
        )])
        .unwrap()
    }
    fn domain() -> TypeDescriptor {
        TypeDescriptor::enumeration(vec![VariantDescriptor::new(
            "known",
            VariantPayload::Unit,
            None,
        )])
        .unwrap()
    }
    fn category(error: ErasedCallError) -> &'static str {
        match error {
            ErasedCallError::Domain { .. } => "domain",
            ErasedCallError::Deadline => "deadline",
            ErasedCallError::Unavailable(_) => "unavailable",
            ErasedCallError::ContractViolation(_) => "contract",
            ErasedCallError::InvalidResponse(_) => "invalid",
            ErasedCallError::Internal(_) => "internal",
            _ => "other",
        }
    }
    fn call(code: &str, message: &str) -> Vec<u8> {
        format!(r#"{{"error":{{"message":"{message}","code":"{code}","kind":"call"}}}}"#)
            .into_bytes()
    }
    fn assert_invalid(status: u16, body: &[u8]) {
        assert!(matches!(
            classify(status, body),
            Err(ErasedCallError::InvalidResponse(_))
        ));
    }

    #[test]
    fn result_and_domain_payloads_are_consumer_tolerant() {
        let result = classify(
            200,
            br#"{"result":{"value":{"known":"yes","future":"ignored"}}}"#,
        )
        .unwrap();
        assert!(matches!(result, SlotValue::Value(_)));
        assert!(matches!(
            classify(422, br#"{"error":{"kind":"domain","value":{"tag":"known","payload":null}}}"#).unwrap_err(),
            ErasedCallError::Domain { error_tag, payload: SlotValue::Null } if error_tag == "known"
        ));
        let error = classify(422, br#"{"error":{"value":{"tag":"future","payload":{"sentinel":"secret"}},"kind":"domain"}}"#).unwrap_err();
        assert!(
            matches!(error, ErasedCallError::Domain { error_tag, .. } if error_tag == "future")
        );
    }

    #[test]
    fn canonical_call_table_maps_every_typed_category() {
        #[rustfmt::skip]
        let expected = ["unavailable", "unavailable", "contract", "invalid", "contract", "invalid", "deadline", "unavailable", "invalid", "internal"];
        for (wire, category_name) in WireCallError::ALL.into_iter().zip(expected) {
            let (status, code, message) = wire.spec();
            assert_eq!(
                category(classify(status, &call(code, message)).unwrap_err()),
                category_name,
                "{code}"
            );
        }
    }

    #[test]
    fn envelopes_are_strict_and_status_bound() {
        for wire in WireCallError::ALL {
            let (status, code, message) = wire.spec();
            assert_invalid(if status == 400 { 401 } else { 400 }, &call(code, message));
        }
        for (status, body) in [
            (500, br#"{"error":{"kind":"call","code":"mystery","message":"sentinel"}}"#.as_slice()),
            (500, br#"{"error":{"kind":"call","code":"internal","message":"wrong"}}"#),
            (500, br#"{"error":{"kind":"domain","code":"internal","message":"internal error"}}"#),
            (500, br#"{"error":{"kind":"call","code":"internal","message":"internal error","extra":0}}"#),
            (500, br#"{"error":{"kind":"call","code":"internal","code":"internal","message":"internal error"}}"#),
            (422, br#"{"result":{"value":{"known":"x"}}}"#),
            (200, br#"{"error":{"kind":"domain","value":{"tag":"known","payload":null}}}"#),
            (200, br#"{"result":{"value":{"known":"x"},"extra":0}}"#),
            (200, br#"{"result":{"value":{"known":"x"},"value":{"known":"x"}}}"#),
            (422, br#"{"error":{"kind":"domain","value":{"tag":"known","payload":null},"extra":0}}"#),
        ] {
            assert_invalid(status, body);
        }
        let error = classify(
            500,
            br#"{"error":{"kind":"call","code":"mystery","message":"sentinel"}}"#,
        )
        .unwrap_err();
        assert!(!format!("{error:?}").contains("sentinel"));
    }

    #[test]
    fn rejects_bad_headers_bodies_and_unrecognized_statuses_without_echoing() {
        let body = br#"{"sentinel":"DO_NOT_ECHO"}"#;
        for headers in [
            &[][..],
            &[b"text/json".as_slice()][..],
            &[
                b"application/json".as_slice(),
                b"application/json".as_slice(),
            ][..],
            &[b"application/json; charset=utf-8".as_slice()][..],
        ] {
            let error = classify_response(200, headers, body, &output(), &domain(), limits(body))
                .unwrap_err();
            assert!(!format!("{error:?}").contains("DO_NOT_ECHO"));
        }
        assert!(
            classify_response(
                200,
                &[b"APPLICATION/JSON"],
                br#"{"result":{"value":{"known":"x"}}}"#,
                &output(),
                &domain(),
                ResponseLimits::default()
            )
            .is_ok()
        );
        for (status, body) in [
            (100, b"{}".as_slice()),
            (204, b""),
            (302, b"{"),
            (201, b"[]"),
            (599, b"null"),
        ] {
            assert_invalid(status, body);
        }
    }

    #[test]
    fn byte_and_depth_limits_are_inclusive() {
        let body = br#"{"result":{"value":{"known":"x"}}}"#;
        assert!(
            classify_response(
                200,
                &[b"application/json"],
                body,
                &output(),
                &domain(),
                limits(body)
            )
            .is_ok()
        );
        let too_small = ResponseLimits {
            max_bytes: body.len() - 1,
            max_depth: 128,
        };
        assert_eq!(
            category(
                classify_response(
                    200,
                    &[b"application/json"],
                    body,
                    &output(),
                    &domain(),
                    too_small
                )
                .unwrap_err()
            ),
            "invalid"
        );
        let exact = ResponseLimits {
            max_bytes: body.len(),
            max_depth: 3,
        };
        assert!(
            classify_response(
                200,
                &[b"application/json"],
                body,
                &output(),
                &domain(),
                exact
            )
            .is_ok()
        );
        let shallow = ResponseLimits {
            max_bytes: body.len(),
            max_depth: 2,
        };
        assert_eq!(
            category(
                classify_response(
                    200,
                    &[b"application/json"],
                    body,
                    &output(),
                    &domain(),
                    shallow
                )
                .unwrap_err()
            ),
            "invalid"
        );
    }
}
