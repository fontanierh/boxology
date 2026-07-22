use crate::{
    encoder::WireCallError,
    semantic::decode_tree,
    syntax::{SyntaxError, SyntaxLimits, parse},
};
use boxology_contract::{
    BoxId, CapabilityDescriptor, CapabilityName, DecodeRole, ExposureLevel, IdempotencyKey,
    SlotValue, TraceContext, TypeDescriptor,
};
use boxology_runtime::TransportExposure;
use http::{HeaderMap, HeaderValue};
use std::time::Duration;

const TIMEOUT_HEADER: &str = "boxology-timeout-ms";
const IDEMPOTENCY_HEADER: &str = "idempotency-key";
const TRACEPARENT_HEADER: &str = "traceparent";
const TRACESTATE_HEADER: &str = "tracestate";
const MAX_TRACESTATE_BYTES: usize = 512;

fn decode_request_body(
    body: &[u8],
    descriptor: &TypeDescriptor,
    limits: SyntaxLimits,
) -> Result<SlotValue, WireCallError> {
    let tree = parse(body, limits).map_err(|error| match error {
        SyntaxError::PayloadTooLarge { .. } => WireCallError::PayloadTooLarge,
        _ => WireCallError::InvalidRequest,
    })?;
    decode_tree(tree, descriptor, DecodeRole::ProviderInput)
        .map_err(|_| WireCallError::InvalidRequest)
}

fn one_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a HeaderValue>, WireCallError> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
        return Err(WireCallError::InvalidRequest);
    }
    Ok(first)
}

fn parse_timeout(headers: &HeaderMap) -> Result<Option<Duration>, WireCallError> {
    let Some(value) = one_header(headers, TIMEOUT_HEADER)? else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 10
        || (bytes.len() > 1 && bytes[0] == b'0')
        || bytes.iter().any(|byte| !byte.is_ascii_digit())
    {
        return Err(WireCallError::InvalidRequest);
    }
    let millis = bytes
        .iter()
        .fold(0_u64, |value, byte| value * 10 + u64::from(byte - b'0'));
    if millis > 9_999_999_999 {
        return Err(WireCallError::InvalidRequest);
    }
    Ok(Some(Duration::from_millis(millis)))
}

fn parse_idempotency_key(headers: &HeaderMap) -> Result<Option<IdempotencyKey>, WireCallError> {
    let Some(value) = one_header(headers, IDEMPOTENCY_HEADER)? else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 256
        || bytes
            .iter()
            .any(|byte| !(0x21..=0x7e).contains(byte) || *byte == b',')
    {
        return Err(WireCallError::InvalidRequest);
    }
    let key = std::str::from_utf8(bytes).map_err(|_| WireCallError::InvalidRequest)?;
    IdempotencyKey::new(key)
        .map(Some)
        .map_err(|_| WireCallError::InvalidRequest)
}

fn parse_trace_context(headers: &HeaderMap) -> TraceContext {
    let mut parents = headers.get_all(TRACEPARENT_HEADER).iter();
    let Some(parent) = parents.next() else {
        return TraceContext::empty();
    };
    if parents.next().is_some() || !valid_traceparent(parent.as_bytes()) {
        return TraceContext::empty();
    }
    let Ok(parent) = parent.to_str() else {
        return TraceContext::empty();
    };
    let state = parse_tracestate(headers);
    TraceContext::new(Some(parent.to_owned()), state)
}

fn valid_traceparent(value: &[u8]) -> bool {
    if value.len() < 55
        || value[2] != b'-'
        || value[35] != b'-'
        || value[52] != b'-'
        || !value[..55]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() || *byte == b'-')
    {
        return false;
    }
    let lowercase_hex = |part: &[u8]| {
        part.iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    };
    if !lowercase_hex(&value[..2])
        || value[..2] == *b"ff"
        || !lowercase_hex(&value[3..35])
        || value[3..35].iter().all(|byte| *byte == b'0')
        || !lowercase_hex(&value[36..52])
        || value[36..52].iter().all(|byte| *byte == b'0')
        || !lowercase_hex(&value[53..55])
    {
        return false;
    }
    if value[..2] == *b"00" {
        value.len() == 55
    } else {
        value.len() == 55 || value[55] == b'-'
    }
}

