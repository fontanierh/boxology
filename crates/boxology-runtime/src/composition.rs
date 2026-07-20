//! Transportless composition registration, validation, and start.

use std::{collections::BTreeMap, sync::Arc};

use boxology_contract::{BoxId, ErasedTarget, ImplementationDescriptor};

use crate::{AssemblyError, AssemblyErrors, ImportHandle, Imports};

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
/// Builds and validates one composition without starting traffic.
#[derive(Default)]
pub struct CompositionBuilder {
    boxes: Vec<BoxRegistration>,
    resolutions: Vec<ImportResolution>,
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
        AssemblyErrors::from_errors(errors).map_or(Ok(()), Err)
    }

    /// Validates and seals every declared import into a transportless composition.
    pub fn start(self) -> Result<Composition, AssemblyErrors> {
        self.validate()?;
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
        Ok(Composition { _boxes: self.boxes })
    }
}
/// A successfully validated, transportless composition.
pub struct Composition {
    _boxes: Vec<BoxRegistration>,
}
