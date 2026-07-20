//! Typed and erased invocation failures.

use std::error::Error;
use std::fmt;

use crate::{ContractError, ContractValue, DecodeRole, SlotValue, TypeDescriptor, ValueRef};

/// Producer-owned string diagnostics for an invocation failure.
///
/// The code identifies a diagnostic within its producer and is not part of
/// the S3 wire-envelope code namespace. Keeping detail content string-only
/// makes it structurally incapable of embedding contract value subtrees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    code: String,
    message: Option<String>,
}

impl Detail {
    /// Constructs detail with a producer-owned code and no message.
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: None,
        }
    }

    /// Adds a producer-owned diagnostic message.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Returns the producer-owned diagnostic code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the diagnostic message, when present.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl fmt::Display for Detail {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.message() {
            Some(message) => write!(formatter, "{}: {message}", self.code()),
            None => formatter.write_str(self.code()),
        }
    }
}

/// A typed domain outcome or failure to complete or interpret a call.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallError<E> {
    /// The capability returned its declared domain error.
    Domain(E),
    /// The call deadline expired.
    Deadline,
    /// The call was cancelled.
    Cancelled,
    /// The target was unavailable.
    Unavailable(Detail),
    /// Caller input violated the contract.
    ContractViolation(Detail),
    /// The provider produced an invalid response.
    InvalidResponse(Detail),
    /// The invocation failed internally.
    Internal(Detail),
}

impl<E> fmt::Display for CallError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(_) => formatter.write_str("domain error"),
            Self::Deadline => formatter.write_str("deadline exceeded"),
            Self::Cancelled => formatter.write_str("call cancelled"),
            Self::Unavailable(detail) => write!(formatter, "unavailable: {detail}"),
            Self::ContractViolation(detail) => {
                write!(formatter, "contract violation: {detail}")
            }
            Self::InvalidResponse(detail) => write!(formatter, "invalid response: {detail}"),
            Self::Internal(detail) => write!(formatter, "internal error: {detail}"),
        }
    }
}

impl<E: fmt::Debug> Error for CallError<E> {}

/// A concrete invocation failure crossing the erased dispatch boundary.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ErasedCallError {
    /// A decomposed domain-error variant and its variant payload slot.
    ///
    /// Known unit variants use [`SlotValue::Null`]. Unknown variants are
    /// descriptor-guided and may capture any received slot opaquely.
    Domain {
        /// The stable domain-error variant tag.
        error_tag: String,
        /// The decomposed variant payload.
        payload: SlotValue,
    },
    /// The call deadline expired.
    Deadline,
    /// The call was cancelled.
    Cancelled,
    /// The target was unavailable.
    Unavailable(Detail),
    /// Caller input violated the contract.
    ContractViolation(Detail),
    /// The provider produced an invalid response.
    InvalidResponse(Detail),
    /// The invocation failed internally.
    Internal(Detail),
}

impl ErasedCallError {
    /// Converts a generated domain error into its erased tag and payload slot.
    #[doc(hidden)]
    pub fn from_domain<E: ContractError>(error: &E) -> ErasedCallError {
        let encoded = match error.encode() {
            Ok(encoded) => encoded,
            Err(error) => {
                return Self::InvalidResponse(conversion_detail("domain_error_encode", error));
            }
        };
        let SlotValue::Value(value) = encoded else {
            return Self::InvalidResponse(Detail::new("domain_error_shape"));
        };
        let ValueRef::Enum { tag, payload } = value.view() else {
            return Self::InvalidResponse(Detail::new("domain_error_shape"));
        };
        Self::Domain {
            error_tag: tag.into(),
            payload: payload.clone(),
        }
    }

    /// Converts an erased failure back to a generated typed call error.
    #[doc(hidden)]
    pub fn into_typed<E: ContractError>(self, error_descriptor: &TypeDescriptor) -> CallError<E> {
        match self {
            Self::Domain { error_tag, payload } => {
                let encoded = SlotValue::Value(ContractValue::enum_value(error_tag, payload));
                let conformed = match error_descriptor.conform(DecodeRole::ConsumerOutput, encoded)
                {
                    Ok(conformed) => conformed,
                    Err(error) => {
                        return CallError::InvalidResponse(conversion_detail(
                            "domain_error_decode",
                            error,
                        ));
                    }
                };
                match E::decode(&conformed) {
                    Ok(error) => CallError::Domain(error),
                    Err(error) => {
                        CallError::InvalidResponse(conversion_detail("domain_error_decode", error))
                    }
                }
            }
            Self::Deadline => CallError::Deadline,
            Self::Cancelled => CallError::Cancelled,
            Self::Unavailable(detail) => CallError::Unavailable(detail),
            Self::ContractViolation(detail) => CallError::ContractViolation(detail),
            Self::InvalidResponse(detail) => CallError::InvalidResponse(detail),
            Self::Internal(detail) => CallError::Internal(detail),
        }
    }
}

fn conversion_detail(code: &'static str, error: impl fmt::Display) -> Detail {
    Detail::new(code).with_message(error.to_string())
}

