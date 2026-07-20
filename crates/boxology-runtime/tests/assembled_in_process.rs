use std::future::{Future, poll_fn, ready};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use boxology_contract::{
    BoxId, CallContext, CallError, Caller, CancelToken, CapabilityDescriptor, CapabilityId,
    CapabilityName, CapabilityShape, ContractDescriptor, ContractError, ContractRevision,
    ContractType, ContractValue, Deadline, DecodeError, DecodeErrorKind, Detail, EncodeError,
    ErasedCallError, ErasedTarget, ExposureLevel, Idempotency, ImplementationDescriptor,
    ImportDescriptor, SlotValue, TraceContext, TypeDescriptor, VariantDescriptor, VariantPayload,
};
use boxology_runtime::{Composition, CompositionBuilder, ImportHandle, ImportTarget, Imports};

type ErasedFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>;

const DOMAIN_TAG: &str = "failing_domain";
const ORDER: Ordering = Ordering::SeqCst;

#[derive(Debug, Clone, PartialEq, Eq)]
struct FailingDomain;

type TypedResult = Result<f32, CallError<FailingDomain>>;

impl ContractType for FailingDomain {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        let payload = f32::NAN.encode()?;
        Ok(ContractValue::enum_value(self.error_tag(), payload))
    }

    fn decode_value(_value: &ContractValue) -> Result<Self, DecodeError> {
        Err(DecodeError::new(DecodeErrorKind::KindMismatch))
    }
}

impl ContractError for FailingDomain {
    fn error_tag(&self) -> &str {
        DOMAIN_TAG
    }
}

#[derive(Clone)]
struct GeneratedHandle {
    import: ImportHandle,
    capability: CapabilityId,
}

impl GeneratedHandle {
    async fn call(&self, context: CallContext, input: f32) -> TypedResult {
        let input = input
            .encode()
            .map_err(|error| CallError::ContractViolation(conversion_detail(error)))?;
        match self.import.call(&self.capability, context, input).await {
            Ok(output) => f32::decode(&output)
                .map_err(|error| CallError::InvalidResponse(conversion_detail(error))),
            Err(error) => Err(error.into_typed(error_descriptor())),
        }
    }
}

struct GeneratedAdapter {
    capability: CapabilityId,
    service: Service,
}

impl ErasedTarget for GeneratedAdapter {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> ErasedFuture<'a> {
        self.service.state.target_calls.fetch_add(1, ORDER);
        assert_eq!(capability, &self.capability);
        let input = match f32::decode(&input) {
            Ok(input) => input,
            Err(error) => {
                return Box::pin(ready(Err(ErasedCallError::ContractViolation(
                    conversion_detail(error),
                ))));
            }
        };
        Box::pin(async move {
            match self.service.call(context, input).await {
                Ok(output) => output
                    .encode()
                    .map_err(|error| ErasedCallError::InvalidResponse(conversion_detail(error))),
                Err(error) => Err(ErasedCallError::from_domain(&error)),
            }
        })
    }
}

struct InertConsumer;

impl ErasedTarget for InertConsumer {
    fn call<'a>(
        &'a self,
        _capability: &'a CapabilityId,
        _context: CallContext,
        _input: SlotValue,
    ) -> ErasedFuture<'a> {
        panic!("inert consumer has no capabilities")
    }
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Echo,
    InvalidOutput,
    DomainError,
    Panic,
    ObserveCancellation,
    WaitPastDeadline(ThreadId),
}

struct State {
    mode: Mutex<Mode>,
    target_calls: AtomicUsize,
    calls: AtomicUsize,
    polls: AtomicUsize,
    cancellation_seen: AtomicBool,
}

impl State {
    fn new() -> Self {
        Self {
            mode: Mutex::new(Mode::Echo),
            target_calls: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            polls: AtomicUsize::new(0),
            cancellation_seen: AtomicBool::new(false),
        }
    }

    fn prepare(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
        self.target_calls.store(0, ORDER);
        self.calls.store(0, ORDER);
        self.polls.store(0, ORDER);
        self.cancellation_seen.store(false, ORDER);
    }

