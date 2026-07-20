//! Composition-owned runtime handles for invoking Boxology capabilities.
//!
//! Import handles are created before composition assembly finishes, then sealed
//! to their resolved targets by the runtime. Generated adapters receive only
//! the public lookup and call surface.

use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use boxology_contract::{
    BoxId, CallContext, CapabilityId, Detail, ErasedCallError, ErasedTarget, SlotValue,
    call_guarded,
};

type ErasedCallFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>;

/// The import handles declared by one box implementation.
///
/// Lookup is deterministic by import-slot identity. A missing slot returns
/// `None`; handles cannot be added or replaced through this public surface.
pub struct Imports {
    handles: BTreeMap<BoxId, ImportHandle>,
}

impl Imports {
    /// Returns the handle declared for `slot`, when one exists.
    pub fn handle(&self, slot: &BoxId) -> Option<&ImportHandle> {
        self.handles.get(slot)
    }

    #[allow(dead_code)] // Exercised by the builder in the next task slice.
    pub(crate) fn new(imports: impl IntoIterator<Item = (BoxId, Vec<CapabilityId>)>) -> Self {
        let handles = imports
            .into_iter()
            .map(|(slot, capabilities)| {
                let handle = ImportHandle::new(slot.clone(), capabilities);
                (slot, handle)
            })
            .collect();
        Self { handles }
    }
}

/// A lazy, composition-bound handle for one declared import slot.
///
/// Clones retain the slot and ordered capability identities and observe the
/// same eventual target seal. Until sealed, calls fail as unavailable.
#[derive(Clone)]
pub struct ImportHandle {
    slot: BoxId,
    capabilities: Arc<[CapabilityId]>,
    target: Arc<OnceLock<Arc<dyn ErasedTarget>>>,
}

impl ImportHandle {
    fn new(slot: BoxId, capabilities: Vec<CapabilityId>) -> Self {
        Self {
            slot,
            capabilities: capabilities.into(),
            target: Arc::new(OnceLock::new()),
        }
    }

    /// Returns this handle's declared import-slot identity.
    pub fn slot_id(&self) -> &BoxId {
        &self.slot
    }

    /// Returns allowed capability identities in declaration order.
    pub fn capabilities(&self) -> &[CapabilityId] {
        &self.capabilities
    }

