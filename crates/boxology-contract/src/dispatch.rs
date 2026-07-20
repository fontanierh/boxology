//! Object-safe erased invocation.

use std::any::Any;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::{CallContext, CapabilityId, Detail, ErasedCallError, SlotValue};

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

/// Invokes erased dispatch without allowing a handler panic to cross the boundary.
#[doc(hidden)]
pub fn call_guarded<'a>(
    target: &'a dyn ErasedTarget,
    capability: &'a CapabilityId,
    ctx: CallContext,
    input: SlotValue,
) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
    match catch_unwind(AssertUnwindSafe(|| target.call(capability, ctx, input))) {
        Ok(future) => Box::pin(PanicGuard { future }),
        Err(payload) => Box::pin(std::future::ready(Err(panic_error(payload.as_ref())))),
    }
}

struct PanicGuard<'a> {
    future: Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>,
}

impl Future for PanicGuard<'_> {
    type Output = Result<SlotValue, ErasedCallError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        catch_unwind(AssertUnwindSafe(|| self.future.as_mut().poll(context)))
            .unwrap_or_else(|payload| Poll::Ready(Err(panic_error(payload.as_ref()))))
    }
}

fn panic_error(payload: &(dyn Any + Send)) -> ErasedCallError {
    let detail = if let Some(message) = payload.downcast_ref::<&str>() {
        Detail::new("panic").with_message(*message)
    } else if let Some(message) = payload.downcast_ref::<String>() {
        Detail::new("panic").with_message(message.clone())
    } else {
        Detail::new("panic")
    };
    ErasedCallError::Internal(detail)
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

    struct FixedTarget(Result<SlotValue, ErasedCallError>);

    impl ErasedTarget for FixedTarget {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            _ctx: CallContext,
            _input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            Box::pin(std::future::ready(self.0.clone()))
        }
    }

    struct BorrowingTarget(&'static str);

    impl ErasedTarget for BorrowingTarget {
        fn call<'a>(
            &'a self,
            capability: &'a CapabilityId,
            _ctx: CallContext,
            input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            Box::pin(async move {
                assert_eq!(self.0, capability.name().as_str());
                Ok(input)
            })
        }
    }

    #[derive(Clone, Copy)]
    enum PanicPayload {
        StaticStr,
        OwnedString,
        NonString,
    }

    fn raise(payload: PanicPayload) -> ! {
        match payload {
            PanicPayload::StaticStr => std::panic::panic_any("borrowed panic"),
            PanicPayload::OwnedString => std::panic::panic_any(String::from("owned panic")),
            PanicPayload::NonString => std::panic::panic_any(17_u8),
        }
    }

    struct ConstructionPanic(PanicPayload);

    impl ErasedTarget for ConstructionPanic {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            _ctx: CallContext,
            _input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            raise(self.0)
        }
    }

    struct PollPanic(PanicPayload);

    impl ErasedTarget for PollPanic {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            _ctx: CallContext,
            _input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            Box::pin(PendingThenPanic {
                pending_polls: 2,
                payload: self.0,
            })
        }
    }

    struct PendingThenPanic {
        pending_polls: u8,
        payload: PanicPayload,
    }

    impl Future for PendingThenPanic {
        type Output = Result<SlotValue, ErasedCallError>;

        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            if self.pending_polls > 0 {
                self.pending_polls -= 1;
                context.waker().wake_by_ref();
                Poll::Pending
            } else {
                raise(self.payload)
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

    fn assert_send<T: Send>(value: T) -> T {
        value
    }

    fn invoke(
        target: &dyn ErasedTarget,
        input: SlotValue,
    ) -> Poll<Result<SlotValue, ErasedCallError>> {
        let capability = capability();
        let mut future = target.call(&capability, context(), input);
        poll_once(future.as_mut())
    }

    fn invoke_guarded(
        target: &dyn ErasedTarget,
        input: SlotValue,
    ) -> Poll<Result<SlotValue, ErasedCallError>> {
        let capability = capability();
        let mut future = call_guarded(target, &capability, context(), input);
        poll_once(future.as_mut())
    }

    fn internal_detail(output: Poll<Result<SlotValue, ErasedCallError>>) -> Detail {
        let Poll::Ready(Err(ErasedCallError::Internal(detail))) = output else {
            panic!("expected internal error, got {output:?}")
        };
        detail
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

    #[test]
    fn guarded_call_preserves_borrowed_success_and_is_send() {
        let target = BorrowingTarget("call");
        let capability = capability();
        let input = SlotValue::Value(ContractValue::string("success"));
        let mut future = assert_send(call_guarded(&target, &capability, context(), input.clone()));
        assert_eq!(poll_once(future.as_mut()), Poll::Ready(Ok(input)));
    }

    #[test]
    fn guarded_call_preserves_every_ordinary_error() {
        let detail = Detail::new("ordinary").with_message("unchanged");
        let errors = [
            ErasedCallError::Domain {
                error_tag: "known".into(),
                payload: SlotValue::Null,
            },
            ErasedCallError::Deadline,
            ErasedCallError::Cancelled,
            ErasedCallError::Unavailable(detail.clone()),
            ErasedCallError::ContractViolation(detail.clone()),
            ErasedCallError::InvalidResponse(detail.clone()),
            ErasedCallError::Internal(detail),
        ];
        for expected in errors {
            assert_eq!(
                invoke_guarded(&FixedTarget(Err(expected.clone())), SlotValue::Missing),
                Poll::Ready(Err(expected))
            );
        }
    }

    #[test]
    fn construction_panics_preserve_static_messages_and_hide_non_strings() {
        let message = internal_detail(invoke_guarded(
            &ConstructionPanic(PanicPayload::StaticStr),
            SlotValue::Missing,
        ));
        assert_eq!(message, Detail::new("panic").with_message("borrowed panic"));

        let code_only = internal_detail(invoke_guarded(
            &ConstructionPanic(PanicPayload::NonString),
            SlotValue::Missing,
        ));
        assert_eq!(code_only, Detail::new("panic"));
    }

    #[test]
    fn every_poll_is_guarded_and_owned_string_messages_are_preserved() {
        let target = PollPanic(PanicPayload::OwnedString);
        let capability = capability();
        let mut future = assert_send(call_guarded(
            &target,
            &capability,
            context(),
            SlotValue::Missing,
        ));
        assert_eq!(poll_once(future.as_mut()), Poll::Pending);
        assert_eq!(poll_once(future.as_mut()), Poll::Pending);
        let detail = internal_detail(poll_once(future.as_mut()));
        assert_eq!(detail, Detail::new("panic").with_message("owned panic"));
    }
}