impl fmt::Display for ErasedCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain { error_tag, .. } => write!(formatter, "domain error: {error_tag}"),
            Self::Deadline => formatter.write_str("deadline exceeded"),
            Self::Cancelled => formatter.write_str("call cancelled"),
            Self::Unavailable(detail) => write!(formatter, "unavailable: {detail}"),
            Self::ContractViolation(detail) => {
                write!(formatter, "contract violation: {detail}")
            }
            Self::InvalidResponse(detail) => write!(formatter, "invalid response: {detail}"),
            Self::Internal(detail) => write!(formatter, "internal error: {detail}"),
        }
    }
}

impl Error for ErasedCallError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContractValue, OpaquePayload, OpaqueTree};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct DomainWithoutDisplay;

    fn detail() -> Detail {
        Detail::new("diagnostic").with_message("context")
    }

    #[test]
    fn detail_preserves_strings_and_pins_display() {
        let code_only = Detail::new("code");
        assert_eq!(code_only.code(), "code");
        assert_eq!(code_only.message(), None);
        assert_eq!(code_only.to_string(), "code");
        assert_eq!(code_only, code_only.clone());

        let messaged = Detail::new("code").with_message("explanation");
        assert_eq!(messaged.code(), "code");
        assert_eq!(messaged.message(), Some("explanation"));
        assert_eq!(messaged.to_string(), "code: explanation");
        assert_eq!(Detail::new("").to_string(), "");
    }

    #[test]
    fn every_typed_category_is_equal_and_has_stable_display_without_e_display() {
        let cases = [
            (CallError::Domain(DomainWithoutDisplay), "domain error"),
            (CallError::Deadline, "deadline exceeded"),
            (CallError::Cancelled, "call cancelled"),
            (
                CallError::Unavailable(detail()),
                "unavailable: diagnostic: context",
            ),
            (
                CallError::ContractViolation(detail()),
                "contract violation: diagnostic: context",
            ),
            (
                CallError::InvalidResponse(detail()),
                "invalid response: diagnostic: context",
            ),
            (
                CallError::Internal(detail()),
                "internal error: diagnostic: context",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error, error.clone());
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn every_erased_category_is_equal_and_has_stable_display() {
        let cases = [
            (
                ErasedCallError::Domain {
                    error_tag: "not_found".into(),
                    payload: SlotValue::Missing,
                },
                "domain error: not_found",
            ),
            (ErasedCallError::Deadline, "deadline exceeded"),
            (ErasedCallError::Cancelled, "call cancelled"),
            (
                ErasedCallError::Unavailable(detail()),
                "unavailable: diagnostic: context",
            ),
            (
                ErasedCallError::ContractViolation(detail()),
                "contract violation: diagnostic: context",
            ),
            (
                ErasedCallError::InvalidResponse(detail()),
                "invalid response: diagnostic: context",
            ),
            (
                ErasedCallError::Internal(detail()),
                "internal error: diagnostic: context",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error, error.clone());
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn detail_variants_and_redacted_domain_payloads_never_leak() {
        const SENSITIVE_SENTINEL: &str = "sensitive-never-print";
        const OPAQUE_SENTINEL: &str = "opaque-never-print";

        let typed_details: [CallError<()>; 4] = [
            CallError::Unavailable(detail()),
            CallError::ContractViolation(detail()),
            CallError::InvalidResponse(detail()),
            CallError::Internal(detail()),
        ];
        let erased_details = [
            ErasedCallError::Unavailable(detail()),
            ErasedCallError::ContractViolation(detail()),
            ErasedCallError::InvalidResponse(detail()),
            ErasedCallError::Internal(detail()),
        ];
        for output in typed_details
            .iter()
            .map(|error| format!("{error:?} {error}"))
            .chain(
                erased_details
                    .iter()
                    .map(|error| format!("{error:?} {error}")),
            )
        {
            assert!(!output.contains(SENSITIVE_SENTINEL));
            assert!(!output.contains(OPAQUE_SENTINEL));
        }

        let domains = [
            (
                ErasedCallError::Domain {
                    error_tag: "sensitive".into(),
                    payload: SlotValue::Value(ContractValue::sensitive(ContractValue::string(
                        SENSITIVE_SENTINEL,
                    ))),
                },
                "domain error: sensitive",
            ),
            (
                ErasedCallError::Domain {
                    error_tag: "opaque".into(),
                    payload: SlotValue::Value(ContractValue::opaque(OpaquePayload::new(
                        OpaqueTree::String(OPAQUE_SENTINEL.into()),
                    ))),
                },
                "domain error: opaque",
            ),
        ];
        for (error, expected_display) in domains {
            let debug = format!("{error:?}");
            let display = error.to_string();
            assert!(!debug.contains(SENSITIVE_SENTINEL));
            assert!(!debug.contains(OPAQUE_SENTINEL));
            assert!(!display.contains(SENSITIVE_SENTINEL));
            assert!(!display.contains(OPAQUE_SENTINEL));
            assert!(debug.contains("<redacted>"));
            assert_eq!(display, expected_display);
        }
    }

    #[test]
    fn public_errors_have_thread_safe_static_bounds() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        fn assert_error<T: Error>() {}

        assert_bounds::<Detail>();
        assert_bounds::<ErasedCallError>();
        assert_bounds::<CallError<DomainWithoutDisplay>>();
        assert_error::<ErasedCallError>();
        assert_error::<CallError<DomainWithoutDisplay>>();
    }
}
