//! Implementation-local import registration descriptors.

use std::error::Error;
use std::fmt;

use crate::{BoxId, CapabilityId, ContractDescriptor, ContractRevision};

/// One implementation import slot and its checked-in foreign contract view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDescriptor {
    slot_id: BoxId,
    expected_revision: ContractRevision,
    capabilities: Vec<CapabilityId>,
}

impl ImportDescriptor {
    /// Constructs an import slot, preserving the supplied capability order.
    pub fn new(
        slot_id: BoxId,
        expected_revision: ContractRevision,
        capabilities: impl IntoIterator<Item = CapabilityId>,
    ) -> Result<Self, ImportDescriptorError> {
        let mut accepted: Vec<CapabilityId> = Vec::new();
        for capability in capabilities {
            if capability.box_id() != &slot_id {
                return Err(ImportDescriptorError::CapabilityPackageMismatch {
                    slot_id,
                    capability,
                });
            }
            if accepted.iter().any(|known| known == &capability) {
                return Err(ImportDescriptorError::DuplicateCapability { capability });
            }
            accepted.push(capability);
        }
        Ok(Self {
            slot_id,
            expected_revision,
            capabilities: accepted,
        })
    }

    /// Returns the import-slot identity, which is the foreign package id in v0.
    pub fn slot_id(&self) -> &BoxId {
        &self.slot_id
    }

    /// Returns the expected revision from the foreign checked-in schema.
    pub fn expected_revision(&self) -> &ContractRevision {
        &self.expected_revision
    }

    /// Returns imported capability identities in their supplied order.
    pub fn capabilities(&self) -> &[CapabilityId] {
        &self.capabilities
    }
}

/// A failure to construct an import descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImportDescriptorError {
    /// An imported capability belonged to a package other than its slot.
    CapabilityPackageMismatch {
        /// The package serving as the import-slot identity.
        slot_id: BoxId,
        /// The mismatched qualified capability identity.
        capability: CapabilityId,
    },
    /// A capability identity appeared more than once in the import set.
    DuplicateCapability {
        /// The repeated capability identity.
        capability: CapabilityId,
    },
}

impl fmt::Display for ImportDescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityPackageMismatch {
                slot_id,
                capability,
            } => write!(
                formatter,
                "capability {capability} does not belong to import slot {slot_id}"
            ),
            Self::DuplicateCapability { capability } => {
                write!(formatter, "duplicate imported capability: {capability}")
            }
        }
    }
}

impl Error for ImportDescriptorError {}

/// Implementation-local registration data for one outward contract.
///
/// Construction happens at runtime from a contract owned by generated static
/// storage, so cloning this value shares rather than duplicates that contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationDescriptor {
    contract: &'static ContractDescriptor,
    imports: Vec<ImportDescriptor>,
}

impl ImplementationDescriptor {
    /// Constructs implementation registration while preserving import order.
    pub fn new(
        contract: &'static ContractDescriptor,
        imports: impl IntoIterator<Item = ImportDescriptor>,
    ) -> Result<Self, ImplementationDescriptorError> {
        let mut accepted: Vec<ImportDescriptor> = Vec::new();
        for import in imports {
            if accepted
                .iter()
                .any(|known| known.slot_id() == import.slot_id())
            {
                return Err(ImplementationDescriptorError::DuplicateImportSlot {
                    slot_id: import.slot_id,
                });
            }
            accepted.push(import);
        }
        Ok(Self {
            contract,
            imports: accepted,
        })
    }

    /// Returns the shared outward contract.
    pub fn contract(&self) -> &'static ContractDescriptor {
        self.contract
    }

    /// Returns implementation-private imports in their supplied order.
    pub fn imports(&self) -> &[ImportDescriptor] {
        &self.imports
    }
}

/// A failure to construct an implementation descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImplementationDescriptorError {
    /// An import-slot package appeared more than once.
    DuplicateImportSlot {
        /// The repeated package and slot identity.
        slot_id: BoxId,
    },
}

impl fmt::Display for ImplementationDescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateImportSlot { slot_id } => {
                write!(formatter, "duplicate import slot: {slot_id}")
            }
        }
    }
}

