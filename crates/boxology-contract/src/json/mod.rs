//! Canonical, binding-independent JSON projection for contract values.
//!
//! JSON is descriptor-guided: the descriptor selects integer widths, blob
//! envelopes, enum shapes, presence rules, and sensitive subtrees. The same
//! codec is used by Boxology's HTTP binding, so IPC, CLI, and file consumers
//! do not need to invent a second mapping.

mod encode;
mod semantic;
mod syntax;

use std::{error::Error, fmt};

pub use encode::{EncodeError, EncodeErrorKind, encode};
pub use semantic::{SemanticError, SemanticErrorKind, decode_tree};
pub use syntax::{DEFAULT_DEPTH_LIMIT, Limits, SyntaxError, parse};

use crate::{DecodeRole, SlotValue, TypeDescriptor};

/// A failure to parse or semantically project JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// JSON bytes failed syntax validation or a resource cap.
    Syntax(SyntaxError),
    /// Valid JSON did not conform to the descriptor and role.
    Semantic(SemanticError),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => error.fmt(formatter),
            Self::Semantic(error) => error.fmt(formatter),
        }
    }
}

impl Error for DecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Syntax(error) => Some(error),
            Self::Semantic(error) => Some(error),
        }
    }
}

/// Decodes JSON bytes into a descriptor-conformed contract slot.
///
/// `role` preserves Boxology's strict provider-input and tolerant
/// consumer-output rules. `limits` are checked before and during parsing.
pub fn decode(
    input: &[u8],
    descriptor: &TypeDescriptor,
    role: DecodeRole,
    limits: Limits,
) -> Result<SlotValue, DecodeError> {
    let tree = parse(input, limits).map_err(DecodeError::Syntax)?;
    decode_tree(tree, descriptor, role).map_err(DecodeError::Semantic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContractValue as V, OpaquePayload, OpaqueTree, VariantDescriptor, VariantPayload};

    fn limits(bytes: &[u8]) -> Limits {
        Limits::new(bytes.len(), DEFAULT_DEPTH_LIMIT)
    }

    #[test]
    fn public_codec_preserves_unknown_enum_payloads_and_rejects_invalid_inputs() {
        let descriptor = TypeDescriptor::enumeration([VariantDescriptor::new(
            "Known",
            VariantPayload::Unit,
            None,
        )])
        .unwrap();
        let raw = br#"{"tag":"Future","payload":{"z":1,"a":true}}"#;
        let decoded = decode(raw, &descriptor, DecodeRole::ConsumerOutput, limits(raw)).unwrap();
        assert_eq!(encode(&decoded, &descriptor).unwrap(), raw);

        assert_eq!(
            encode(&SlotValue::Missing, &TypeDescriptor::bool())
                .unwrap_err()
                .kind(),
            EncodeErrorKind::MissingValue
        );
        let Err(DecodeError::Semantic(error)) = decode(
            b"true",
            &TypeDescriptor::string(),
            DecodeRole::ProviderInput,
            Limits::new(4, 1),
        ) else {
            panic!("wrong representations must be rejected")
        };
        assert_eq!(error.kind(), SemanticErrorKind::RepresentationMismatch);
        assert!(matches!(
            decode(
                b"[[]]",
                &TypeDescriptor::list(TypeDescriptor::list(TypeDescriptor::bool()).unwrap())
                    .unwrap(),
                DecodeRole::ProviderInput,
                Limits::new(4, 1)
            ),
            Err(DecodeError::Syntax(SyntaxError::DepthLimitExceeded {
                limit: 1
            }))
        ));

        let opaque = SlotValue::Value(V::opaque(OpaquePayload::new(OpaqueTree::Null)));
        assert_eq!(
            encode(&opaque, &TypeDescriptor::bool()).unwrap_err().kind(),
            EncodeErrorKind::RepresentationMismatch
        );

        let descriptor =
            TypeDescriptor::secret(TypeDescriptor::optional(TypeDescriptor::string()).unwrap())
                .unwrap();
        let decoded = decode(
            b"null",
            &descriptor,
            DecodeRole::ProviderInput,
            Limits::new(4, 1),
        )
        .unwrap();
        assert_eq!(decoded, SlotValue::Value(V::sensitive(V::null())));
        assert_eq!(encode(&decoded, &descriptor).unwrap(), b"null");
    }
}
