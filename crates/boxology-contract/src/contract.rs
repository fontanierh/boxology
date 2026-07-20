//! Outward, importless contract and capability descriptors.

use std::error::Error;
use std::fmt;

use crate::{BoxId, CapabilityId, CapabilityName, ContractRevision, Deprecation, TypeDescriptor};

/// A capability's declared interaction shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityShape {
    /// One request produces one response.
    Unary,
    /// One request produces a stream of responses.
    ServerStreaming,
    /// A stream of requests produces one response.
    ClientStreaming,
    /// Request and response streams proceed independently.
    BidirectionalStreaming,
    /// A request subscribes to a stream of events.
    EventSubscription,
}

/// The greatest exposure a capability permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ExposureLevel {
    /// Calls remain inside code composition.
    CodeOnly,
    /// Calls may cross an internal service boundary.
    Internal,
    /// Calls may cross an external service boundary.
    External,
}

/// A capability's declared idempotency property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Idempotency {
    /// No idempotency property is declared.
    None,
    /// Repeating the operation has the same effect as performing it once.
    Inherent,
}

/// The complete outward description of one capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDescriptor {
    id: CapabilityId,
    input: TypeDescriptor,
    output: TypeDescriptor,
    error: TypeDescriptor,
    shape: CapabilityShape,
    max_exposure: ExposureLevel,
    idempotency: Idempotency,
    deprecation: Option<Deprecation>,
}

impl CapabilityDescriptor {
    /// Constructs an owned capability descriptor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: CapabilityId,
        input: TypeDescriptor,
        output: TypeDescriptor,
        error: TypeDescriptor,
        shape: CapabilityShape,
        max_exposure: ExposureLevel,
        idempotency: Idempotency,
        deprecation: Option<Deprecation>,
    ) -> Self {
        Self {
            id,
            input,
            output,
            error,
            shape,
            max_exposure,
            idempotency,
            deprecation,
        }
    }

    /// Returns the box-local name from the qualified identity.
    pub fn name(&self) -> &CapabilityName {
        self.id.name()
    }

    /// Returns the box-qualified identity.
    pub fn id(&self) -> &CapabilityId {
        &self.id
    }

    /// Returns the input type slot.
    pub fn input(&self) -> &TypeDescriptor {
        &self.input
    }

    /// Returns the output type slot.
    pub fn output(&self) -> &TypeDescriptor {
        &self.output
    }

    /// Returns the structured error type slot.
    pub fn error(&self) -> &TypeDescriptor {
        &self.error
    }

    /// Returns the interaction shape.
    pub fn shape(&self) -> CapabilityShape {
        self.shape
    }

    /// Returns the maximum permitted exposure.
    pub fn max_exposure(&self) -> ExposureLevel {
        self.max_exposure
    }

    /// Returns the declared idempotency property.
    pub fn idempotency(&self) -> Idempotency {
        self.idempotency
    }

    /// Returns the optional deprecation metadata.
    pub fn deprecation(&self) -> Option<&Deprecation> {
        self.deprecation.as_ref()
    }
}

/// One box's ordered, outward contract without implementation imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractDescriptor {
    box_id: BoxId,
    capabilities: Vec<CapabilityDescriptor>,
    revision: ContractRevision,
}

impl ContractDescriptor {
    /// Constructs a contract after validating capability ownership and uniqueness.
    pub fn new(
        box_id: BoxId,
        capabilities: impl IntoIterator<Item = CapabilityDescriptor>,
        revision: ContractRevision,
    ) -> Result<Self, ContractDescriptorError> {
        let mut accepted: Vec<CapabilityDescriptor> = Vec::new();
        for capability in capabilities {
            if capability.id().box_id() != &box_id {
                return Err(ContractDescriptorError::CapabilityBoxMismatch {
                    contract_box: box_id,
                    capability: capability.id,
                });
            }
            if accepted.iter().any(|known| known.id() == capability.id()) {
                return Err(ContractDescriptorError::DuplicateCapability {
                    capability: capability.id,
                });
            }
            accepted.push(capability);
        }
        Ok(Self {
            box_id,
            capabilities: accepted,
            revision,
        })
    }

    /// Returns the box identity.
    pub fn box_id(&self) -> &BoxId {
        &self.box_id
    }

    /// Returns capabilities in declared order.
    pub fn capabilities(&self) -> &[CapabilityDescriptor] {
        &self.capabilities
    }

    /// Returns the opaque contract revision.
    pub fn revision(&self) -> &ContractRevision {
        &self.revision
    }
}

/// A failure to construct an outward contract descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContractDescriptorError {
    /// A capability belonged to a different box.
    CapabilityBoxMismatch {
        /// The box whose contract was being constructed.
        contract_box: BoxId,
        /// The mismatched qualified capability identity.
        capability: CapabilityId,
    },
    /// A qualified capability identity appeared more than once.
    DuplicateCapability {
        /// The repeated capability identity.
        capability: CapabilityId,
    },
}

impl fmt::Display for ContractDescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityBoxMismatch {
                contract_box,
                capability,
            } => write!(
                formatter,
                "capability {capability} does not belong to contract box {contract_box}"
            ),
            Self::DuplicateCapability { capability } => {
                write!(formatter, "duplicate capability: {capability}")
            }
        }
    }
}

