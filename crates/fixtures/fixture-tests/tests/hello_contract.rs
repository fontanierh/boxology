use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use boxology_contract::{
    CallContext, CallError, Caller, CancelToken, CapabilityId, ContractError, ContractType,
    ContractValue, Deadline, DecodeErrorKind, Detail, ErasedCallError, ErasedCallTarget,
    IdempotencyKey, OpaquePayload, OpaqueTree, PathSegment, SlotValue, TraceContext, ValueRef,
};
use hello_contract::test_support::HelloFake;
use hello_contract::{GreetError, HelloDispatch, HelloHandle};

struct ScriptedTarget {
    response: Result<SlotValue, ErasedCallError>,
    calls: Arc<AtomicUsize>,
    expected_context: Option<CallContext>,
}

impl ErasedCallTarget for ScriptedTarget {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        assert_eq!(capability.to_string(), "hello.greet");
        assert_eq!(input, SlotValue::Value(ContractValue::string("Ada")));
        if let Some(expected) = &self.expected_context {
            assert_eq!(context.caller(), expected.caller());
            assert_eq!(context.deadline(), expected.deadline());
            assert_eq!(
                context.cancellation().is_cancelled(),
                expected.cancellation().is_cancelled()
            );
            assert_eq!(context.trace(), expected.trace());
            assert_eq!(context.idempotency_key(), expected.idempotency_key());
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(ready(self.response.clone()))
    }
}

struct DispatchProbe(&'static str);

impl HelloDispatch for DispatchProbe {
    fn greet<'a>(
        &'a self,
        _context: CallContext,
        name: String,
    ) -> Pin<Box<dyn Future<Output = Result<String, GreetError>> + Send + 'a>> {
        Box::pin(async move { Ok(format!("{}, {name}!", self.0)) })
    }
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

fn scripted(
    response: Result<SlotValue, ErasedCallError>,
    expected_context: Option<CallContext>,
) -> (HelloHandle, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let target: Arc<dyn ErasedCallTarget> = Arc::new(ScriptedTarget {
        response,
        calls: Arc::clone(&calls),
        expected_context,
    });
    (HelloHandle::from_erased(target), calls)
}

fn invoke(response: Result<SlotValue, ErasedCallError>) -> Result<String, CallError<GreetError>> {
    let (handle, calls) = scripted(response, None);
    let output = block_on(handle.greet(context(), "Ada".into()));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    output
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            return output;
        }
    }
}

#[test]
fn generated_dispatch_handle_and_fake_have_exact_thread_safe_shapes() {
    fn assert_bounds<T: Send + Sync + 'static>() {}
    fn assert_clone_default<T: Clone + Default>() {}
    fn assert_send<T: Send>(value: T) -> T {
        value
    }

    assert_bounds::<HelloHandle>();
    assert_bounds::<HelloFake>();
    assert_clone_default::<HelloFake>();
    assert_bounds::<Arc<dyn HelloDispatch>>();
    let dispatch: Arc<dyn HelloDispatch> = Arc::new(DispatchProbe("Hello"));
    let future: Pin<Box<dyn Future<Output = Result<String, GreetError>> + Send + '_>> =
        dispatch.greet(context(), "Ada".into());
    assert_eq!(block_on(assert_send(future)), Ok("Hello, Ada!".into()));

    let fake = HelloFake::new().with_greet(|_, name| async move { Ok(name) });
    let handle = fake.handle();
    assert_eq!(
        block_on(assert_send(handle.greet(context(), "Ada".into()))),
        Ok("Ada".into())
    );
}