fn parse_tracestate(headers: &HeaderMap) -> Option<String> {
    let values: Vec<_> = headers.get_all(TRACESTATE_HEADER).iter().collect();
    if values.is_empty() {
        return None;
    }
    let combined_len = values
        .iter()
        .map(|value| value.as_bytes().len())
        .sum::<usize>()
        + values.len().saturating_sub(1);
    if combined_len > MAX_TRACESTATE_BYTES {
        return None;
    }
    let mut combined = String::with_capacity(combined_len);
    for (index, value) in values.into_iter().enumerate() {
        if index != 0 {
            combined.push(',');
        }
        combined.push_str(value.to_str().ok()?);
    }
    valid_tracestate(&combined).then_some(combined)
}

fn valid_tracestate(value: &str) -> bool {
    let mut keys = Vec::new();
    let members: Vec<_> = value.split(',').collect();
    if members.len() > 32 {
        return false;
    }
    let last = members.len() - 1;
    for (index, member) in members.into_iter().enumerate() {
        let member = member.trim_start_matches([' ', '\t']);
        let member = if index == last {
            member
        } else {
            member.trim_end_matches([' ', '\t'])
        };
        if member.is_empty() {
            continue;
        }
        let Some((key, state)) = member.split_once('=') else {
            return false;
        };
        if !valid_tracestate_key(key)
            || state.is_empty()
            || state.len() > 256
            || state
                .as_bytes()
                .iter()
                .any(|byte| !(0x20..=0x7e).contains(byte) || *byte == b',' || *byte == b'=')
            || state.ends_with(' ')
            || keys.contains(&key)
        {
            return false;
        }
        keys.push(key);
    }
    true
}

fn valid_tracestate_key(key: &str) -> bool {
    let allowed = |byte: u8| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'-' | b'*' | b'/')
    };
    if let Some((tenant, system)) = key.split_once('@') {
        !tenant.is_empty()
            && tenant.len() <= 241
            && (tenant.as_bytes()[0].is_ascii_lowercase() || tenant.as_bytes()[0].is_ascii_digit())
            && tenant.bytes().all(allowed)
            && !system.is_empty()
            && system.len() <= 14
            && system.as_bytes()[0].is_ascii_lowercase()
            && system.bytes().all(allowed)
    } else {
        !key.is_empty()
            && key.len() <= 256
            && key.as_bytes()[0].is_ascii_lowercase()
            && key.bytes().all(allowed)
    }
}

trait ExposureView {
    fn descriptor(&self) -> &CapabilityDescriptor;
    fn level(&self) -> ExposureLevel;
}

impl ExposureView for TransportExposure {
    fn descriptor(&self) -> &CapabilityDescriptor {
        self.descriptor()
    }

    fn level(&self) -> ExposureLevel {
        self.level()
    }
}

