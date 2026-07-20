//! Composition registration, validation, and transport-aware start.

use std::{collections::BTreeMap, sync::Arc};

use boxology_contract::{
    BoxId, CapabilityDescriptor, CapabilityId, Detail, ErasedTarget, ExposureLevel,
    ImplementationDescriptor,
};
use tokio_util::sync::CancellationToken;

use crate::{
    AssemblyError, AssemblyErrors, ImportHandle, Imports, TransportBinding, TransportExposure,
    TransportHandle, TransportRuntime, TransportTaskTracker,
};

/// A selected target for one declared import slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTarget(ImportTargetKind);
#[derive(Debug, Clone, PartialEq, Eq)]
enum ImportTargetKind {
    Local(BoxId),
}
impl ImportTarget {
    /// Selects a registered in-process provider box.
    pub fn local(provider: BoxId) -> Self {
        Self(ImportTargetKind::Local(provider))
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
            let ImportTargetKind::Local(target) = &resolution.target.0;
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
                let ImportTargetKind::Local(provider) = &resolution.target.0;
                let target = &self
                    .boxes
                    .iter()
                    .find(|registration| registration.descriptor.contract().box_id() == provider)
                    .unwrap()
                    .target;
                let handle = registration.handles.get(import.slot_id()).unwrap();
                assert!(
                    handle.seal(Arc::clone(target)).is_ok(),
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