impl Error for ContractDescriptorError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DescriptorRef;

    fn id(box_name: &str, capability: &str) -> CapabilityId {
        CapabilityId::new(
            BoxId::new(box_name).unwrap(),
            CapabilityName::new(capability).unwrap(),
        )
    }

    fn capability(
        id: CapabilityId,
        shape: CapabilityShape,
        idempotency: Idempotency,
    ) -> CapabilityDescriptor {
        CapabilityDescriptor::new(
            id,
            TypeDescriptor::string(),
            TypeDescriptor::u64(),
            TypeDescriptor::bool(),
            shape,
            ExposureLevel::Internal,
            idempotency,
            (idempotency == Idempotency::None)
                .then(|| Deprecation::new(Some("use replacement".into()))),
        )
    }

    #[test]
    fn complete_outward_view_preserves_capability_order() {
        let box_id = BoxId::new("billing").unwrap();
        let first = capability(
            id("billing", "quote"),
            CapabilityShape::Unary,
            Idempotency::None,
        );
        let second = capability(
            id("billing", "invoice"),
            CapabilityShape::ServerStreaming,
            Idempotency::Inherent,
        );
        let revision = ContractRevision::new("sha256:123").unwrap();
        let contract = ContractDescriptor::new(
            box_id.clone(),
            [first.clone(), second.clone()],
            revision.clone(),
        )
        .unwrap();

        assert_eq!(contract.box_id(), &box_id);
        assert_eq!(contract.revision(), &revision);
        assert_eq!(contract.capabilities(), &[first, second]);
        let viewed = &contract.capabilities()[0];
        assert_eq!(viewed.name().as_str(), "quote");
        assert_eq!(viewed.id(), &id("billing", "quote"));
        assert_eq!(viewed.input().view(), DescriptorRef::String);
        assert_eq!(viewed.output().view(), DescriptorRef::U64);
        assert_eq!(viewed.error().view(), DescriptorRef::Bool);
        assert_eq!(viewed.shape(), CapabilityShape::Unary);
        assert_eq!(viewed.max_exposure(), ExposureLevel::Internal);
        assert_eq!(viewed.idempotency(), Idempotency::None);
        assert_eq!(
            viewed.deprecation().unwrap().note(),
            Some("use replacement")
        );
        assert_eq!(contract.capabilities()[1].deprecation(), None);
    }

    #[test]
    fn outward_contract_construction_requires_no_implementation_data() {
        let contract = ContractDescriptor::new(
            BoxId::new("empty").unwrap(),
            [],
            ContractRevision::new("r1").unwrap(),
        )
        .unwrap();
        assert!(contract.capabilities().is_empty());
    }

    #[test]
    fn every_interaction_shape_and_idempotency_variant_is_constructible() {
        let shapes = [
            CapabilityShape::Unary,
            CapabilityShape::ServerStreaming,
            CapabilityShape::ClientStreaming,
            CapabilityShape::BidirectionalStreaming,
            CapabilityShape::EventSubscription,
        ];
        for (index, shape) in shapes.into_iter().enumerate() {
            let idempotency = if index % 2 == 0 {
                Idempotency::None
            } else {
                Idempotency::Inherent
            };
            let descriptor = capability(id("box", &format!("cap_{index}")), shape, idempotency);
            assert_eq!(descriptor.shape(), shape);
            assert_eq!(descriptor.idempotency(), idempotency);
        }
    }

    #[test]
    fn exposure_levels_form_the_declared_lattice() {
        assert!(ExposureLevel::CodeOnly < ExposureLevel::Internal);
        assert!(ExposureLevel::Internal < ExposureLevel::External);
    }

    #[test]
    fn contract_rejects_identity_mismatch_exactly() {
        let error = ContractDescriptor::new(
            BoxId::new("expected").unwrap(),
            [capability(
                id("other", "run"),
                CapabilityShape::Unary,
                Idempotency::None,
            )],
            ContractRevision::new("r1").unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            ContractDescriptorError::CapabilityBoxMismatch {
                contract_box: BoxId::new("expected").unwrap(),
                capability: id("other", "run"),
            }
        );
        assert_eq!(
            error.to_string(),
            "capability other.run does not belong to contract box expected"
        );
    }

    #[test]
    fn contract_rejects_duplicate_capabilities_exactly() {
        let duplicate = capability(id("box", "run"), CapabilityShape::Unary, Idempotency::None);
        let error = ContractDescriptor::new(
            BoxId::new("box").unwrap(),
            [duplicate.clone(), duplicate],
            ContractRevision::new("r1").unwrap(),
        )
        .unwrap_err();
        assert_eq!(
            error,
            ContractDescriptorError::DuplicateCapability {
                capability: id("box", "run"),
            }
        );
        assert_eq!(error.to_string(), "duplicate capability: box.run");
    }

    #[test]
    fn descriptors_are_structurally_equal_owned_plain_data() {
        fn assert_bounds<T: Send + Sync + 'static>() {}

        let build = || {
            ContractDescriptor::new(
                BoxId::new("box").unwrap(),
                [capability(
                    id("box", "run"),
                    CapabilityShape::Unary,
                    Idempotency::None,
                )],
                ContractRevision::new("r1").unwrap(),
            )
            .unwrap()
        };
        assert_eq!(build(), build());
        assert_bounds::<CapabilityShape>();
        assert_bounds::<ExposureLevel>();
        assert_bounds::<Idempotency>();
        assert_bounds::<CapabilityDescriptor>();
        assert_bounds::<ContractDescriptor>();
        assert_bounds::<ContractDescriptorError>();
    }
}
