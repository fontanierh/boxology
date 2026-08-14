//! Composition-owned runtime handles for invoking Boxology capabilities.
//!
//! Import handles are created before composition assembly finishes, then sealed
//! to their resolved targets by the runtime. Generated adapters receive only
//! the public lookup and call surface. Assembly failures are reported through
//! ordered, payload-free diagnostics.

mod assembly;
mod composition;
mod local;
#[cfg(feature = "test-support")]
pub mod test_support;
mod transport;

pub use assembly::{AssemblyError, AssemblyErrors};
pub use composition::{Composition, CompositionBuilder, ImportTarget, RegisteredBox};
pub use local::LocalBinding;
pub use transport::{
    TransportBinding, TransportExposure, TransportHandle, TransportJoinFuture, TransportRuntime,
    TransportTaskTracker,
};

use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use boxology_contract::{
    BoxId, CallContext, CapabilityId, Detail, ErasedCallError, ErasedCallTarget, ErasedTarget,
    SlotValue, call_guarded,
};

type ErasedCallFuture<'a> =
    Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>;

/// A configured remote target that can prove which exact capabilities it supports.
pub trait RemoteImportTarget: ErasedCallTarget {
    /// Reports whether the target supports this exact capability identity.
    fn supports_capability(&self, capability: &CapabilityId) -> bool;
}

/// The import handles declared by one box implementation.
///
/// Lookup is deterministic by import-slot identity. A missing slot returns
/// `None`; handles cannot be added or replaced through this public surface.
/// Applications receive this bundle only through [`CompositionBuilder`]; a
/// generated, doc-hidden `factory` is a composition hook, not a standalone
/// construction API. Ordinary composition code calls the generated `register`.
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

