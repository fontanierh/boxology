//! Object-safe erased invocation.

use std::future::Future;
use std::pin::Pin;

use crate::{CallContext, CapabilityId, ErasedCallError, SlotValue};

/// Generated value-level dispatch implemented by receiver adapters.
///
/// Receivers are `Send + Sync`; returned futures are `Send` and may borrow the
/// receiver and capability for the call lifetime. Typed callers normally use
/// generated handles rather than this doc-hidden value-level ABI.
#[doc(hidden)]
pub trait ErasedTarget: Send + Sync {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        ctx: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};

    use super::*;
    use crate::{
        BoxId, CallError, Caller, CancelToken, CapabilityName, ContractError, ContractType,
        ContractValue, DecodeError, DecodeErrorKind, Detail, EncodeError, OpaquePayload,
        OpaqueTree, TraceContext, TypeDescriptor, ValueRef, VariantDescriptor, VariantPayload,
    };

    #[derive(Debug, Clone, PartialEq)]
    enum GeneratedError {
        Known(String),
        Unit,
        Float(f32),
        Unknown { tag: String, payload: OpaquePayload },
    }

    impl ContractType for GeneratedError {
        fn encode_value(&self) -> Result<ContractValue, EncodeError> {
            let (tag, payload) = match self {
                Self::Known(value) => ("known".into(), value.encode()?),
                Self::Unit => ("unit".into(), SlotValue::Null),
                Self::Float(value) => ("float".into(), value.encode()?),
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
                "known" => String::decode(payload).map(Self::Known),
                "unit" if matches!(payload, SlotValue::Null) => Ok(Self::Unit),
                "unit" => Err(DecodeError::new(DecodeErrorKind::UnexpectedPayload)),
                "float" => f32::decode(payload).map(Self::Float),
                _ => match payload {
                    SlotValue::Value(value) => match value.view() {
                        ValueRef::Opaque(payload) => Ok(Self::Unknown {
                            tag: tag.into(),
                            payload: payload.forward(),
                        }),
                        _ => Err(DecodeError::new(DecodeErrorKind::UnknownVariant(
                            tag.into(),
                        ))),
                    },
                    _ => Err(DecodeError::new(DecodeErrorKind::UnknownVariant(
                        tag.into(),
                    ))),
                },
            }
        }
    }

    impl ContractError for GeneratedError {
        fn error_tag(&self) -> &str {
            match self {
                Self::Known(_) => "known",
                Self::Unit => "unit",
                Self::Float(_) => "float",
                Self::Unknown { tag, .. } => tag,
            }
        }
    }

    #[derive(Debug)]
    struct NonEnumError;

    impl ContractType for NonEnumError {
        fn encode_value(&self) -> Result<ContractValue, EncodeError> {
            Ok(ContractValue::string("not-an-enum"))
        }

        fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
            String::decode_value(value).map(|_| Self)
        }
    }

    impl ContractError for NonEnumError {
        fn error_tag(&self) -> &str {
            "not-an-enum"
        }
    }

    fn error_descriptor() -> TypeDescriptor {
        TypeDescriptor::enumeration([
            VariantDescriptor::new(
                "known",
                VariantPayload::Value(TypeDescriptor::string()),
                None,
            ),
            VariantDescriptor::new("unit", VariantPayload::Unit, None),
            VariantDescriptor::new("float", VariantPayload::Value(TypeDescriptor::f32()), None),
        ])
        .unwrap()
    }

    #[derive(Clone, Copy)]
    enum Behavior {
        Echo,
        Domain(&'static str),
    }

    struct Target(Behavior);

    impl ErasedTarget for Target {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            _ctx: CallContext,
            input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            match self.0 {
                Behavior::Echo => Box::pin(std::future::ready(Ok(input))),
                Behavior::Domain(error_tag) => {
                    Box::pin(std::future::ready(Err(ErasedCallError::Domain {
                        error_tag: error_tag.into(),
                        payload: input,
                    })))
                }
            }
        }
    }

    fn capability() -> CapabilityId {
        CapabilityId::new(
            BoxId::new("test").unwrap(),
            CapabilityName::new("call").unwrap(),
        )
    }

    fn context() -> CallContext {
        CallContext::new(
            Caller::Anonymous,
            None,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        )
    }

    fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    fn invoke(
        target: &dyn ErasedTarget,
        input: SlotValue,
    ) -> Poll<Result<SlotValue, ErasedCallError>> {
        let capability = capability();
        let mut future = target.call(&capability, context(), input);
        poll_once(future.as_mut())
    }

    fn invalid_detail(error: CallError<GeneratedError>) -> Detail {
        let CallError::InvalidResponse(detail) = error else {
            panic!("expected InvalidResponse")
        };
        detail
    }

    #[test]
    fn known_and_unit_domain_errors_round_trip_exactly() {
        let descriptor = error_descriptor();
        let known = GeneratedError::Known("payload".into());
        let known_erased = ErasedCallError::from_domain(&known);
        assert_eq!(
            known_erased,
            ErasedCallError::Domain {
                error_tag: "known".into(),
                payload: "payload".to_string().encode().unwrap(),
            }
        );
        assert_eq!(
            known_erased.into_typed(&descriptor),
            CallError::Domain(known)
        );

        let unit_erased = ErasedCallError::from_domain(&GeneratedError::Unit);
        assert_eq!(
            unit_erased,
            ErasedCallError::Domain {
                error_tag: "unit".into(),
                payload: SlotValue::Null,
            }
        );
        assert_eq!(
            unit_erased.into_typed(&descriptor),
            CallError::Domain(GeneratedError::Unit)
        );
    }

    #[test]
    fn dyn_dispatch_preserves_success_and_unknown_domain_opacity() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        fn assert_send<T: Send>(value: T) -> T {
            value
        }

        let target: Arc<dyn ErasedTarget> = Arc::new(Target(Behavior::Echo));
        assert_bounds::<Arc<dyn ErasedTarget>>();
        let capability = capability();
        let input = SlotValue::Value(ContractValue::u64(7));
        let mut future = assert_send(target.call(&capability, context(), input.clone()));
        assert_eq!(poll_once(future.as_mut()), Poll::Ready(Ok(input)));

        const SENTINEL: &str = "unknown-domain-sentinel";
        let raw =
            ContractValue::object([("marker".into(), ContractValue::string(SENTINEL))]).unwrap();
        let Poll::Ready(Err(erased)) =
            invoke(&Target(Behavior::Domain("future")), SlotValue::Value(raw))
        else {
            panic!()
        };
        let typed = erased.into_typed::<GeneratedError>(&error_descriptor());
        let CallError::Domain(GeneratedError::Unknown { tag, payload }) = &typed else {
            panic!()
        };
        assert_eq!(tag, "future");
        assert_eq!(
            payload.reveal(),
            &OpaqueTree::Object(vec![("marker".into(), OpaqueTree::String(SENTINEL.into()))])
        );
        let diagnostics = format!("{typed:?} {typed} {payload:?}");
        assert!(!diagnostics.contains(SENTINEL));
        assert!(diagnostics.contains("<redacted>"));

        let missing = ErasedCallError::Domain {
            error_tag: "future_unit".into(),
            payload: SlotValue::Missing,
        }
        .into_typed::<GeneratedError>(&error_descriptor());
        let CallError::Domain(GeneratedError::Unknown { payload, .. }) = missing else {
            panic!()
        };
        assert_eq!(payload.reveal(), &OpaqueTree::Null);
    }

    #[test]
    fn conversion_failures_have_stable_payload_free_details() {
        const PAYLOAD_SENTINEL: &str = "18446744073709551615";
        let malformed = ErasedCallError::Domain {
            error_tag: "known".into(),
            payload: SlotValue::Value(ContractValue::u64(u64::MAX)),
        };
        let detail = invalid_detail(malformed.into_typed(&error_descriptor()));
        assert_eq!(detail.code(), "domain_error_decode");
        assert!(!detail.message().unwrap().contains(PAYLOAD_SENTINEL));

        let ErasedCallError::InvalidResponse(detail) =
            ErasedCallError::from_domain(&GeneratedError::Float(f32::NAN))
        else {
            panic!()
        };
        assert_eq!(detail.code(), "domain_error_encode");
        assert!(!detail.message().unwrap().contains("NaN"));
        assert_eq!(
            ErasedCallError::from_domain(&NonEnumError),
            ErasedCallError::InvalidResponse(Detail::new("domain_error_shape"))
        );
    }

    #[test]
    fn non_domain_categories_map_one_to_one() {
        let detail = Detail::new("preserved").with_message("exact");
        let cases = [
            (ErasedCallError::Deadline, CallError::Deadline),
            (ErasedCallError::Cancelled, CallError::Cancelled),
            (
                ErasedCallError::Unavailable(detail.clone()),
                CallError::Unavailable(detail.clone()),
            ),
            (
                ErasedCallError::ContractViolation(detail.clone()),
                CallError::ContractViolation(detail.clone()),
            ),
            (
                ErasedCallError::InvalidResponse(detail.clone()),
                CallError::InvalidResponse(detail.clone()),
            ),
            (
                ErasedCallError::Internal(detail.clone()),
                CallError::Internal(detail),
            ),
        ];
        for (erased, typed) in cases {
            assert_eq!(
                erased.into_typed::<GeneratedError>(&error_descriptor()),
                typed
            );
        }
    }
}
