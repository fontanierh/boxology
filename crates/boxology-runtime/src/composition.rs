//! Composition registration, validation, and transport-aware start.

use std::{collections::BTreeMap, future::Future, sync::Arc, task::Poll, time::Duration};

use boxology_contract::{
    BoxHandle, BoxId, CallContext, CapabilityDescriptor, CapabilityId, Detail, ErasedCallError,
    ErasedTarget, ExposureLevel, ImplementationDescriptor, SlotValue,
};
use tokio_util::sync::CancellationToken;

use crate::{
    AssemblyError, AssemblyErrors, ImportHandle, Imports, LocalBinding, RemoteImportTarget,
    TransportBinding, TransportExposure, TransportHandle, TransportRuntime, TransportTaskTracker,
};

/// A selected target for one declared import slot.
#[derive(Clone)]
pub struct ImportTarget(ImportTargetKind);
#[derive(Clone)]
enum ImportTargetKind {
    Local(BoxId),
    Remote(Arc<dyn RemoteImportTarget>),
}
impl ImportTarget {
    /// Selects a registered in-process provider box.
    pub fn local(provider: BoxId) -> Self {
        Self(ImportTargetKind::Local(provider))
    }

    /// Selects an already-configured caller-side target.
    pub fn remote(target: Arc<dyn RemoteImportTarget>) -> Self {
        Self(ImportTargetKind::Remote(target))
    }
}
impl std::fmt::Debug for ImportTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            ImportTargetKind::Local(provider) => {
                write!(formatter, "ImportTarget(Local({provider:?}))")
            }
            ImportTargetKind::Remote(_) => formatter.write_str("ImportTarget(Remote(<redacted>))"),
        }
    }
}
impl PartialEq for ImportTarget {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (ImportTargetKind::Local(left), ImportTargetKind::Local(right)) => left == right,
            (ImportTargetKind::Remote(left), ImportTargetKind::Remote(right)) => {
                Arc::ptr_eq(left, right)
            }
            _ => false,
        }
    }
}
impl Eq for ImportTarget {}

