use boxology_contract::{
    DecodeRole, Detail, ErasedCallError, OpaqueTree, SlotValue, TypeDescriptor, ValueRef,
};

use crate::{
    encoder::WireCallError,
    semantic::decode_tree,
    syntax::{DEFAULT_DEPTH_LIMIT, SyntaxLimits, parse},
};

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
    use boxology_contract::{FieldDescriptor, VariantDescriptor, VariantPayload};

    fn limits(body: &[u8]) -> ResponseLimits {
        ResponseLimits {
            max_bytes: body.len(),
            max_depth: 128,
        }
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