#[test]
fn hello_handle_routes_success_and_preserves_the_complete_context() {
    let deadline = Deadline::at(Instant::now() + Duration::from_secs(60));
    let cancellation = CancelToken::new();
    cancellation.cancel();
    let context = CallContext::new(
        Caller::System("fixture-test"),
        Some(deadline),
        cancellation.clone(),
        TraceContext::new(Some("trace-parent".into()), Some("trace-state".into())),
        Some(IdempotencyKey::new("operation-7").unwrap()),
    );
    let expected = context.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    let fake = HelloFake::new().with_greet(move |actual, name| {
        let expected = expected.clone();
        let observed_calls = Arc::clone(&observed_calls);
        async move {
            assert_eq!(name, "Ada");
            assert_eq!(actual.caller(), expected.caller());
            assert_eq!(actual.deadline(), expected.deadline());
            assert_eq!(
                actual.cancellation().is_cancelled(),
                expected.cancellation().is_cancelled()
            );
            assert_eq!(actual.trace(), expected.trace());
            assert_eq!(actual.idempotency_key(), expected.idempotency_key());
            observed_calls.fetch_add(1, Ordering::SeqCst);
            Ok(format!("Hello, {name}!"))
        }
    });
    let handle = fake.handle();

    assert_eq!(
        block_on(handle.greet(context, "Ada".into())),
        Ok("Hello, Ada!".into())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(cancellation.is_cancelled());
}

#[test]
fn hello_fake_round_trips_a_typed_domain_error() {
    let fake = HelloFake::new().with_greet(|_, name| async move {
        assert!(name.is_empty());
        Err(GreetError::EmptyName)
    });
    let handle = fake.handle();

    assert_eq!(
        block_on(handle.greet(context(), String::new())),
        Err(CallError::Domain(GreetError::EmptyName))
    );
}

#[test]
fn hello_fake_reports_the_exact_unprogrammed_capability_code() {
    let handle = HelloFake::new().handle();
    let Err(CallError::Internal(detail)) = block_on(handle.greet(context(), "Ada".into())) else {
        panic!("unprogrammed fake did not return an internal call error")
    };

    assert_eq!(detail.code(), "unprogrammed_capability");
}

#[test]
fn hello_handle_decodes_known_and_future_domain_errors() {
    assert_eq!(
        invoke(Err(ErasedCallError::Domain {
            error_tag: "EmptyName".into(),
            payload: SlotValue::Null,
        })),
        Err(CallError::Domain(GreetError::EmptyName))
    );

    const SECRET: &str = "future-error-secret";
    let raw = ContractValue::object([("detail".into(), ContractValue::string(SECRET))]).unwrap();
    let error = invoke(Err(ErasedCallError::Domain {
        error_tag: "FutureError".into(),
        payload: SlotValue::Value(raw),
    }))
    .unwrap_err();
    let CallError::Domain(GreetError::Unknown { tag, payload }) = &error else {
        panic!("future domain error did not remain a domain error")
    };
    let tree = OpaqueTree::Object(vec![("detail".into(), OpaqueTree::String(SECRET.into()))]);
    assert_eq!(tag, "FutureError");
    assert_eq!(payload.reveal(), &tree);
    assert_eq!(payload.forward().reveal(), &tree);
    let diagnostics = format!("{error:?} {error} {payload:?}");
    assert!(diagnostics.contains("<redacted>"));
    assert!(!diagnostics.contains(SECRET));
}

#[test]
fn hello_handle_rejects_malformed_success_and_known_error_payload() {
    assert!(matches!(
        invoke(Ok(SlotValue::Null)),
        Err(CallError::InvalidResponse(_))
    ));

    let Err(CallError::InvalidResponse(detail)) = invoke(Err(ErasedCallError::Domain {
        error_tag: "EmptyName".into(),
        payload: SlotValue::Value(ContractValue::string("unexpected")),
    })) else {
        panic!("malformed known domain payload was accepted")
    };
    assert_eq!(detail.code(), "domain_error_decode");
}

#[test]
fn hello_handle_preserves_every_non_domain_error() {
    let detail = Detail::new("preserved").with_message("exact");
    let cases: [(ErasedCallError, CallError<GreetError>); 6] = [
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
            CallError::InvalidResponse(detail),
        ),
        (
            ErasedCallError::Internal(Detail::new("panic")),
            CallError::Internal(Detail::new("panic")),
        ),
    ];
    for (erased, expected) in cases {
        assert_eq!(invoke(Err(erased)), Err(expected));
    }
}

#[test]
fn empty_name_has_exact_wire_shape_and_round_trips() {
    let error = GreetError::EmptyName;
    let encoded = error.encode().unwrap();
    let SlotValue::Value(value) = &encoded else {
        panic!("errors must encode as values");
    };
    let ValueRef::Enum { tag, payload } = value.view() else {
        panic!("errors must encode as enums");
    };
    assert_eq!(tag, "EmptyName");
    assert_eq!(payload, &SlotValue::Null);
    assert_eq!(GreetError::decode(&encoded).unwrap(), error);
    assert_eq!(error.error_tag(), "EmptyName");
}

#[test]
fn empty_name_rejects_non_null_payload_at_the_variant_path() {
    let encoded = SlotValue::Value(ContractValue::enum_value(
        "EmptyName",
        SlotValue::Value(ContractValue::string("unexpected")),
    ));
    let error = GreetError::decode(&encoded).unwrap_err();
    assert_eq!(error.kind(), &DecodeErrorKind::UnexpectedPayload);
    assert_eq!(error.path(), &[PathSegment::Variant("EmptyName".into())]);
}

#[test]
fn opaque_unknown_variant_round_trips_forwards_and_redacts() {
    const SECRET: &str = "future-error-detail";
    let tree = OpaqueTree::Object(vec![("detail".into(), OpaqueTree::String(SECRET.into()))]);
    let error = GreetError::Unknown {
        tag: "FutureError".into(),
        payload: OpaquePayload::new(tree.clone()),
    };
    let encoded = error.encode().unwrap();
    let decoded = GreetError::decode(&encoded).unwrap();
    assert_eq!(decoded, error);
    assert_eq!(decoded.encode().unwrap(), encoded);
    let GreetError::Unknown { tag, payload } = &decoded else {
        panic!("unknown tag must remain unknown");
    };
    assert_eq!(tag, "FutureError");
    assert_eq!(payload.reveal(), &tree);
    assert_eq!(payload.forward().reveal(), &tree);
    let debug = format!("{decoded:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains(SECRET));
}

#[test]
fn raw_unknown_variant_is_rejected_at_the_variant_path() {
    let encoded = SlotValue::Value(ContractValue::enum_value(
        "FutureError",
        SlotValue::Value(ContractValue::string("not conformed")),
    ));
    let error = GreetError::decode(&encoded).unwrap_err();
    assert_eq!(
        error.kind(),
        &DecodeErrorKind::UnknownVariant("FutureError".into())
    );
    assert_eq!(error.path(), &[PathSegment::Variant("FutureError".into())]);
}

#[test]
fn generated_contract_has_no_runtime_or_http_dependency() {
    let manifest = include_str!("../../hello/generated/contract/Cargo.toml");
    let dependencies = manifest
        .split_once("[dependencies]\n")
        .expect("generated contract must declare dependencies")
        .1
        .trim();
    assert_eq!(dependencies, "boxology-contract = { workspace = true }");
    assert!(!dependencies.contains("runtime"));
    assert!(!dependencies.contains("http"));
}
