// @PROVENANCE@
use boxology_contract::{
    ContractError, ContractType, ContractValue, DecodeError, DecodeErrorKind, EncodeError,
    OpaquePayload, PathSegment, SlotValue, ValueRef,
};

#[derive(Debug, Clone, PartialEq)]
pub enum GreetError {
    EmptyName,
    Unknown { tag: String, payload: OpaquePayload },
}

impl ContractType for GreetError {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        let (tag, payload) = match self {
            Self::EmptyName => ("EmptyName".into(), SlotValue::Null),
            Self::Unknown { tag, payload } => (
                tag.clone(),
                SlotValue::Value(ContractValue::opaque(payload.forward())),
            ),
        };
        Ok(ContractValue::enum_value(tag, payload))
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        let ValueRef::Enum { tag, payload } = value.view() else {
            return Err(DecodeError::new(DecodeErrorKind::KindMismatch));
        };
        match tag {
            "EmptyName" if matches!(payload, SlotValue::Null) => Ok(Self::EmptyName),
            "EmptyName" => Err(DecodeError::new(DecodeErrorKind::UnexpectedPayload)
                .under(PathSegment::Variant(tag.into()))),
            _ => match payload {
                SlotValue::Value(value) => match value.view() {
                    ValueRef::Opaque(payload) => Ok(Self::Unknown {
                        tag: tag.into(),
                        payload: payload.forward(),
                    }),
                    _ => Err(
                        DecodeError::new(DecodeErrorKind::UnknownVariant(tag.into()))
                            .under(PathSegment::Variant(tag.into())),
                    ),
                },
                _ => Err(
                    DecodeError::new(DecodeErrorKind::UnknownVariant(tag.into()))
                        .under(PathSegment::Variant(tag.into())),
                ),
            },
        }
    }
}

impl ContractError for GreetError {
    fn error_tag(&self) -> &str {
        match self {
            Self::EmptyName => "EmptyName",
            Self::Unknown { tag, .. } => tag,
        }
    }
}