impl ErasedCallTarget for ImportHandle {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> ErasedCallFuture<'a> {
        ImportHandle::call(self, capability, context, input)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, Weak};
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
        drops: Option<Arc<AtomicUsize>>,
    }

    struct RemoteTarget {
        panic_on_call: bool,
        panic_on_poll: bool,
    }

    impl ErasedCallTarget for RemoteTarget {
        fn call<'a>(
            &'a self,
            _capability: &'a CapabilityId,
            _context: CallContext,
            input: SlotValue,
        ) -> ErasedCallFuture<'a> {
            assert!(!self.panic_on_call, "remote construction panic");
            if self.panic_on_poll {
                return Box::pin(std::future::poll_fn(|_| panic!("remote poll panic")));
            }
            Box::pin(ready(Ok(input)))
        }
    }

    impl RemoteImportTarget for RemoteTarget {
        fn supports_capability(&self, _capability: &CapabilityId) -> bool {
            true
        }
    }

    impl Drop for Target {
        fn drop(&mut self) {
            if let Some(drops) = &self.drops {
                drops.fetch_add(1, Ordering::SeqCst);
            }
        }
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
            drops: None,
        })
    }

    fn adapter(calls: &Arc<AtomicUsize>) -> Target {
        Target {
            calls: Arc::clone(calls),
            behavior: Behavior::Echo,
            drops: None,
        }
    }
    fn implementation(
        package: &str,
        provided: &[&str],
        imports: &[(&str, &[&str])],
    ) -> ImplementationDescriptor {
        let provided: Vec<_> = provided
            .iter()
            .map(|name| (*name, ExposureLevel::CodeOnly))
            .collect();
        implementation_at(package, &provided, imports)
    }
    fn implementation_at(
        package: &str,
        provided: &[(&str, ExposureLevel)],
        imports: &[(&str, &[&str])],
    ) -> ImplementationDescriptor {
        let revision = ContractRevision::new("r1").unwrap();
        let capabilities = provided.iter().map(|(name, maximum)| {
            CapabilityDescriptor::new(
                capability(package, name),
                TypeDescriptor::bool(),
                TypeDescriptor::bool(),
                TypeDescriptor::bool(),
                CapabilityShape::Unary,
                *maximum,
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
    #[derive(Default)]
    struct TransportProbe {
        trace: Mutex<Vec<String>>,
        runtimes: Mutex<Vec<Weak<TransportRuntime<()>>>>,
        drops: AtomicUsize,
        active_drops: Mutex<Vec<bool>>,
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ProbeFailure {
        None,
        Prepare,
        Start,
    }
    struct ProbeBinding {
        id: u8,
        failure: ProbeFailure,
        config: Arc<()>,
        probe: Arc<TransportProbe>,
        import: Option<ImportHandle>,
    }
    struct ProbeHandle {
        id: u8,
        runtime: Arc<TransportRuntime<()>>,
        probe: Arc<TransportProbe>,
    }
    impl Drop for ProbeHandle {
        fn drop(&mut self) {
            self.probe.drops.fetch_add(1, Ordering::SeqCst);
            self.probe
                .active_drops
                .lock()
                .unwrap()
                .push(self.runtime.is_active());
        }
    }
    impl TransportHandle for ProbeHandle {
        fn stop_intake(&self) {
            self.record("stop");
        }
        fn cancel_tasks(&self) {
            self.record("cancel");
        }
        fn abort_tasks(&self) {
            self.record("abort");
        }
        fn join_tasks(self: Box<Self>) -> TransportJoinFuture {
            Box::pin(ready(Ok(())))
        }
    }
    impl ProbeHandle {
        fn record(&self, phase: &str) {
            self.probe
                .trace
                .lock()
                .unwrap()
                .push(format!("{phase}{}", self.id));
        }
    }
    impl TransportBinding for ProbeBinding {
        type Config = ();
        type Handle = ProbeHandle;
        fn config(&self) -> Arc<()> {
            self.config.clone()
        }
        fn conform(
            &self,
            descriptor: &CapabilityDescriptor,
            level: ExposureLevel,
        ) -> Result<(), Detail> {
            let name = descriptor.name().to_string();
            let mut trace = self.probe.trace.lock().unwrap();
            trace.push(format!("c{name}:{level:?}"));
            drop(trace);
            let rejected = self.id == 0
                && ((name == "limited" && level == ExposureLevel::External)
                    || (name == "rejected" && level == ExposureLevel::Internal));
            (!rejected)
                .then_some(())
                .ok_or_else(|| Detail::new("test_conformance"))
        }
        fn prepare(&self, descriptors: &[&'static CapabilityDescriptor]) -> Result<(), Detail> {
            self.record('p', descriptors.iter().map(|item| item.id()));
            match self.failure {
                ProbeFailure::Prepare => Err(Detail::new("test_prepare")),
                _ => Ok(()),
            }
        }
        fn start(&self, runtime: TransportRuntime<()>) -> Result<ProbeHandle, Detail> {
            let exposures = runtime.exposures();
            let ids = exposures.iter().map(|item| item.descriptor().id());
            self.record('s', ids);
            assert!(std::ptr::eq(runtime.config(), self.config.as_ref()));
            let mut active = Box::pin(runtime.wait_until_active());
            assert!(!runtime.is_active() && matches!(poll_once(active.as_mut()), Poll::Pending));
            drop(active);
            if let Some(handle) = &self.import {
                let capability = &handle.capabilities()[0];
                let result = invoke(handle, capability, context(None), SlotValue::Null);
                assert_eq!(
                    result,
                    Err(ErasedCallError::Unavailable(Detail::new("unsealed_import")))
                );
            }
            if self.failure == ProbeFailure::Start {
                return Err(Detail::new("test_start"));
            }
            let runtime = Arc::new(runtime);
            let weak = Arc::downgrade(&runtime);
            self.probe.runtimes.lock().unwrap().push(weak);
            Ok(ProbeHandle {
                id: self.id,
                runtime,
                probe: self.probe.clone(),
            })
        }
    }
    impl ProbeBinding {
        fn new(id: u8, probe: &Arc<TransportProbe>, import: Option<ImportHandle>) -> Arc<Self> {
            Self::with_failure(id, ProbeFailure::None, probe, import)
        }
        fn with_failure(
            id: u8,
            failure: ProbeFailure,
            probe: &Arc<TransportProbe>,
            import: Option<ImportHandle>,
        ) -> Arc<Self> {
            Arc::new(Self {
                id,
                failure,
                config: Arc::new(()),
                probe: probe.clone(),
                import,
            })
        }
        fn record<'a>(&self, phase: char, ids: impl Iterator<Item = &'a CapabilityId>) {
            let ids = ids.map(ToString::to_string).collect::<Vec<_>>().join(",");
            let mut trace = self.probe.trace.lock().unwrap();
            trace.push(format!("{phase}{}:{ids}", self.id));
        }
    }
    struct FailureSetup {
        builder: CompositionBuilder,
        handle: ImportHandle,
        calls: Arc<AtomicUsize>,
        target_drops: Arc<AtomicUsize>,
        bindings: Vec<Weak<ProbeBinding>>,
        configs: Vec<Weak<()>>,
    }
    fn failure_setup(
        count: u8,
        failure: ProbeFailure,
        probe: &Arc<TransportProbe>,
    ) -> FailureSetup {
        let provider = box_id("provider");
        let call = capability("provider", "call");
        let calls = Arc::new(AtomicUsize::new(0));
        let target_drops = Arc::new(AtomicUsize::new(0));
        let mut captured = None;
        let mut builder = CompositionBuilder::new();
        builder.add_box(
            implementation("consumer", &[], &[("provider", &["call"])]),
            |imports| {
                captured = Some(imports.handle(&provider).unwrap().clone());
                adapter(&calls)
            },
        );
        builder.add_box(implementation("provider", &["call"], &[]), |_| Target {
            calls: calls.clone(),
            behavior: Behavior::Echo,
            drops: Some(target_drops.clone()),
        });
        builder.resolve_import(
            box_id("consumer"),
            provider.clone(),
            ImportTarget::local(provider.clone()),
        );
        let handle = captured.unwrap();
        let mut bindings = Vec::new();
        let mut configs = Vec::new();
        for id in 1..=count {
            let failure = if id == count {
                failure
            } else {
                ProbeFailure::None
            };
            let binding = ProbeBinding::with_failure(id, failure, probe, Some(handle.clone()));
            bindings.push(Arc::downgrade(&binding));
            configs.push(Arc::downgrade(&binding.config));
            builder.expose(
                provider.clone(),
                call.clone(),
                binding,
                ExposureLevel::CodeOnly,
            );
        }
        FailureSetup {
            builder,
            handle,
            calls,
            target_drops,
            bindings,
            configs,
        }
    }
    fn lifecycle(probe: &TransportProbe) -> Vec<String> {
        probe
            .trace
            .lock()
            .unwrap()
            .iter()
            .filter(|event| !event.starts_with("ccall:"))
            .cloned()
            .collect()
    }
    fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
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
    fn composition_starts_grouped_transports_then_commits_all_traffic() {
        let provider = box_id("provider");
        let imported = capability("provider", "call");
        let [b, c] = ["b", "c"].map(|name| capability("provider", name));
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
        builder.add_box(implementation("provider", &["call", "b", "c"], &[]), |_| {
            factories += 1;
            adapter(&provider_calls)
        });
        builder.resolve_import(box_id("consumer"), provider.clone(), selected);
        let handle = captured.unwrap();
        let probe = Arc::new(TransportProbe::default());
        let first = ProbeBinding::new(1, &probe, Some(handle.clone()));
        let second = ProbeBinding::new(2, &probe, Some(handle.clone()));
        let binding_weaks = [Arc::downgrade(&first), Arc::downgrade(&second)];
        let config_weaks = [
            Arc::downgrade(&first.config),
            Arc::downgrade(&second.config),
        ];
        assert!(!Arc::ptr_eq(&first, &second) && !Arc::ptr_eq(&first.config, &second.config));
        let level = ExposureLevel::CodeOnly;
        for (capability, binding) in [
            (imported.clone(), first.clone()),
            (b, second.clone()),
            (c.clone(), first.clone()),
            (c, first.clone()),
        ] {
            builder.expose(provider.clone(), capability, binding, level);
        }
        assert_eq!(builder.validate(), Ok(()));
        assert_eq!(builder.validate(), Ok(()));
        assert_eq!(
            invoke(&handle, &imported, context(None), SlotValue::Null),
            Err(ErasedCallError::Unavailable(Detail::new("unsealed_import")))
        );
        assert_eq!(provider_calls.load(Ordering::SeqCst), 0);
        drop((first, second));
        let _composition = builder.start().unwrap();
        let trace = probe.trace.lock().unwrap();
        assert_eq!(
            trace[trace.len() - 4..].join("|"),
            "p1:provider.call,provider.c,provider.c|p2:provider.b|s1:provider.call,provider.c,provider.c|s2:provider.b"
        );
        drop(trace);
        let runtimes: Vec<_> = probe
            .runtimes
            .lock()
            .unwrap()
            .iter()
            .map(Weak::upgrade)
            .collect();
        let runtimes: Vec<_> = runtimes.into_iter().map(Option::unwrap).collect();
        assert!(TransportTaskTracker::ptr_eq(
            runtimes[0].tracker(),
            runtimes[1].tracker()
        ));
        assert!(runtimes.iter().all(|runtime| runtime.is_active()));
        assert_eq!(
            invoke(&handle, &imported, context(None), SlotValue::Null),
            Ok(SlotValue::Null)
        );
        assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
        assert_eq!(consumer_calls.load(Ordering::SeqCst), 0);
        assert_eq!(factories, 2);
        assert!(binding_weaks.iter().all(|weak| weak.upgrade().is_some()));
        assert!(config_weaks.iter().all(|weak| weak.upgrade().is_some()));
        assert_eq!(probe.drops.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn composition_seals_remote_target_without_a_local_provider_and_contains_panics() {
        let slot = box_id("remote");
        let imported = capability("remote", "call");
        let remote: Arc<dyn RemoteImportTarget> = Arc::new(RemoteTarget {
            panic_on_call: false,
            panic_on_poll: false,
        });
        let selected = ImportTarget::remote(remote.clone());
        assert_eq!(selected, selected.clone());
        assert_eq!(format!("{selected:?}"), "ImportTarget(Remote(<redacted>))");
        assert_ne!(
            selected,
            ImportTarget::remote(Arc::new(RemoteTarget {
                panic_on_call: false,
                panic_on_poll: false,
            }))
        );
        assert_ne!(selected, ImportTarget::local(slot.clone()));

        for (target, expected) in [
            (selected, Ok(SlotValue::Null)),
            (
                ImportTarget::remote(Arc::new(RemoteTarget {
                    panic_on_call: true,
                    panic_on_poll: false,
                })),
                Err(ErasedCallError::Internal(
                    Detail::new("panic").with_message("remote construction panic"),
                )),
            ),
            (
                ImportTarget::remote(Arc::new(RemoteTarget {
                    panic_on_call: false,
                    panic_on_poll: true,
                })),
                Err(ErasedCallError::Internal(
                    Detail::new("panic").with_message("remote poll panic"),
                )),
            ),
        ] {
            let mut captured = None;
            let mut builder = CompositionBuilder::new();
            builder.add_box(
                implementation("consumer", &[], &[("remote", &["call"])]),
                |imports| {
                    captured = Some(imports.handle(&slot).unwrap().clone());
                    adapter(&Arc::new(AtomicUsize::new(0)))
                },
            );
            builder.resolve_import(box_id("consumer"), slot.clone(), target);
            let handle = captured.unwrap();
            assert_eq!(
                invoke(&handle, &imported, context(None), SlotValue::Null),
                Err(ErasedCallError::Unavailable(Detail::new("unsealed_import")))
            );
            let _composition = builder.start().unwrap();
            assert_eq!(
                invoke(&handle, &imported, context(None), SlotValue::Null),
                expected
            );
        }
    }

    #[test]
    fn prepare_failure_returns_without_starting_or_retaining_ownership() {
        let probe = Arc::new(TransportProbe::default());
        let FailureSetup {
            builder,
            handle,
            calls,
            target_drops,
            bindings,
            configs,
        } = failure_setup(2, ProbeFailure::Prepare, &probe);
        let error = builder.start().err().expect("prepare failure started");
        assert_eq!(
            lifecycle(&probe).join("|"),
            "p1:provider.call|p2:provider.call"
        );
        assert_eq!(
            error.errors(),
            &[AssemblyError::TransportPrepareFailed {
                detail: Detail::new("test_prepare"),
            }]
        );
        assert_eq!(error.to_string(), "transport prepare failed: test_prepare");
        assert_eq!(
            invoke(
                &handle,
                &capability("provider", "call"),
                context(None),
                SlotValue::Null
            ),
            Err(ErasedCallError::Unavailable(Detail::new("unsealed_import")))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(probe.runtimes.lock().unwrap().is_empty());
        assert_eq!(probe.drops.load(Ordering::SeqCst), 0);
        assert!(probe.active_drops.lock().unwrap().is_empty());
        assert!(bindings.iter().all(|weak| weak.upgrade().is_none()));
        assert!(configs.iter().all(|weak| weak.upgrade().is_none()));
        assert_eq!(target_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn start_failure_rolls_back_three_handles_in_global_reverse_phases() {
        let probe = Arc::new(TransportProbe::default());
        let FailureSetup {
            builder,
            handle,
            calls,
            target_drops,
            bindings,
            configs,
        } = failure_setup(4, ProbeFailure::Start, &probe);
        let error = builder.start().err().expect("start failure committed");
        let events = lifecycle(&probe);
        assert_eq!(
            events.join("|"),
            "p1:provider.call|p2:provider.call|p3:provider.call|p4:provider.call|s1:provider.call|s2:provider.call|s3:provider.call|s4:provider.call|stop3|stop2|stop1|cancel3|cancel2|cancel1|abort3|abort2|abort1"
        );
        assert!(events[..4].iter().all(|event| event.starts_with('p')));
        assert!(events[4..8].iter().all(|event| event.starts_with('s')));
        assert_eq!(
            error.errors(),
            &[AssemblyError::TransportStartFailed {
                detail: Detail::new("test_start"),
            }]
        );
        assert_eq!(error.to_string(), "transport start failed: test_start");
        assert_eq!(
            invoke(
                &handle,
                &capability("provider", "call"),
                context(None),
                SlotValue::Null
            ),
            Err(ErasedCallError::Unavailable(Detail::new("unsealed_import")))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let runtimes = probe.runtimes.lock().unwrap();
        assert_eq!(runtimes.len(), 3);
        assert!(runtimes.iter().all(|weak| weak.upgrade().is_none()));
        drop(runtimes);
        assert_eq!(probe.drops.load(Ordering::SeqCst), 3);
        let active_drops = probe.active_drops.lock().unwrap();
        assert_eq!(active_drops.len(), 3);
        assert!(active_drops.iter().all(|active| !active));
        drop(active_drops);
        assert!(bindings.iter().all(|weak| weak.upgrade().is_none()));
        assert!(configs.iter().all(|weak| weak.upgrade().is_none()));
        assert_eq!(target_drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn composition_aggregates_every_failure_with_exact_precedence_and_no_seals() {
        use ExposureLevel::{CodeOnly as C, External as E, Internal as I};
        let c = box_id("consumer");
        let valid = capability("consumer", "valid");
        let limited = capability("consumer", "limited");
        let rejected = capability("consumer", "rejected");
        let calls = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        let mut factories = 0;
        let mut builder = CompositionBuilder::new();
        builder.add_box(
            implementation_at(
                "consumer",
                &[("valid", E), ("limited", I), ("rejected", E)],
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
        builder.add_box(implementation("consumer", &["only_second"], &[]), |_| {
            factories += 1;
            adapter(&calls)
        });
        builder.add_box(implementation("consumer", &[], &[]), |_| {
            factories += 1;
            adapter(&calls)
        });
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
        let probe = Arc::new(TransportProbe::default());
        let binding = ProbeBinding::new(0, &probe, None);
        for (owner, capability, level) in [
            (box_id("ghost"), rejected.clone(), E),
            (c.clone(), capability("other", "rejected"), E),
            (c.clone(), limited.clone(), E),
            (c.clone(), rejected, I),
            (c.clone(), capability("consumer", "only_second"), E),
            (c.clone(), valid.clone(), E),
            (c.clone(), limited, C),
            (c.clone(), valid.clone(), I),
            (c.clone(), valid, I),
        ] {
            builder.expose(owner, capability, binding.clone(), level);
        }
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
        assert_eq!(&validated.errors()[..expected.len()], expected);
        let display = validated.to_string();
        assert_eq!(
            display
                .lines()
                .skip(expected.len())
                .collect::<Vec<_>>()
                .join("|"),
            "unknown exposure provider: ghost|unknown exposed capability other.rejected for provider consumer|exposure external exceeds maximum internal for capability consumer.limited|transport conformance failed for capability consumer.rejected: test_conformance|unknown exposed capability consumer.only_second for provider consumer"
        );
        let conformed =
            "crejected:Internal|cvalid:External|climited:CodeOnly|cvalid:Internal|cvalid:Internal";
        assert_eq!(probe.trace.lock().unwrap().join("|"), conformed);
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
        assert_eq!(
            probe.trace.lock().unwrap().join("|"),
            [conformed; 4].join("|")
        );
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
        let carrier: &dyn ErasedCallTarget = handle;
        let mut future = assert_send(carrier.call(&capability, context(None), SlotValue::Null));
        assert_eq!(poll_once(future.as_mut()), Poll::Ready(Ok(SlotValue::Null)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
