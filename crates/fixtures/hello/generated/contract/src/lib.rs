// @PROVENANCE@
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};

use boxology_contract::{
    BoxId, CallContext, CallError, CapabilityId, CapabilityName, ContractError, ContractType,
    ContractValue, DecodeError, DecodeErrorKind, DecodeRole, Detail, EncodeError, ErasedCallTarget,
    OpaquePayload, PathSegment, SlotValue, TypeDescriptor, ValueRef, VariantDescriptor,
    VariantPayload,
};

pub trait HelloDispatch: Send + Sync + 'static {
    fn greet<'a>(
        &'a self,
        context: CallContext,
        name: String,
    ) -> Pin<Box<dyn Future<Output = Result<String, GreetError>> + Send + 'a>>;
}

#[derive(Clone)]
pub struct HelloHandle {
    target: Arc<dyn ErasedCallTarget>,
}

impl HelloHandle {
    #[doc(hidden)]
    pub fn from_erased(target: Arc<dyn ErasedCallTarget>) -> Self {
        Self { target }
    }

    pub async fn greet(
        &self,
        context: CallContext,
        name: String,
    ) -> Result<String, CallError<GreetError>> {
        let input = name
            .encode()
            .map_err(|error| conversion_detail("input_encode", error))
            .map_err(CallError::ContractViolation)?;
        let output = self
            .target
            .call(&HELLO_GREET, context, input)
            .await
            .map_err(|error| error.into_typed::<GreetError>(&GREET_ERROR_DESCRIPTOR))?;
        let output = TypeDescriptor::string()
            .conform(DecodeRole::ConsumerOutput, output)
            .map_err(|error| conversion_detail("output_decode", error))
            .map_err(CallError::InvalidResponse)?;
        String::decode(&output)
            .map_err(|error| conversion_detail("output_decode", error))
            .map_err(CallError::InvalidResponse)
    }
}

static HELLO_GREET: LazyLock<CapabilityId> = LazyLock::new(|| {
    CapabilityId::new(
        BoxId::new("hello").expect("generated hello box identity is valid"),
        CapabilityName::new("greet").expect("generated greet capability name is valid"),
    )
});

static GREET_ERROR_DESCRIPTOR: LazyLock<TypeDescriptor> = LazyLock::new(|| {
    TypeDescriptor::enumeration([VariantDescriptor::new(
        "EmptyName",
        VariantPayload::Unit,
        None,
    )])
    .expect("generated greet error descriptor is valid")
});

fn conversion_detail(code: &'static str, error: impl std::fmt::Display) -> Detail {
    Detail::new(code).with_message(error.to_string())
}

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