fn resolve_route<'a, E: ExposureView>(
    raw_path: &str,
    query_present: bool,
    exposures: &'a [E],
) -> Result<&'a E, WireCallError> {
    let Some(rest) = raw_path.strip_prefix("/rpc/") else {
        return Err(WireCallError::UnknownBox);
    };
    let (box_segment, capability_segment) = rest.split_once('/').unwrap_or((rest, ""));
    let box_id = BoxId::new(box_segment).map_err(|_| WireCallError::UnknownBox)?;
    let box_known = exposures
        .iter()
        .any(|exposure| exposure.descriptor().id().box_id() == &box_id);
    if !box_known {
        return Err(WireCallError::UnknownBox);
    }
    if capability_segment.contains('/') {
        return Err(WireCallError::UnknownCapability);
    }
    let capability =
        CapabilityName::new(capability_segment).map_err(|_| WireCallError::UnknownCapability)?;
    let exposure = exposures
        .iter()
        .find(|exposure| {
            let id = exposure.descriptor().id();
            id.box_id() == &box_id && id.name() == &capability
        })
        .ok_or(WireCallError::UnknownCapability)?;
    if query_present {
        return Err(WireCallError::InvalidRequest);
    }
    Ok(exposure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conformance::conform_capability;
    use boxology_contract::{
        CapabilityId, CapabilityShape, ContractValue, FieldDescriptor, Idempotency,
        VariantDescriptor, VariantPayload,
    };
    use http::{HeaderValue, header::ACCEPT};

    #[derive(Debug, PartialEq, Eq)]
    struct Exposure {
        descriptor: CapabilityDescriptor,
        level: ExposureLevel,
    }

    impl ExposureView for Exposure {
        fn descriptor(&self) -> &CapabilityDescriptor {
            &self.descriptor
        }
        fn level(&self) -> ExposureLevel {
            self.level
        }
    }

    fn capability(box_id: &str, name: &str, input: TypeDescriptor) -> CapabilityDescriptor {
        CapabilityDescriptor::new(
            CapabilityId::new(
                BoxId::new(box_id).unwrap(),
                CapabilityName::new(name).unwrap(),
            ),
            input,
            TypeDescriptor::string(),
            TypeDescriptor::enumeration([]).unwrap(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        )
    }

    fn exposure(box_id: &str, name: &str, level: ExposureLevel) -> Exposure {
        Exposure {
            descriptor: capability(box_id, name, TypeDescriptor::string()),
            level,
        }
    }

    fn with_slots(
        shape: CapabilityShape,
        input: TypeDescriptor,
        output: TypeDescriptor,
        error: TypeDescriptor,
    ) -> CapabilityDescriptor {
        let mut descriptor = capability("box", "call", input);
        descriptor = CapabilityDescriptor::new(
            descriptor.id().clone(),
            descriptor.input().clone(),
            output,
            error,
            shape,
            ExposureLevel::External,
            Idempotency::None,
            None,
        );
        descriptor
    }

    fn body_limits(max_bytes: usize) -> SyntaxLimits {
        SyntaxLimits(max_bytes, crate::syntax::DEFAULT_DEPTH_LIMIT)
    }

    fn assert_body_error(
        body: &[u8],
        descriptor: &TypeDescriptor,
        limits: SyntaxLimits,
        expected: WireCallError,
    ) {
        let error = decode_request_body(body, descriptor, limits).unwrap_err();
        assert_eq!(error, expected);
        let encoded = error.encode();
        let (status, body) = match expected {
            WireCallError::InvalidRequest => (400, br#"{"error":{"kind":"call","code":"invalid_request","message":"invalid request"}}"#.as_slice()),
            WireCallError::PayloadTooLarge => (413, br#"{"error":{"kind":"call","code":"payload_too_large","message":"payload too large"}}"#.as_slice()),
            _ => unreachable!(),
        };
        assert_eq!(encoded.status(), status);
        assert_eq!(encoded.body(), body);
        assert!(
            !encoded
                .body()
                .windows(11)
                .any(|bytes| bytes == b"DO_NOT_LEAK")
        );
    }

    #[test]
    fn request_body_decodes_plain_structured_and_sensitive_values() {
        let string = TypeDescriptor::string();
        assert_eq!(
            decode_request_body(br#""plain""#, &string, body_limits(7)),
            Ok(SlotValue::Value(ContractValue::string("plain")))
        );
        let structure = TypeDescriptor::structure([
            FieldDescriptor::new("name", string.clone(), None),
            FieldDescriptor::new("active", TypeDescriptor::bool(), None),
        ])
        .unwrap();
        let expected = ContractValue::object([
            ("name".into(), ContractValue::string("Ada")),
            ("active".into(), ContractValue::bool(true)),
        ])
        .unwrap();
        let structured = br#"{"name":"Ada","active":true}"#;
        assert_eq!(
            decode_request_body(structured, &structure, body_limits(structured.len())),
            Ok(SlotValue::Value(expected))
        );
        let secret = TypeDescriptor::secret(string).unwrap();
        assert_eq!(
            decode_request_body(br#""DO_NOT_LEAK""#, &secret, body_limits(13)),
            Ok(SlotValue::Value(ContractValue::sensitive(
                ContractValue::string("DO_NOT_LEAK")
            )))
        );
    }

    #[test]
    fn request_body_enforces_byte_limit_before_payload_inspection() {
        let body = br#""okay""#;
        assert!(
            decode_request_body(body, &TypeDescriptor::string(), body_limits(body.len())).is_ok()
        );
        assert_body_error(
            body,
            &TypeDescriptor::string(),
            body_limits(body.len() - 1),
            WireCallError::PayloadTooLarge,
        );
        assert_body_error(
            b"{DO_NOT_LEAK",
            &TypeDescriptor::string(),
            body_limits(1),
            WireCallError::PayloadTooLarge,
        );
    }

    #[test]
    fn request_body_maps_syntax_failures_to_canonical_bad_request() {
        let string = TypeDescriptor::string();
        for body in [
            b"".as_slice(),
            &[0xff],
            b"DO_NOT_LEAK",
            br#""value" trailing"#,
        ] {
            assert_body_error(
                body,
                &string,
                body_limits(body.len()),
                WireCallError::InvalidRequest,
            );
        }
        assert_body_error(
            b"[[]]",
            &TypeDescriptor::list(TypeDescriptor::list(string).unwrap()).unwrap(),
            SyntaxLimits(4, 1),
            WireCallError::InvalidRequest,
        );
    }

    #[test]
    fn request_body_maps_provider_semantic_failures_to_canonical_bad_request() {
        let map = TypeDescriptor::map(TypeDescriptor::string()).unwrap();
        let structure = TypeDescriptor::structure([FieldDescriptor::new(
            "known",
            TypeDescriptor::string(),
            None,
        )])
        .unwrap();
        let enumeration = TypeDescriptor::enumeration([VariantDescriptor::new(
            "known",
            VariantPayload::Unit,
            None,
        )])
        .unwrap();
        for (body, descriptor) in [
            (br#"{"DO_NOT_LEAK":"a","DO_NOT_LEAK":"b"}"#.as_slice(), &map),
            (br#"{"DO_NOT_LEAK":"value"}"#, &structure),
            (br#"{"tag":"DO_NOT_LEAK","payload":null}"#, &enumeration),
            (br#""01""#, &TypeDescriptor::i64()),
        ] {
            assert_body_error(
                body,
                descriptor,
                body_limits(body.len()),
                WireCallError::InvalidRequest,
            );
        }
    }

    #[test]
    fn exact_route_returns_the_selected_exposure_and_runtime_seam_exists() {
        fn actual_runtime_exposure_uses_view<T: ExposureView>() {}
        actual_runtime_exposure_uses_view::<TransportExposure>();
        let exposures = [
            exposure("alpha", "read", ExposureLevel::Internal),
            exposure("alpha", "write", ExposureLevel::External),
            exposure("beta", "read", ExposureLevel::CodeOnly),
        ];
        let selected = resolve_route("/rpc/alpha/write", false, &exposures).unwrap();
        assert_eq!(selected.descriptor().id().to_string(), "alpha.write");
        assert_eq!(selected.level(), ExposureLevel::External);
        assert_eq!(
            resolve_route("/rpc/beta/read", false, &exposures)
                .unwrap()
                .level(),
            ExposureLevel::CodeOnly
        );
    }

    #[test]
    fn malformed_and_unknown_routes_have_canonical_distinct_errors() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        for path in [
            "",
            "/",
            "/RPC/known/call",
            "/rpc",
            "/rpc/",
            "/rpc//call",
            "/rpc/Known/call",
            "/rpc/known%2fother/call",
            "/rpc/ghost/call",
            "/rpc_ignored/known/call",
        ] {
            assert_eq!(
                resolve_route(path, false, &exposures),
                Err(WireCallError::UnknownBox),
                "{path}"
            );
        }
        for path in [
            "/rpc/known",
            "/rpc/known/",
            "/rpc/known/Call",
            "/rpc/known/call%20",
            "/rpc/known/ghost",
            "/rpc/known/call/",
            "/rpc/known/call/extra",
            "/rpc/known//call",
        ] {
            assert_eq!(
                resolve_route(path, false, &exposures),
                Err(WireCallError::UnknownCapability),
                "{path}"
            );
        }
        let box_error = WireCallError::UnknownBox.encode();
        let capability_error = WireCallError::UnknownCapability.encode();
        assert_eq!(box_error.status(), 404);
        assert_eq!(capability_error.status(), 404);
        assert_ne!(box_error.body(), capability_error.body());
        assert_eq!(
            box_error.body(),
            br#"{"error":{"kind":"call","code":"unknown_box","message":"unknown box"}}"#
        );
        assert_eq!(capability_error.body(), br#"{"error":{"kind":"call","code":"unknown_capability","message":"unknown capability"}}"#);
    }

    #[test]
    fn route_precedes_query_validation() {
        let exposures = [exposure("known", "call", ExposureLevel::Internal)];
        assert_eq!(
            resolve_route("/rpc/known/call", true, &exposures),
            Err(WireCallError::InvalidRequest)
        );
        assert_eq!(
            resolve_route("/rpc/known/ghost", true, &exposures),
            Err(WireCallError::UnknownCapability)
        );
        assert_eq!(
            resolve_route("/rpc/ghost/call", true, &exposures),
            Err(WireCallError::UnknownBox)
        );
    }

    #[test]
    fn rejects_every_non_unary_shape_before_presence() {
        let tri = TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap();
        for shape in [
            CapabilityShape::ServerStreaming,
            CapabilityShape::ClientStreaming,
            CapabilityShape::BidirectionalStreaming,
            CapabilityShape::EventSubscription,
        ] {
            let error =
                conform_capability(&with_slots(shape, tri.clone(), tri.clone(), tri.clone()))
                    .unwrap_err();
            assert_eq!(error.code(), "http_non_unary");
        }
    }

    #[test]
    fn top_level_field_is_rejected_in_each_slot_with_stable_precedence() {
        let plain = || TypeDescriptor::string();
        let field = || TypeDescriptor::tri_state(plain()).unwrap();
        for (descriptor, slot) in [
            (
                with_slots(CapabilityShape::Unary, field(), plain(), plain()),
                "input",
            ),
            (
                with_slots(CapabilityShape::Unary, plain(), field(), plain()),
                "output",
            ),
            (
                with_slots(CapabilityShape::Unary, plain(), plain(), field()),
                "error",
            ),
        ] {
            let error = conform_capability(&descriptor).unwrap_err();
            assert_eq!(error.code(), "http_top_level_field");
            assert_eq!(
                error.message(),
                Some(format!("HTTP cannot represent top-level Field in {slot}").as_str())
            );
        }
        let all = with_slots(CapabilityShape::Unary, field(), field(), field());
        assert_eq!(
            conform_capability(&all).unwrap_err().message(),
            Some("HTTP cannot represent top-level Field in input")
        );
    }

    #[test]
    fn secret_rejects_deep_presence_across_all_aggregate_kinds_without_leaking_names() {
        let optional = TypeDescriptor::optional(TypeDescriptor::string()).unwrap();
        let enumeration = TypeDescriptor::enumeration([VariantDescriptor::new(
            "variant-sentinel",
            VariantPayload::Value(optional),
            None,
        )])
        .unwrap();
        let nested = TypeDescriptor::structure([FieldDescriptor::new(
            "payload-sentinel",
            TypeDescriptor::list(TypeDescriptor::map(enumeration).unwrap()).unwrap(),
            None,
        )])
        .unwrap();
        let error = conform_capability(&with_slots(
            CapabilityShape::Unary,
            TypeDescriptor::string(),
            TypeDescriptor::secret(nested).unwrap(),
            TypeDescriptor::string(),
        ))
        .unwrap_err();
        assert_eq!(error.code(), "http_secret_presence");
        assert_eq!(
            error.message(),
            Some("HTTP cannot represent presence inside Secret")
        );
        assert!(!error.to_string().contains("sentinel"));

        let tri_in_struct = TypeDescriptor::structure([FieldDescriptor::new(
            "field",
            TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
            None,
        )])
        .unwrap();
        let error_slot = TypeDescriptor::secret(tri_in_struct).unwrap();
        assert_eq!(
            conform_capability(&with_slots(
                CapabilityShape::Unary,
                TypeDescriptor::string(),
                TypeDescriptor::string(),
                error_slot
            ))
            .unwrap_err()
            .code(),
            "http_secret_presence"
        );
    }

    #[test]
    fn accepts_supported_presence_and_secret_shapes() {
        let object_field = TypeDescriptor::structure([FieldDescriptor::new(
            "field",
            TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
            None,
        )])
        .unwrap();
        for input in [
            TypeDescriptor::string(),
            TypeDescriptor::optional(TypeDescriptor::string()).unwrap(),
            object_field,
            TypeDescriptor::secret(TypeDescriptor::string()).unwrap(),
            TypeDescriptor::optional(TypeDescriptor::secret(TypeDescriptor::string()).unwrap())
                .unwrap(),
        ] {
            conform_capability(&with_slots(
                CapabilityShape::Unary,
                input,
                TypeDescriptor::string(),
                TypeDescriptor::string(),
            ))
            .unwrap();
        }
    }

    fn headers(name: &'static str, value: &[u8]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_bytes(value).unwrap());
        headers
    }

    #[test]
    fn timeout_header_accepts_exact_grammar_and_ignores_unrelated_headers() {
        assert_eq!(parse_timeout(&HeaderMap::new()), Ok(None));
        for (raw, millis) in [
            (b"0".as_slice(), 0),
            (b"1", 1),
            (b"9999999999", 9_999_999_999),
        ] {
            assert_eq!(
                parse_timeout(&headers(TIMEOUT_HEADER, raw)),
                Ok(Some(Duration::from_millis(millis)))
            );
        }
        let mut mixed = headers("BoXoLoGy-TiMeOuT-Ms", b"7");
        mixed.insert(ACCEPT, HeaderValue::from_static("text/plain"));
        mixed.insert("x-proxy-note", HeaderValue::from_static("ignored"));
        assert_eq!(parse_timeout(&mixed), Ok(Some(Duration::from_millis(7))));
    }

    #[test]
    fn timeout_header_rejects_every_malformed_or_duplicate_form() {
        for raw in [
            b"".as_slice(),
            b"00",
            b"01",
            b"+1",
            b"-1",
            b" 1",
            b"1 ",
            b"10000000000",
            b"one",
            b"1,2",
            &[0x80],
        ] {
            assert_eq!(
                parse_timeout(&headers(TIMEOUT_HEADER, raw)),
                Err(WireCallError::InvalidRequest)
            );
        }
        for duplicate in [b"1".as_slice(), b"2"] {
            let mut values = headers(TIMEOUT_HEADER, b"1");
            values.append(TIMEOUT_HEADER, HeaderValue::from_bytes(duplicate).unwrap());
            assert_eq!(parse_timeout(&values), Err(WireCallError::InvalidRequest));
        }
    }

    #[test]
    fn idempotency_header_accepts_boundaries_and_preserves_only_the_key() {
        assert_eq!(parse_idempotency_key(&HeaderMap::new()), Ok(None));
        let boundary = vec![b'x'; 256];
        for raw in [b"!".as_slice(), b"~", boundary.as_slice()] {
            let parsed = parse_idempotency_key(&headers("IdEmPoTeNcY-KeY", raw))
                .unwrap()
                .unwrap();
            assert_eq!(parsed.as_str().as_bytes(), raw);
        }
    }

    #[test]
    fn idempotency_header_rejects_every_malformed_or_duplicate_form() {
        let too_long = vec![b'x'; 257];
        for raw in [
            b"".as_slice(),
            too_long.as_slice(),
            b"a b",
            b"a\tb",
            &[0x80],
            b"a,b",
        ] {
            assert_eq!(
                parse_idempotency_key(&headers(IDEMPOTENCY_HEADER, raw)),
                Err(WireCallError::InvalidRequest)
            );
        }
        // `http` refuses these before a `HeaderMap` can carry them. The parser's
        // visible-ASCII check independently excludes the same byte ranges.
        assert!(HeaderValue::from_bytes(&[0x1f]).is_err());
        assert!(HeaderValue::from_bytes(&[0x7f]).is_err());
        let mut duplicate = headers(IDEMPOTENCY_HEADER, b"same");
        duplicate.append(IDEMPOTENCY_HEADER, HeaderValue::from_static("same"));
        assert_eq!(
            parse_idempotency_key(&duplicate),
            Err(WireCallError::InvalidRequest)
        );
    }

    const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    fn trace_headers(parent: Option<&str>, states: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(parent) = parent {
            headers.insert(TRACEPARENT_HEADER, HeaderValue::from_str(parent).unwrap());
        }
        for state in states {
            headers.append(TRACESTATE_HEADER, HeaderValue::from_str(state).unwrap());
        }
        headers
    }

    #[test]
    fn traceparent_accepts_level_one_and_future_prefixes_opaquely() {
        for parent in [
            PARENT.to_owned(),
            PARENT.replacen("00-", "01-", 1),
            format!("{}-future-opaque", PARENT.replacen("00-", "fe-", 1)),
        ] {
            let parsed = parse_trace_context(&trace_headers(Some(&parent), &[]));
            assert_eq!(parsed.traceparent(), Some(parent.as_str()));
            assert_eq!(parsed.tracestate(), None);
        }
    }

    #[test]
    fn malformed_or_duplicate_traceparent_drops_the_entire_context() {
        for parent in [
            "",
            "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4BF92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00F067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-0A",
            "00-4bf92f3577b34da6a3ce929d0e0e4736_00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01x",
        ] {
            let parsed = parse_trace_context(&trace_headers(Some(parent), &["vendor=state"]));
            assert_eq!(parsed.traceparent(), None, "accepted {parent:?}");
            assert_eq!(parsed.tracestate(), None);
        }

        let mut duplicate = trace_headers(Some(PARENT), &["vendor=state"]);
        duplicate.append(TRACEPARENT_HEADER, HeaderValue::from_static(PARENT));
        let parsed = parse_trace_context(&duplicate);
        assert_eq!(parsed.traceparent(), None);
        assert_eq!(parsed.tracestate(), None);
    }

    #[test]
    fn tracestate_accepts_level_one_grammar_and_preserves_combined_bytes() {
        for state in [
            "vendor=value",
            "tenant-1@system=value",
            " ,\t, vendor=value , ",
            "",
        ] {
            let parsed = parse_trace_context(&trace_headers(Some(PARENT), &[state]));
            assert_eq!(parsed.tracestate(), Some(state));
        }

        let parsed = parse_trace_context(&trace_headers(Some(PARENT), &["rojo=one", "congo=two"]));
        assert_eq!(parsed.tracestate(), Some("rojo=one,congo=two"));

        let members = (0..32)
            .map(|index| format!("k{index}=v"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            parse_trace_context(&trace_headers(Some(PARENT), &[&members])).tracestate(),
            Some(members.as_str())
        );
        let max_value = format!("vendor={}", "v".repeat(256));
        assert_eq!(
            parse_trace_context(&trace_headers(Some(PARENT), &[&max_value])).tracestate(),
            Some(max_value.as_str())
        );
        let max_combined = format!("a={},b={}", "v".repeat(256), "w".repeat(251));
        assert_eq!(max_combined.len(), 512);
        assert_eq!(
            parse_trace_context(&trace_headers(Some(PARENT), &[&max_combined])).tracestate(),
            Some(max_combined.as_str())
        );
    }

    #[test]
    fn invalid_tracestate_drops_only_state() {
        let too_many = (0..33)
            .map(|index| format!("k{index}=v"))
            .collect::<Vec<_>>()
            .join(",");
        let too_large = format!("a={},b={}", "v".repeat(256), "w".repeat(252));
        assert_eq!(too_large.len(), 513);
        for state in [
            "Vendor=value",
            "1vendor=value",
            "tenant@System=value",
            "vendor",
            "vendor=",
            "vendor=one,vendor=two",
            "vendor=bad=value",
            "vendor=bad\tvalue",
            "vendor=trailing ",
            too_many.as_str(),
            too_large.as_str(),
        ] {
            let parsed = parse_trace_context(&trace_headers(Some(PARENT), &[state]));
            assert_eq!(
                parsed.traceparent(),
                Some(PARENT),
                "lost parent for {state:?}"
            );
            assert_eq!(parsed.tracestate(), None, "accepted {state:?}");
        }
        let orphan = parse_trace_context(&trace_headers(None, &["vendor=value"]));
        assert_eq!(orphan.traceparent(), None);
        assert_eq!(orphan.tracestate(), None);
    }
}