    /// Invokes a declared capability through the sealed import target.
    ///
    /// Calls fail before provider invocation when the handle is unsealed, the
    /// capability is undeclared, or the supplied deadline is already expired,
    /// in that order. Provider panics are contained by the contract dispatch
    /// boundary. The returned future is `Send`.
    pub fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> ErasedCallFuture<'a> {
        let Some(target) = self.target.get() else {
            return Box::pin(ready(Err(ErasedCallError::Unavailable(Detail::new(
                "unsealed_import",
            )))));
        };
        if !self
            .capabilities
            .iter()
            .any(|allowed| allowed == capability)
        {
            return Box::pin(ready(Err(ErasedCallError::ContractViolation(Detail::new(
                "undeclared_import_capability",
            )))));
        }
        if context
            .deadline()
            .is_some_and(|deadline| deadline.remaining().is_zero())
        {
            return Box::pin(ready(Err(ErasedCallError::Deadline)));
        }
        call_guarded(target.as_ref(), capability, context, input)
    }

    #[allow(dead_code)] // Exercised by the builder in the next task slice.
    pub(crate) fn seal(&self, target: Arc<dyn ErasedTarget>) -> Result<(), Arc<dyn ErasedTarget>> {
        self.target.set(target)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};
    use std::time::{Duration, Instant};

    use boxology_contract::{
        Caller, CancelToken, CapabilityName, ContractValue, Deadline, TraceContext,
    };

    use super::*;

    enum Behavior {
        Echo,
        Error(ErasedCallError),
        ConstructionPanic,
        PollPanic,
    }

    struct Target {
        calls: Arc<AtomicUsize>,
        behavior: Behavior,
    }

    impl ErasedTarget for Target {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            _context: CallContext,
            input: SlotValue,
        ) -> ErasedCallFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.behavior {
                Behavior::Echo => Box::pin(ready(Ok(input))),
                Behavior::Error(error) => Box::pin(ready(Err(error.clone()))),
                Behavior::ConstructionPanic => panic!("construction panic"),
                Behavior::PollPanic => Box::pin(std::future::poll_fn(|_| {
                    panic!("poll panic");
                })),
            }
        }
    }

    fn box_id(value: &str) -> BoxId {
        BoxId::new(value).unwrap()
    }

    fn capability(package: &str, name: &str) -> CapabilityId {
        CapabilityId::new(box_id(package), CapabilityName::new(name).unwrap())
    }

    fn handle(names: &[&str]) -> ImportHandle {
        let slot = box_id("service");
        let capabilities = names
            .iter()
            .map(|name| capability("service", name))
            .collect();
        let imports = Imports::new([(slot.clone(), capabilities)]);
        imports.handle(&slot).unwrap().clone()
    }

    fn context(deadline: Option<Deadline>) -> CallContext {
        CallContext::new(
            Caller::Anonymous,
            deadline,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        )
    }

    fn target(calls: &Arc<AtomicUsize>, behavior: Behavior) -> Arc<dyn ErasedTarget> {
        Arc::new(Target {
            calls: Arc::clone(calls),
            behavior,
        })
    }

    fn invoke(
        handle: &ImportHandle,
        capability: &CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Result<SlotValue, ErasedCallError> {
        let mut future = handle.call(capability, context, input);
        loop {
            match future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
            {
                Poll::Ready(output) => return output,
                Poll::Pending => {}
            }
        }
    }

    #[test]
    fn short_circuits_in_required_order_without_provider_invocation() {
        let handle = handle(&["allowed"]);
        let allowed = capability("service", "allowed");
        let undeclared = capability("service", "undeclared");
        let deadline = Deadline::at(Instant::now());
        assert_eq!(deadline.remaining(), Duration::ZERO);
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = target(&calls, Behavior::Echo);

        assert_eq!(
            invoke(
                &handle,
                &undeclared,
                context(Some(deadline)),
                SlotValue::Null,
            ),
            Err(ErasedCallError::Unavailable(Detail::new("unsealed_import")))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        assert!(handle.seal(provider).is_ok());
        assert_eq!(
            invoke(
                &handle,
                &undeclared,
                context(Some(deadline)),
                SlotValue::Null,
            ),
            Err(ErasedCallError::ContractViolation(Detail::new(
                "undeclared_import_capability"
            )))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            invoke(&handle, &allowed, context(Some(deadline)), SlotValue::Null,),
            Err(ErasedCallError::Deadline)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sealed_calls_preserve_success_and_erased_errors() {
        let capability = capability("service", "call");
        let input = SlotValue::Value(ContractValue::string("payload"));
        let success = handle(&["call"]);
        let success_calls = Arc::new(AtomicUsize::new(0));
        assert!(success.seal(target(&success_calls, Behavior::Echo)).is_ok());
        let deadline = Deadline::at(Instant::now() + Duration::from_secs(3600));
        assert_eq!(
            invoke(
                &success,
                &capability,
                context(Some(deadline)),
                input.clone()
            ),
            Ok(input.clone())
        );
        assert_eq!(success_calls.load(Ordering::SeqCst), 1);

        let expected = ErasedCallError::Domain {
            error_tag: "ordinary".into(),
            payload: input,
        };
        let failure = handle(&["call"]);
        let failure_calls = Arc::new(AtomicUsize::new(0));
        assert!(
            failure
                .seal(target(&failure_calls, Behavior::Error(expected.clone())))
                .is_ok()
        );
        assert_eq!(
            invoke(&failure, &capability, context(None), SlotValue::Null),
            Err(expected)
        );
        assert_eq!(failure_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dispatch_panics_reach_internal_through_the_handle() {
        let capability = capability("service", "call");
        for behavior in [Behavior::ConstructionPanic, Behavior::PollPanic] {
            let handle = handle(&["call"]);
            let calls = Arc::new(AtomicUsize::new(0));
            assert!(handle.seal(target(&calls, behavior)).is_ok());
            let error = invoke(&handle, &capability, context(None), SlotValue::Null).unwrap_err();
            let ErasedCallError::Internal(detail) = error else {
                panic!("expected Internal, got {error:?}");
            };
            assert_eq!(detail.code(), "panic");
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn clones_observe_the_same_seal() {
        let original = handle(&["call"]);
        let clone = original.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        assert!(original.seal(target(&calls, Behavior::Echo)).is_ok());

        assert_eq!(
            invoke(
                &clone,
                &capability("service", "call"),
                context(None),
                SlotValue::Null,
            ),
            Ok(SlotValue::Null)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn public_carriers_and_returned_future_are_thread_safe() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        fn assert_send<T: Send>(value: T) -> T {
            value
        }

        assert_bounds::<Imports>();
        assert_bounds::<ImportHandle>();
        let service = box_id("service");
        let capabilities = vec![
            capability("service", "second"),
            capability("service", "first"),
        ];
        let imports = Imports::new([(service.clone(), capabilities.clone())]);
        let handle = imports.handle(&service).unwrap();
        assert_eq!(handle.slot_id(), &service);
        assert_eq!(handle.capabilities(), capabilities);
        assert!(imports.handle(&box_id("missing")).is_none());

        let calls = Arc::new(AtomicUsize::new(0));
        assert!(handle.seal(target(&calls, Behavior::Echo)).is_ok());
        let capability = capability("service", "first");
        drop(assert_send(handle.call(
            &capability,
            context(None),
            SlotValue::Null,
        )));
    }
}