    fn counts(&self) -> (usize, usize) {
        (self.target_calls.load(ORDER), self.calls.load(ORDER))
    }
}

struct Service {
    state: Arc<State>,
}

impl Service {
    async fn call(&self, context: CallContext, input: f32) -> Result<f32, FailingDomain> {
        self.state.calls.fetch_add(1, ORDER);
        let mode = *self.state.mode.lock().unwrap();
        match mode {
            Mode::Echo => Ok(input),
            Mode::InvalidOutput => Ok(f32::NAN),
            Mode::DomainError => Err(FailingDomain),
            Mode::Panic => panic!("provider poll panic"),
            Mode::ObserveCancellation => {
                let cancelled = context.cancellation().is_cancelled();
                self.state.cancellation_seen.store(cancelled, ORDER);
                Ok(input)
            }
            Mode::WaitPastDeadline(calling_thread) => {
                let deadline = context.deadline().expect("test supplies a deadline");
                poll_fn(|context| {
                    assert_eq!(thread::current().id(), calling_thread);
                    self.state.polls.fetch_add(1, ORDER);
                    if deadline.remaining().is_zero() {
                        Poll::Ready(Ok(input))
                    } else {
                        context.waker().wake_by_ref();
                        Poll::Pending
                    }
                })
                .await
            }
        }
    }
}

type Unstarted = (CompositionBuilder, GeneratedHandle, Arc<State>);

struct Assembled {
    handle: GeneratedHandle,
    state: Arc<State>,
    _composition: Composition,
}

fn build() -> Unstarted {
    let state = Arc::new(State::new());
    let provider = box_id("provider");
    let capability = capability();
    let mut captured = None;
    let mut builder = CompositionBuilder::new();
    let consumer = implementation("consumer", false, true);
    builder.add_box(consumer, |imports: Imports| {
        captured = Some(GeneratedHandle {
            import: imports.handle(&provider).unwrap().clone(),
            capability: capability.clone(),
        });
        InertConsumer
    });
    assert!(captured.is_some(), "consumer factory did not run inline");
    builder.add_box(implementation("provider", true, false), |_| {
        GeneratedAdapter {
            capability: capability.clone(),
            service: Service {
                state: Arc::clone(&state),
            },
        }
    });
    builder.resolve_import(
        box_id("consumer"),
        provider.clone(),
        ImportTarget::local(provider),
    );
    (builder, captured.unwrap(), state)
}

fn assemble() -> Assembled {
    let (builder, handle, state) = build();
    Assembled {
        handle,
        state,
        _composition: builder.start().unwrap(),
    }
}

fn implementation(box_name: &str, provides: bool, imports: bool) -> ImplementationDescriptor {
    let revision = ContractRevision::new("r1").unwrap();
    let capabilities = provides.then(|| {
        CapabilityDescriptor::new(
            capability(),
            TypeDescriptor::f32(),
            TypeDescriptor::f32(),
            error_descriptor().clone(),
            CapabilityShape::Unary,
            ExposureLevel::CodeOnly,
            Idempotency::None,
            None,
        )
    });
    let contract = Box::leak(Box::new(
        ContractDescriptor::new(box_id(box_name), capabilities, revision.clone()).unwrap(),
    ));
    let imports = imports
        .then(|| ImportDescriptor::new(box_id("provider"), revision, [capability()]).unwrap());
    ImplementationDescriptor::new(contract, imports).unwrap()
}

static ERROR_DESCRIPTOR: LazyLock<TypeDescriptor> = LazyLock::new(|| {
    TypeDescriptor::enumeration([VariantDescriptor::new(
        DOMAIN_TAG,
        VariantPayload::Value(TypeDescriptor::f32()),
        None,
    )])
    .unwrap()
});

fn error_descriptor() -> &'static TypeDescriptor {
    &ERROR_DESCRIPTOR
}

fn box_id(value: &str) -> BoxId {
    BoxId::new(value).unwrap()
}

fn capability() -> CapabilityId {
    CapabilityId::new(box_id("provider"), CapabilityName::new("compute").unwrap())
}