impl Error for ImplementationDescriptorError {}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::{
        CapabilityDescriptor, CapabilityName, CapabilityShape, ExposureLevel, Idempotency,
        TypeDescriptor,
    };

    fn box_id(value: &str) -> BoxId {
        BoxId::new(value).unwrap()
    }

    fn capability(package: &str, name: &str) -> CapabilityId {
        CapabilityId::new(box_id(package), CapabilityName::new(name).unwrap())
    }

    fn import(package: &str, revision: &str, names: &[&str]) -> ImportDescriptor {
        ImportDescriptor::new(
            box_id(package),
            ContractRevision::new(revision).unwrap(),
            names.iter().map(|name| capability(package, name)),
        )
        .unwrap()
    }

    static CONTRACT: LazyLock<ContractDescriptor> = LazyLock::new(|| {
        let id = capability("greeter", "greet");
        let descriptor = CapabilityDescriptor::new(
            id,
            TypeDescriptor::string(),
            TypeDescriptor::string(),
            TypeDescriptor::string(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        );
        ContractDescriptor::new(
            box_id("greeter"),
            [descriptor],
            ContractRevision::new("greeter-r1").unwrap(),
        )
        .unwrap()
    });

    #[test]
    fn import_view_preserves_revision_and_capability_order() {
        let descriptor = import("hello", "hello-r7", &["health", "greet"]);

        assert_eq!(descriptor.slot_id(), &box_id("hello"));
        assert_eq!(descriptor.expected_revision().as_str(), "hello-r7");
        assert_eq!(
            descriptor.capabilities(),
            &[capability("hello", "health"), capability("hello", "greet")]
        );
    }

    #[test]
    fn import_rejects_cross_package_capability_exactly() {
        let error = ImportDescriptor::new(
            box_id("hello"),
            ContractRevision::new("r1").unwrap(),
            [capability("other", "greet")],
        )
        .unwrap_err();
        assert_eq!(
            error,
            ImportDescriptorError::CapabilityPackageMismatch {
                slot_id: box_id("hello"),
                capability: capability("other", "greet"),
            }
        );
        assert_eq!(
            error.to_string(),
            "capability other.greet does not belong to import slot hello"
        );
    }

    #[test]
    fn import_rejects_duplicate_capability_exactly() {
        let repeated = capability("hello", "greet");
        let error = ImportDescriptor::new(
            box_id("hello"),
            ContractRevision::new("r1").unwrap(),
            [repeated.clone(), repeated],
        )
        .unwrap_err();
        assert_eq!(
            error,
            ImportDescriptorError::DuplicateCapability {
                capability: capability("hello", "greet"),
            }
        );
        assert_eq!(
            error.to_string(),
            "duplicate imported capability: hello.greet"
        );
    }

    #[test]
    fn implementation_shares_contract_and_preserves_import_order() {
        let hello = import("hello", "r1", &["greet"]);
        let audit = import("audit", "r2", &["record"]);
        let descriptor =
            ImplementationDescriptor::new(&CONTRACT, [hello.clone(), audit.clone()]).unwrap();

        assert!(std::ptr::eq(descriptor.contract(), &*CONTRACT));
        assert_eq!(descriptor.imports(), &[hello, audit]);
    }

    #[test]
    fn implementation_rejects_duplicate_slots_exactly() {
        let first = import("hello", "r1", &["greet"]);
        let second = import("hello", "r2", &["health"]);
        let error = ImplementationDescriptor::new(&CONTRACT, [first, second]).unwrap_err();

        assert_eq!(
            error,
            ImplementationDescriptorError::DuplicateImportSlot {
                slot_id: box_id("hello"),
            }
        );
        assert_eq!(error.to_string(), "duplicate import slot: hello");
    }

    #[test]
    fn private_import_changes_leave_outward_contract_unchanged() {
        let without_imports = ImplementationDescriptor::new(&CONTRACT, []).unwrap();
        let with_imports =
            ImplementationDescriptor::new(&CONTRACT, [import("hello", "r1", &["greet"])]).unwrap();

        assert!(std::ptr::eq(
            without_imports.contract(),
            with_imports.contract()
        ));
        assert_eq!(
            without_imports.contract().revision(),
            with_imports.contract().revision()
        );
        assert_ne!(without_imports.imports(), with_imports.imports());
    }

    #[test]
    fn implementation_descriptors_are_structural_static_plain_data() {
        fn assert_bounds<T: Send + Sync + 'static>() {}

        let build = || {
            ImplementationDescriptor::new(&CONTRACT, [import("hello", "r1", &["greet"])]).unwrap()
        };
        assert_eq!(build(), build());
        assert_eq!(build(), build().clone());
        assert_bounds::<ImportDescriptor>();
        assert_bounds::<ImportDescriptorError>();
        assert_bounds::<ImplementationDescriptor>();
        assert_bounds::<ImplementationDescriptorError>();
    }
}