struct CallerTargetAdapter(Arc<dyn RemoteImportTarget>);
impl ErasedTarget for CallerTargetAdapter {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>
    {
        self.0.call(capability, context, input)
    }
}
struct BoxRegistration {
    descriptor: ImplementationDescriptor,
    handles: BTreeMap<BoxId, ImportHandle>,
    target: Arc<dyn ErasedTarget>,
}
struct ImportResolution {
    consumer: BoxId,
    slot: BoxId,
    target: ImportTarget,
}
type ExposureRegistration = (BoxId, CapabilityId, ExposureLevel, usize);
type BindingGroup = (usize, Box<dyn ErasedTransportBinding>);
trait ErasedTransportBinding: Send + Sync {
    fn conform(
        &self,
        descriptor: &CapabilityDescriptor,
        level: ExposureLevel,
    ) -> Result<(), Detail>;
    fn prepare(&self, descriptors: &[&'static CapabilityDescriptor]) -> Result<(), Detail>;
    fn start(
        &self,
        exposures: Arc<[TransportExposure]>,
        tracker: TransportTaskTracker,
        activation: CancellationToken,
    ) -> Result<Box<dyn TransportHandle>, Detail>;
}
struct OwnedTransport<B: TransportBinding> {
    binding: Arc<B>,
    config: Arc<B::Config>,
}
impl<B: TransportBinding> ErasedTransportBinding for OwnedTransport<B> {
    fn conform(
        &self,
        descriptor: &CapabilityDescriptor,
        level: ExposureLevel,
    ) -> Result<(), Detail> {
        self.binding.conform(descriptor, level)
    }
    fn prepare(&self, descriptors: &[&'static CapabilityDescriptor]) -> Result<(), Detail> {
        self.binding.prepare(descriptors)
    }
    fn start(
        &self,
        exposures: Arc<[TransportExposure]>,
        tracker: TransportTaskTracker,
        activation: CancellationToken,
    ) -> Result<Box<dyn TransportHandle>, Detail> {
        let runtime = TransportRuntime::new(exposures, tracker, self.config.clone(), activation);
        self.binding
            .start(runtime)
            .map(|handle| Box::new(handle) as Box<dyn TransportHandle>)
    }
}
/// Builds and validates one composition without starting traffic.
#[derive(Default)]
pub struct CompositionBuilder {
    boxes: Vec<BoxRegistration>,
    resolutions: Vec<ImportResolution>,
    exposures: Vec<ExposureRegistration>,
    bindings: Vec<BindingGroup>,
}

/// One box registered in a composition draft.
///
/// This typed token lets application code connect and expose a box without repeating its string
/// identity or walking its implementation descriptor.
#[derive(Clone)]
pub struct RegisteredBox {
    id: BoxId,
    capabilities: Arc<[CapabilityId]>,
}

impl RegisteredBox {
    /// Returns the registered box identity.
    pub fn id(&self) -> &BoxId {
        &self.id
    }
}

impl CompositionBuilder {
    /// Constructs an empty composition builder.
    pub fn new() -> Self {
        Self::default()
    }
    /// Registers a box and immediately constructs its target from lazy imports.
    pub fn add_box<T, F>(&mut self, descriptor: ImplementationDescriptor, factory: F) -> &mut Self
    where
        T: ErasedTarget + 'static,
        F: FnOnce(Imports) -> T,
    {
        let imports = Imports::new(
            descriptor
                .imports()
                .iter()
                .map(|import| (import.slot_id().clone(), import.capabilities().to_vec())),
        );
        let handles = imports.cloned_handles();
        let target: Arc<dyn ErasedTarget> = Arc::new(factory(imports));
        self.boxes.push(BoxRegistration {
            descriptor,
            handles,
            target,
        });
        self
    }
    /// Registers a box and returns a token for concise typed wiring.
    pub fn register<T, F>(
        &mut self,
        descriptor: ImplementationDescriptor,
        factory: F,
    ) -> RegisteredBox
    where
        T: ErasedTarget + 'static,
        F: FnOnce(Imports) -> T,
    {
        let registered = RegisteredBox {
            id: descriptor.contract().box_id().clone(),
            capabilities: descriptor
                .contract()
                .capabilities()
                .iter()
                .map(|capability| capability.id().clone())
                .collect(),
        };
        self.add_box(descriptor, factory);
        registered
    }
    /// Connects the consumer import slot named by the provider box to that local provider.
    pub fn connect(&mut self, consumer: &RegisteredBox, provider: &RegisteredBox) -> &mut Self {
        self.resolve_import(
            consumer.id.clone(),
            provider.id.clone(),
            ImportTarget::local(provider.id.clone()),
        )
    }
    /// Exposes every capability of one registered box through a transport allocation.
    pub fn expose_all<B>(
        &mut self,
        provider: &RegisteredBox,
        transport: Arc<B>,
        level: ExposureLevel,
    ) -> &mut Self
    where
        B: TransportBinding,
    {
        for capability in provider.capabilities.iter().cloned() {
            self.expose(provider.id.clone(), capability, transport.clone(), level);
        }
        self
    }
    /// Creates a generated typed handle and exposes its box in-process.
    pub fn handle<H>(&mut self, provider: &RegisteredBox) -> H
    where
        H: BoxHandle,
    {
        let local = Arc::new(LocalBinding::new());
        self.expose_all(provider, local.clone(), ExposureLevel::CodeOnly);
        H::from_erased(local)
    }
    /// Records a target selection for one consumer import slot.
    pub fn resolve_import(
        &mut self,
        consumer: BoxId,
        slot: BoxId,
        target: ImportTarget,
    ) -> &mut Self {
        self.resolutions.push(ImportResolution {
            consumer,
            slot,
            target,
        });
        self
    }
    /// Exposes one exact provider capability through a configured transport allocation.
    pub fn expose<B>(
        &mut self,
        provider: BoxId,
        capability: CapabilityId,
        transport: Arc<B>,
        level: ExposureLevel,
    ) -> &mut Self
    where
        B: TransportBinding,
    {
        let allocation_key = Arc::as_ptr(&transport).cast::<()>().addr();
        let binding_group = self
            .bindings
            .iter()
            .position(|group| group.0 == allocation_key)
            .unwrap_or_else(|| {
                let index = self.bindings.len();
                let config = transport.config();
                self.bindings.push((
                    allocation_key,
                    Box::new(OwnedTransport {
                        binding: transport,
                        config,
                    }),
                ));
                index
            });
        self.exposures
            .push((provider, capability, level, binding_group));
        self
    }
    /// Reports every assembly failure without sealing imports or starting traffic.
    pub fn validate(&self) -> Result<(), AssemblyErrors> {
        let mut errors = Vec::new();
        let mut registrations = BTreeMap::new();
        for (index, registration) in self.boxes.iter().enumerate() {
            let box_id = registration.descriptor.contract().box_id();
            if registrations.contains_key(box_id) {
                errors.push(AssemblyError::DuplicateBox {
                    box_id: box_id.clone(),
                });
            } else {
                registrations.insert(box_id, index);
            }
        }
        let mut states = BTreeMap::new();
        for resolution in &self.resolutions {
            let Some(&consumer_index) = registrations.get(&resolution.consumer) else {
                let consumer = resolution.consumer.clone();
                errors.push(AssemblyError::UnknownImportConsumer { consumer });
                continue;
            };
            let consumer = &self.boxes[consumer_index];
            if !consumer
                .descriptor
                .imports()
                .iter()
                .any(|import| import.slot_id() == &resolution.slot)
            {
                let consumer = resolution.consumer.clone();
                let slot = resolution.slot.clone();
                errors.push(AssemblyError::UnknownImportSlot { consumer, slot });
                continue;
            }
            let key = (&resolution.consumer, &resolution.slot);
            if states.insert(key, None).is_some() {
                let consumer = resolution.consumer.clone();
                let slot = resolution.slot.clone();
                errors.push(AssemblyError::DuplicateImportResolution { consumer, slot });
                continue;
            }
            match &resolution.target.0 {
                ImportTargetKind::Local(target) => {
                    if let Some(&provider_index) = registrations.get(target) {
                        states.insert(key, Some(provider_index));
                    } else {
                        errors.push(AssemblyError::UnknownImportTarget {
                            consumer: resolution.consumer.clone(),
                            slot: resolution.slot.clone(),
                            target: target.clone(),
                        });
                    }
                }
                ImportTargetKind::Remote(target) => {
                    let import = consumer
                        .descriptor
                        .imports()
                        .iter()
                        .find(|import| import.slot_id() == &resolution.slot)
                        .unwrap();
                    for capability in import.capabilities() {
                        if !target.supports_capability(capability) {
                            errors.push(AssemblyError::MissingImportedCapability {
                                consumer: resolution.consumer.clone(),
                                slot: resolution.slot.clone(),
                                capability: capability.clone(),
                            });
                        }
                    }
                }
            }
        }
        for (index, registration) in self.boxes.iter().enumerate() {
            let consumer = registration.descriptor.contract().box_id();
            if registrations.get(consumer) != Some(&index) {
                continue;
            }
            for import in registration.descriptor.imports() {
                match states.get(&(consumer, import.slot_id())) {
                    None => {
                        let consumer = consumer.clone();
                        let slot = import.slot_id().clone();
                        errors.push(AssemblyError::MissingImportResolution { consumer, slot });
                    }
                    Some(Some(provider_index)) => {
                        let contract = self.boxes[*provider_index].descriptor.contract();
                        let provided = contract.capabilities();
                        for capability in import.capabilities() {
                            if !provided.iter().any(|known| known.id() == capability) {
                                errors.push(AssemblyError::MissingImportedCapability {
                                    consumer: consumer.clone(),
                                    slot: import.slot_id().clone(),
                                    capability: capability.clone(),
                                });
                            }
                        }
                    }
                    Some(None) => {}
                }
            }
        }
        for (provider, capability, level, binding_group) in &self.exposures {
            let Some(&provider_index) = registrations.get(provider) else {
                errors.push(AssemblyError::UnknownExposureProvider {
                    provider: provider.clone(),
                });
                continue;
            };
            let Some(descriptor) = self.boxes[provider_index]
                .descriptor
                .contract()
                .capabilities()
                .iter()
                .find(|descriptor| descriptor.id() == capability)
            else {
                errors.push(AssemblyError::UnknownExposedCapability {
                    provider: provider.clone(),
                    capability: capability.clone(),
                });
                continue;
            };
            if *level > descriptor.max_exposure() {
                errors.push(AssemblyError::ExposureExceedsMaximum {
                    capability: capability.clone(),
                    requested: *level,
                    maximum: descriptor.max_exposure(),
                });
            } else if let Err(detail) = self.bindings[*binding_group].1.conform(descriptor, *level)
            {
                errors.push(AssemblyError::TransportConformanceFailed {
                    capability: capability.clone(),
                    detail,
                });
            }
        }
        AssemblyErrors::from_errors(errors).map_or(Ok(()), Err)
    }

    /// Validates, starts every transport closed, then atomically commits traffic.
    pub fn start(self) -> Result<Composition, AssemblyErrors> {
        self.validate()?;
        let mut grouped = vec![Vec::new(); self.bindings.len()];
        for (provider, capability, level, binding_group) in &self.exposures {
            let registration = self
                .boxes
                .iter()
                .find(|registration| registration.descriptor.contract().box_id() == provider)
                .unwrap();
            let descriptor = registration
                .descriptor
                .contract()
                .capabilities()
                .iter()
                .find(|descriptor| descriptor.id() == capability)
                .unwrap();
            grouped[*binding_group].push(TransportExposure::new(
                descriptor,
                *level,
                registration.target.clone(),
            ));
        }
        let grouped: Vec<Arc<[TransportExposure]>> = grouped.into_iter().map(Arc::from).collect();
        for (binding, exposures) in self.bindings.iter().zip(&grouped) {
            let descriptors: Vec<_> = exposures
                .iter()
                .map(TransportExposure::descriptor)
                .collect();
            if let Err(detail) = binding.1.prepare(&descriptors) {
                return Err(single_error(AssemblyError::TransportPrepareFailed {
                    detail,
                }));
            }
        }
        let tracker = TransportTaskTracker::new();
        let activation = CancellationToken::new();
        let mut handles = Vec::with_capacity(self.bindings.len());
        for (binding, exposures) in self.bindings.iter().zip(&grouped) {
            match binding
                .1
                .start(exposures.clone(), tracker.clone(), activation.clone())
            {
                Ok(handle) => handles.push(handle),
                Err(detail) => {
                    for handle in handles.iter().rev() {
                        handle.stop_intake();
                    }
                    for handle in handles.iter().rev() {
                        handle.cancel_tasks();
                    }
                    for handle in handles.iter().rev() {
                        handle.abort_tasks();
                    }
                    return Err(single_error(AssemblyError::TransportStartFailed { detail }));
                }
            }
        }
        for registration in &self.boxes {
            let consumer = registration.descriptor.contract().box_id();
            for import in registration.descriptor.imports() {
                let resolution = self
                    .resolutions
                    .iter()
                    .find(|resolution| {
                        &resolution.consumer == consumer && resolution.slot == *import.slot_id()
                    })
                    .unwrap();
                let target: Arc<dyn ErasedTarget> = match &resolution.target.0 {
                    ImportTargetKind::Local(provider) => self
                        .boxes
                        .iter()
                        .find(|registration| {
                            registration.descriptor.contract().box_id() == provider
                        })
                        .unwrap()
                        .target
                        .clone(),
                    ImportTargetKind::Remote(target) => {
                        Arc::new(CallerTargetAdapter(target.clone()))
                    }
                };
                let handle = registration.handles.get(import.slot_id()).unwrap();
                assert!(
                    handle.seal(target).is_ok(),
                    "import handle was already sealed"
                );
            }
        }
        activation.cancel();
        Ok(Composition {
            _boxes: self.boxes,
            _bindings: self.bindings.into_iter().map(|group| group.1).collect(),
            _exposures: grouped,
            _handles: handles,
            _tracker: tracker,
            _activation: activation,
        })
    }
}
fn single_error(error: AssemblyError) -> AssemblyErrors {
    AssemblyErrors::from_errors(vec![error]).unwrap()
}
/// A successfully validated and activated composition.
pub struct Composition {
    _boxes: Vec<BoxRegistration>,
    _bindings: Vec<Box<dyn ErasedTransportBinding>>,
    _exposures: Vec<Arc<[TransportExposure]>>,
    _handles: Vec<Box<dyn TransportHandle>>,
    _tracker: TransportTaskTracker,
    _activation: CancellationToken,
}

impl Composition {
    /// Stops transport intake and drains, cancels, or aborts all tracked work.
    pub async fn shutdown(mut self, drain_timeout: Duration) -> Result<(), ErasedCallError> {
        for handle in self._handles.iter().rev() {
            handle.stop_intake();
        }
        self._tracker.close();
        if completes_within(&self._tracker, drain_timeout).await {
            return Ok(());
        }
        for handle in self._handles.iter().rev() {
            handle.cancel_tasks();
        }
        if completes_within(&self._tracker, drain_timeout).await {
            return Ok(());
        }
        for handle in self._handles.iter().rev() {
            handle.abort_tasks();
        }
        let handles = std::mem::take(&mut self._handles);
        let mut first_failure = None;
        for handle in handles.into_iter().rev() {
            if let Err(detail) = handle.join_tasks().await
                && first_failure.is_none()
            {
                first_failure = Some(detail);
            }
        }
        let result = first_failure.map_or(Ok(()), |detail| Err(ErasedCallError::Internal(detail)));
        drop(self);
        result
    }
}

async fn completes_within(tracker: &TransportTaskTracker, duration: Duration) -> bool {
    let mut completion = Box::pin(tracker.wait());
    let mut timeout = Box::pin(tokio::time::sleep(duration));
    std::future::poll_fn(|context| {
        if completion.as_mut().poll(context).is_ready() {
            return Poll::Ready(true);
        }
        timeout.as_mut().poll(context).map(|()| false)
    })
    .await
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    use crate::TransportJoinFuture;
    use std::{future::pending, sync::Mutex};
    use tokio::task::JoinHandle;
    use tokio_util::task::task_tracker::TaskTrackerToken;

    #[derive(Clone, Copy)]
    enum Exit {
        CancelAfter(Duration),
        Never,
    }

    struct LifecycleHandle {
        id: u8,
        trace: Arc<Mutex<Vec<String>>>,
        cancel: CancellationToken,
        tokens: Mutex<Vec<TaskTrackerToken>>,
        tasks: Vec<JoinHandle<Result<(), Detail>>>,
    }

    impl LifecycleHandle {
        fn record(&self, phase: &str) {
            self.trace
                .lock()
                .unwrap()
                .push(format!("{phase}{}", self.id));
        }
    }

    impl TransportHandle for LifecycleHandle {
        fn stop_intake(&self) {
            self.record("stop");
        }

        fn cancel_tasks(&self) {
            self.record("cancel");
            self.cancel.cancel();
            self.tokens.lock().unwrap().clear();
        }

        fn abort_tasks(&self) {
            self.record("abort");
            for task in &self.tasks {
                task.abort();
            }
        }

        fn join_tasks(self: Box<Self>) -> TransportJoinFuture {
            Box::pin(async move {
                let mut first_failure = None;
                for (index, task) in self.tasks.into_iter().enumerate() {
                    let event = format!("join{}.{index}", self.id);
                    self.trace.lock().unwrap().push(event.clone());
                    let result = match task.await {
                        Ok(result) => result,
                        Err(_) => Err(Detail::new(event)),
                    };
                    if let Err(detail) = result
                        && first_failure.is_none()
                    {
                        first_failure = Some(detail);
                    }
                }
                first_failure.map_or(Ok(()), Err)
            })
        }
    }

    fn handle(
        id: u8,
        tracker: &TransportTaskTracker,
        trace: &Arc<Mutex<Vec<String>>>,
        exit: Option<Exit>,
        task_count: usize,
    ) -> LifecycleHandle {
        let cancel = CancellationToken::new();
        let tasks = (0..task_count)
            .map(|_| {
                let cancel = cancel.clone();
                tracker.spawn(async move {
                    match exit.expect("task requires an exit mode") {
                        Exit::CancelAfter(delay) => {
                            cancel.cancelled().await;
                            tokio::time::sleep(delay).await;
                            Ok(())
                        }
                        Exit::Never => pending().await,
                    }
                })
            })
            .collect();
        LifecycleHandle {
            id,
            trace: trace.clone(),
            cancel,
            tokens: Mutex::new(Vec::new()),
            tasks,
        }
    }

    fn token_handle(
        id: u8,
        tracker: &TransportTaskTracker,
        trace: &Arc<Mutex<Vec<String>>>,
    ) -> LifecycleHandle {
        let mut handle = handle(id, tracker, trace, None, 0);
        handle.tokens = Mutex::new(vec![tracker.token()]);
        handle
    }

    fn composition(tracker: &TransportTaskTracker, handles: Vec<LifecycleHandle>) -> Composition {
        Composition {
            _boxes: Vec::new(),
            _bindings: Vec::new(),
            _exposures: Vec::new(),
            _handles: handles
                .into_iter()
                .map(|handle| Box::new(handle) as Box<dyn TransportHandle>)
                .collect(),
            _tracker: tracker.clone(),
            _activation: CancellationToken::new(),
        }
    }

    fn events(trace: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        trace.lock().unwrap().clone()
    }

    fn run_paused(future: impl Future<Output = ()>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            tokio::time::pause();
            future.await;
        });
    }

    #[test]
    fn immediate_drain_closes_tracker_stops_in_reverse_and_wins_zero_tie() {
        run_paused(async {
            let tracker = TransportTaskTracker::new();
            let trace = Arc::new(Mutex::new(Vec::new()));
            let handles = (1..=2)
                .map(|id| handle(id, &tracker, &trace, None, 0))
                .collect();

            assert_eq!(
                composition(&tracker, handles)
                    .shutdown(Duration::ZERO)
                    .await,
                Ok(())
            );
            assert!(tracker.is_closed() && tracker.is_empty());
            assert_eq!(events(&trace), ["stop2", "stop1"]);
        });
    }

    #[test]
    fn drain_timeout_cancels_in_reverse_then_uses_a_fresh_grace_window() {
        run_paused(async {
            let tracker = TransportTaskTracker::new();
            let trace = Arc::new(Mutex::new(Vec::new()));
            let handles = (1..=2)
                .map(|id| {
                    handle(
                        id,
                        &tracker,
                        &trace,
                        Some(Exit::CancelAfter(Duration::from_secs(4))),
                        1,
                    )
                })
                .collect();

            assert_eq!(
                composition(&tracker, handles)
                    .shutdown(Duration::from_secs(5))
                    .await,
                Ok(())
            );
            assert!(tracker.is_empty());
            assert_eq!(events(&trace), ["stop2", "stop1", "cancel2", "cancel1"]);
        });
    }

    #[test]
    fn grace_completion_wins_a_same_poll_zero_timeout_tie() {
        run_paused(async {
            let tracker = TransportTaskTracker::new();
            let trace = Arc::new(Mutex::new(Vec::new()));
            let handles = (1..=2)
                .map(|id| token_handle(id, &tracker, &trace))
                .collect();

            assert_eq!(
                composition(&tracker, handles)
                    .shutdown(Duration::ZERO)
                    .await,
                Ok(())
            );
            assert!(tracker.is_empty());
            assert_eq!(events(&trace), ["stop2", "stop1", "cancel2", "cancel1"]);
        });
    }

    #[test]
    fn forced_cleanup_aborts_globally_then_joins_every_task_in_reverse() {
        run_paused(async {
            let tracker = TransportTaskTracker::new();
            let trace = Arc::new(Mutex::new(Vec::new()));
            let handles = [(1, 1), (2, 1), (3, 2)]
                .into_iter()
                .map(|(id, count)| handle(id, &tracker, &trace, Some(Exit::Never), count))
                .collect();

            let error = composition(&tracker, handles)
                .shutdown(Duration::ZERO)
                .await
                .unwrap_err();
            assert_eq!(error, ErasedCallError::Internal(Detail::new("join3.0")));
            assert!(tracker.is_closed() && tracker.is_empty());
            assert_eq!(
                events(&trace),
                [
                    "stop3", "stop2", "stop1", "cancel3", "cancel2", "cancel1", "abort3", "abort2",
                    "abort1", "join3.0", "join3.1", "join2.0", "join1.0",
                ]
            );
        });
    }
}
