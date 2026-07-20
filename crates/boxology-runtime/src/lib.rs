//! Composition-owned runtime handles for invoking Boxology capabilities.
//!
//! Import handles are created before composition assembly finishes, then sealed
//! to their resolved targets by the runtime. Generated adapters receive only
//! the public lookup and call surface. Assembly failures are reported through
//! ordered, payload-free diagnostics.

mod assembly;
mod composition;
mod transport;

pub use assembly::{AssemblyError, AssemblyErrors};
pub use composition::{Composition, CompositionBuilder, ImportTarget};
pub use transport::{
    TransportBinding, TransportExposure, TransportHandle, TransportRuntime, TransportTaskTracker,
};

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

    pub(crate) fn cloned_handles(&self) -> BTreeMap<BoxId, ImportHandle> {
        self.handles.clone()
    }

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
        Caller, CancelToken, CapabilityDescriptor, CapabilityName, CapabilityShape,
        ContractDescriptor, ContractRevision, ContractValue, Deadline, ExposureLevel, Idempotency,
        ImplementationDescriptor, ImportDescriptor, TraceContext, TypeDescriptor,
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

    fn adapter(calls: &Arc<AtomicUsize>) -> Target {
        Target {
            calls: Arc::clone(calls),
            behavior: Behavior::Echo,
        }
    }
    fn implementation(
        package: &str,
        provided: &[&str],
        imports: &[(&str, &[&str])],
    ) -> ImplementationDescriptor {
        let revision = ContractRevision::new("r1").unwrap();
        let capabilities = provided.iter().map(|name| {
            CapabilityDescriptor::new(
                capability(package, name),
                TypeDescriptor::bool(),
                TypeDescriptor::bool(),
                TypeDescriptor::bool(),
                CapabilityShape::Unary,
                ExposureLevel::CodeOnly,
                Idempotency::None,
                None,
            )
        });
        let contract = Box::leak(Box::new(
            ContractDescriptor::new(box_id(package), capabilities, revision.clone()).unwrap(),
        ));
        let imports = imports.iter().map(|(slot, names)| {
            ImportDescriptor::new(
                box_id(slot),
                revision.clone(),
                names.iter().map(|name| capability(slot, name)),
            )
            .unwrap()
        });
        ImplementationDescriptor::new(contract, imports).unwrap()
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
    fn composition_validates_without_authorizing_then_seals_all_clones() {
        let provider = box_id("provider");
        let imported = capability("provider", "call");
        let selected = ImportTarget::local(provider.clone());
        assert_eq!(selected, selected.clone());
        let mut captured = None;
        let mut factories = 0;
        let consumer_calls = Arc::new(AtomicUsize::new(0));
        let provider_calls = Arc::new(AtomicUsize::new(0));
        let mut builder = CompositionBuilder::new();
        builder.add_box(
            implementation("consumer", &[], &[("provider", &["call"])]),
            |imports| {
                factories += 1;
                captured = Some(imports.handle(&provider).unwrap().clone());
                adapter(&consumer_calls)
            },
        );
        assert_eq!(factories, 1);
        builder.add_box(implementation("provider", &["call"], &[]), |_| {
            factories += 1;
            adapter(&provider_calls)
        });
        builder.resolve_import(box_id("consumer"), provider, selected);
        assert_eq!(builder.validate(), Ok(()));
        assert_eq!(builder.validate(), Ok(()));
        let handle = captured.unwrap();
        assert_eq!(
            invoke(&handle, &imported, context(None), SlotValue::Null),
            Err(ErasedCallError::Unavailable(Detail::new("unsealed_import")))
        );
        assert_eq!(provider_calls.load(Ordering::SeqCst), 0);
        let _composition = builder.start().unwrap();
        assert_eq!(
            invoke(&handle, &imported, context(None), SlotValue::Null),
            Ok(SlotValue::Null)
        );
        assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
        assert_eq!(consumer_calls.load(Ordering::SeqCst), 0);
        assert_eq!(factories, 2);
    }

    #[test]
    fn composition_aggregates_every_failure_with_exact_precedence_and_no_seals() {
        let c = box_id("consumer");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        let mut factories = 0;
        let mut builder = CompositionBuilder::new();
        builder.add_box(
            implementation(
                "consumer",
                &[],
                &[
                    ("missing", &["call"]),
                    ("partial", &["first", "second"]),
                    ("duplicate", &["call"]),
                    ("unknown", &["call"]),
                ],
            ),
            |imports| {
                factories += 1;
                handles = imports.cloned_handles().into_values().collect();
                adapter(&calls)
            },
        );
        for _ in 0..2 {
            builder.add_box(implementation("consumer", &[], &[]), |_| {
                factories += 1;
                adapter(&calls)
            });
        }
        assert_eq!(factories, 3);
        builder.add_box(implementation("partial", &[], &[]), |_| {
            factories += 1;
            adapter(&calls)
        });
        assert_eq!(factories, 4);
        let mut resolve = |c, s, t| {
            builder.resolve_import(box_id(c), box_id(s), ImportTarget::local(box_id(t)));
        };
        resolve("ghost", "slot", "partial");
        resolve("ghost", "slot", "partial");
        resolve("consumer", "undeclared", "partial");
        resolve("consumer", "undeclared", "partial");
        resolve("consumer", "duplicate", "partial");
        resolve("consumer", "duplicate", "absent");
        resolve("consumer", "unknown", "absent");
        resolve("consumer", "partial", "partial");
        use AssemblyError::*;
        let unknown_consumer = |name| UnknownImportConsumer {
            consumer: box_id(name),
        };
        let unknown_slot = || UnknownImportSlot {
            consumer: c.clone(),
            slot: box_id("undeclared"),
        };
        let missing_capability = |name| MissingImportedCapability {
            consumer: c.clone(),
            slot: box_id("partial"),
            capability: capability("partial", name),
        };
        let expected = vec![
            DuplicateBox { box_id: c.clone() },
            DuplicateBox { box_id: c.clone() },
            unknown_consumer("ghost"),
            unknown_consumer("ghost"),
            unknown_slot(),
            unknown_slot(),
            DuplicateImportResolution {
                consumer: c.clone(),
                slot: box_id("duplicate"),
            },
            UnknownImportTarget {
                consumer: c.clone(),
                slot: box_id("unknown"),
                target: box_id("absent"),
            },
            MissingImportResolution {
                consumer: c.clone(),
                slot: box_id("missing"),
            },
            missing_capability("first"),
            missing_capability("second"),
        ];
        let validated = builder.validate().unwrap_err();
        assert_eq!(validated.errors(), expected);
        let display = validated.to_string();
        assert_eq!(builder.validate().unwrap_err(), validated);
        assert_eq!(builder.validate().unwrap_err().to_string(), display);
        let started = builder.start().err().expect("invalid composition started");
        assert_eq!(started, validated);
        assert_eq!(started.to_string(), display);
        for handle in handles {
            let capability = handle.capabilities()[0].clone();
            assert_eq!(
                invoke(&handle, &capability, context(None), SlotValue::Null),
                Err(ErasedCallError::Unavailable(Detail::new("unsealed_import")))
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn public_carriers_and_returned_future_are_thread_safe() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        fn assert_send<T: Send>(value: T) -> T {
            value
        }

        assert_bounds::<Imports>();
        assert_bounds::<ImportHandle>();
        assert_bounds::<CompositionBuilder>();
        assert_bounds::<ImportTarget>();
        assert_bounds::<Composition>();
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
