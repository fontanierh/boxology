use boxology_contract::{ContractValue, ErasedCallError, SlotValue, TypeDescriptor, json};

pub(crate) use json::EncodeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WireCallError {
    UnknownBox,
    UnknownCapability,
    InvalidRequest,
    MethodNotAllowed,
    PayloadTooLarge,
    UnsupportedMediaType,
    DeadlineExceeded,
    Unavailable,
    InvalidUpstreamResponse,
    Internal,
}

impl WireCallError {
    pub(crate) const ALL: [Self; 10] = [
        Self::UnknownBox,
        Self::UnknownCapability,
        Self::InvalidRequest,
        Self::MethodNotAllowed,
        Self::PayloadTooLarge,
        Self::UnsupportedMediaType,
        Self::DeadlineExceeded,
        Self::Unavailable,
        Self::InvalidUpstreamResponse,
        Self::Internal,
    ];

    pub(crate) fn spec(self) -> (u16, &'static str, &'static str) {
        match self {
            Self::UnknownBox => (404, "unknown_box", "unknown box"),
            Self::UnknownCapability => (404, "unknown_capability", "unknown capability"),
            Self::InvalidRequest => (400, "invalid_request", "invalid request"),
            Self::MethodNotAllowed => (405, "method_not_allowed", "method not allowed"),
            Self::PayloadTooLarge => (413, "payload_too_large", "payload too large"),
            Self::UnsupportedMediaType => (415, "unsupported_media_type", "unsupported media type"),
            Self::DeadlineExceeded => (504, "deadline_exceeded", "deadline exceeded"),
            Self::Unavailable => (503, "unavailable", "service unavailable"),
            Self::InvalidUpstreamResponse => (
                502,
                "invalid_upstream_response",
                "invalid upstream response",
            ),
            Self::Internal => (500, "internal", "internal error"),
        }
    }

    pub(crate) fn from_erased(error: &ErasedCallError) -> Result<Self, DomainIsNotCallError> {
        match error {
            ErasedCallError::Domain { .. } => Err(DomainIsNotCallError),
            ErasedCallError::Deadline => Ok(Self::DeadlineExceeded),
            ErasedCallError::Cancelled => Ok(Self::Internal),
            ErasedCallError::Unavailable(_) => Ok(Self::Unavailable),
            ErasedCallError::ContractViolation(_) => Ok(Self::InvalidRequest),
            ErasedCallError::InvalidResponse(_) => Ok(Self::InvalidUpstreamResponse),
            ErasedCallError::Internal(_) => Ok(Self::Internal),
            _ => Ok(Self::Internal),
        }
    }

    pub(crate) fn encode(self) -> EncodedCallError {
        let (status, code, message) = self.spec();
        let body = format!(
            "{{\"error\":{{\"kind\":\"call\",\"code\":{},\"message\":{}}}}}",
            serde_json::to_string(code).unwrap(),
            serde_json::to_string(message).unwrap()
        )
        .into_bytes();
        EncodedCallError { status, body }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DomainIsNotCallError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncodedCallError {
    status: u16,
    body: Vec<u8>,
}

impl EncodedCallError {
    pub(crate) fn status(&self) -> u16 {
        self.status
    }
    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }
}

pub(crate) fn encode_request(
    slot: &SlotValue,
    descriptor: &TypeDescriptor,
) -> Result<Vec<u8>, EncodeError> {
    json::encode(slot, descriptor)
}

pub(crate) fn encode_result(
    slot: &SlotValue,
    descriptor: &TypeDescriptor,
) -> Result<Vec<u8>, EncodeError> {
    let value = json::encode(slot, descriptor)?;
    let mut output = br#"{"result":{"value":"#.to_vec();
    output.extend_from_slice(&value);
    output.extend_from_slice(b"}}");
    Ok(output)
}

pub(crate) fn encode_domain(
    error_tag: &str,
    payload: &SlotValue,
    descriptor: &TypeDescriptor,
) -> Result<Vec<u8>, EncodeError> {
    let value = json::encode(
        &SlotValue::Value(ContractValue::enum_value(error_tag, payload.clone())),
        descriptor,
    )?;
    let mut output = br#"{"error":{"kind":"domain","value":"#.to_vec();
    output.extend_from_slice(&value);
    output.extend_from_slice(b"}}");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::Detail;

    #[test]
    fn call_error_mapping_and_envelopes_remain_exact_and_payload_free() {
        let cases = [
            (WireCallError::UnknownBox, 404, "unknown_box", "unknown box"),
            (
                WireCallError::UnknownCapability,
                404,
                "unknown_capability",
                "unknown capability",
            ),
            (
                WireCallError::InvalidRequest,
                400,
                "invalid_request",
                "invalid request",
            ),
            (
                WireCallError::MethodNotAllowed,
                405,
                "method_not_allowed",
                "method not allowed",
            ),
            (
                WireCallError::PayloadTooLarge,
                413,
                "payload_too_large",
                "payload too large",
            ),
            (
                WireCallError::UnsupportedMediaType,
                415,
                "unsupported_media_type",
                "unsupported media type",
            ),
            (
                WireCallError::DeadlineExceeded,
                504,
                "deadline_exceeded",
                "deadline exceeded",
            ),
            (
                WireCallError::Unavailable,
                503,
                "unavailable",
                "service unavailable",
            ),
            (
                WireCallError::InvalidUpstreamResponse,
                502,
                "invalid_upstream_response",
                "invalid upstream response",
            ),
            (WireCallError::Internal, 500, "internal", "internal error"),
        ];
        assert_eq!(cases.map(|case| case.0), WireCallError::ALL);
        for (error, status, code, message) in cases {
            let encoded = error.encode();
            assert_eq!(encoded.status(), status);
            assert_eq!(
                encoded.body(),
                format!(r#"{{"error":{{"kind":"call","code":"{code}","message":"{message}"}}}}"#)
                    .as_bytes()
            );
        }

        let detail = || Detail::new("DO_NOT_LEAK").with_message("DO_NOT_LEAK");
        for (error, expected) in [
            (ErasedCallError::Deadline, WireCallError::DeadlineExceeded),
            (ErasedCallError::Cancelled, WireCallError::Internal),
            (
                ErasedCallError::Unavailable(detail()),
                WireCallError::Unavailable,
            ),
            (
                ErasedCallError::ContractViolation(detail()),
                WireCallError::InvalidRequest,
            ),
            (
                ErasedCallError::InvalidResponse(detail()),
                WireCallError::InvalidUpstreamResponse,
            ),
            (ErasedCallError::Internal(detail()), WireCallError::Internal),
        ] {
            let encoded = WireCallError::from_erased(&error).unwrap().encode();
            assert_eq!(WireCallError::from_erased(&error).unwrap(), expected);
            assert!(!String::from_utf8_lossy(encoded.body()).contains("DO_NOT_LEAK"));
        }
        assert!(matches!(
            WireCallError::from_erased(&ErasedCallError::Domain {
                error_tag: "DO_NOT_LEAK".into(),
                payload: SlotValue::Missing,
            }),
            Err(DomainIsNotCallError)
        ));
    }
}