fn conversion_detail(error: impl std::fmt::Display) -> Detail {
    Detail::new("test_conversion").with_message(error.to_string())
}

fn context(deadline: Option<Deadline>, cancellation: CancelToken) -> CallContext {
    let trace = TraceContext::empty();
    CallContext::new(Caller::Anonymous, deadline, cancellation, trace, None)
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

fn invoke(handle: &GeneratedHandle, deadline: Option<Deadline>, input: f32) -> TypedResult {
    block_on(handle.call(context(deadline, CancelToken::new()), input))
}

fn assert_send<T: Send>(value: T) -> T {
    value
}

fn assert_send_sync_static<T: Send + Sync + 'static>() {}

#[test]
fn start_is_the_only_authorization_and_typed_success_selects_the_provider() {
    assert_send_sync_static::<GeneratedAdapter>();
    let (builder, handle, state) = build();
    let before_start = block_on(assert_send(
        handle.call(context(None, CancelToken::new()), 7.25),
    ));
    assert_eq!(
        before_start,
        Err(CallError::Unavailable(Detail::new("unsealed_import")))
    );
    assert_eq!(state.counts(), (0, 0));

    let assembled = Assembled {
        handle,
        state,
        _composition: builder.start().unwrap(),
    };
    assert_eq!(invoke(&assembled.handle, None, 7.25), Ok(7.25));
    assert_eq!(assembled.state.counts(), (1, 1));
}

#[test]
fn violations_deadlines_invalid_results_domain_errors_and_panics_are_ordered() {
    let assembled = assemble();

    assert!(matches!(
        invoke(&assembled.handle, None, f32::NAN),
        Err(CallError::ContractViolation(_))
    ));
    assert_eq!(assembled.state.counts(), (0, 0));

    let expired = Deadline::at(Instant::now());
    assert!(expired.remaining().is_zero());
    let expired_result = invoke(&assembled.handle, Some(expired), 1.0);
    assert_eq!(expired_result, Err(CallError::Deadline));
    assert_eq!(assembled.state.counts(), (0, 0));

    assembled.state.prepare(Mode::InvalidOutput);
    assert!(matches!(
        invoke(&assembled.handle, None, 2.0),
        Err(CallError::InvalidResponse(_))
    ));
    assert_eq!(assembled.state.counts(), (1, 1));

    assembled.state.prepare(Mode::DomainError);
    assert!(matches!(
        invoke(&assembled.handle, None, 3.0),
        Err(CallError::InvalidResponse(detail)) if detail.code() == "domain_error_encode"
    ));
    assert_eq!(assembled.state.counts(), (1, 1));

    assembled.state.prepare(Mode::Panic);
    assert!(matches!(
        invoke(&assembled.handle, None, 4.0),
        Err(CallError::Internal(detail)) if detail.code() == "panic"
    ));
    assert_eq!(assembled.state.counts(), (1, 1));
}

#[test]
fn cancellation_is_advisory_and_observable_by_the_provider() {
    let assembled = assemble();
    assembled.state.prepare(Mode::ObserveCancellation);
    let cancellation = CancelToken::new();
    cancellation.cancel();

    let output = block_on(assembled.handle.call(context(None, cancellation), 5.0));
    assert_eq!(output, Ok(5.0));
    assert!(assembled.state.cancellation_seen.load(ORDER));
    assert_eq!(assembled.state.counts(), (1, 1));
}

#[test]
fn provider_polling_is_inline_and_a_positive_deadline_is_not_a_mid_call_timer() {
    let assembled = assemble();
    assembled
        .state
        .prepare(Mode::WaitPastDeadline(thread::current().id()));
    let deadline = Deadline::at(Instant::now() + Duration::from_millis(20));
    assert!(!deadline.remaining().is_zero());

    assert_eq!(invoke(&assembled.handle, Some(deadline), 6.0), Ok(6.0));
    assert!(deadline.remaining().is_zero());
    assert!(assembled.state.polls.load(ORDER) > 1);
    assert_eq!(assembled.state.counts(), (1, 1));
}
