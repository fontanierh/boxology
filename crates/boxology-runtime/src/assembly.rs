//! Deterministic, payload-free composition-assembly diagnostics.

use std::error::Error;
use std::fmt;

use boxology_contract::{BoxId, CapabilityId, Detail, ExposureLevel};

/// One deterministic composition-assembly validation failure.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyError {
    /// A box identity was registered more than once.
    DuplicateBox {
        /// The repeated box identity.
        box_id: BoxId,
    },
    /// An import resolution named an unregistered consumer.
    UnknownImportConsumer {
        /// The unregistered consumer identity.
        consumer: BoxId,
    },
    /// An import resolution named an undeclared slot.
    UnknownImportSlot {
        /// The registered consumer identity.
        consumer: BoxId,
        /// The undeclared import-slot identity.
        slot: BoxId,
    },
    /// An import slot was resolved more than once.
    DuplicateImportResolution {
        /// The registered consumer identity.
        consumer: BoxId,
        /// The multiply resolved import-slot identity.
        slot: BoxId,
    },
    /// An import resolution named an unregistered target.
    UnknownImportTarget {
        /// The registered consumer identity.
        consumer: BoxId,
        /// The declared import-slot identity.
        slot: BoxId,
        /// The unregistered target identity.
        target: BoxId,
    },
    /// A declared import slot had no resolution.
    MissingImportResolution {
        /// The registered consumer identity.
        consumer: BoxId,
        /// The unresolved import-slot identity.
        slot: BoxId,
    },
    /// A resolved target lacked one declared imported capability.
    MissingImportedCapability {
        /// The registered consumer identity.
        consumer: BoxId,
        /// The declared import-slot identity.
        slot: BoxId,
        /// The capability absent from the resolved target contract.
        capability: CapabilityId,
    },
    /// An exposure named an unregistered provider.
    UnknownExposureProvider {
        /// The unregistered provider identity.
        provider: BoxId,
    },
    /// An exposure named a capability absent from its provider contract.
    UnknownExposedCapability {
        /// The registered provider identity.
        provider: BoxId,
        /// The absent capability identity.
        capability: CapabilityId,
    },
    /// An exposure requested a boundary wider than the capability permits.
    ExposureExceedsMaximum {
        /// The capability being exposed.
        capability: CapabilityId,
        /// The requested exposure boundary.
        requested: ExposureLevel,
        /// The greatest permitted exposure boundary.
        maximum: ExposureLevel,
    },
    /// A transport rejected one otherwise valid exposure.
    TransportConformanceFailed {
        /// The rejected capability.
        capability: CapabilityId,
        /// Producer-owned payload-safe diagnostic detail.
        detail: Detail,
    },
    /// A transport failed transactional descriptor preflight.
    TransportPrepareFailed {
        /// Producer-owned payload-safe diagnostic detail.
        detail: Detail,
    },
    /// A transport failed transactional startup.
    TransportStartFailed {
        /// Producer-owned payload-safe diagnostic detail.
        detail: Detail,
    },
}

impl fmt::Display for AssemblyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateBox { box_id } => {
                write!(formatter, "duplicate box registration: {box_id}")
            }
            Self::UnknownImportConsumer { consumer } => {
                write!(formatter, "unknown import consumer: {consumer}")
            }
            Self::UnknownImportSlot { consumer, slot } => {
                write!(
                    formatter,
                    "unknown import slot {slot} for consumer {consumer}"
                )
            }
            Self::DuplicateImportResolution { consumer, slot } => write!(
                formatter,
                "duplicate import resolution for consumer {consumer}, slot {slot}"
            ),
            Self::UnknownImportTarget {
                consumer,
                slot,
                target,
            } => write!(
                formatter,
                "unknown import target {target} for consumer {consumer}, slot {slot}"
            ),
            Self::MissingImportResolution { consumer, slot } => write!(
                formatter,
                "missing import resolution for consumer {consumer}, slot {slot}"
            ),
            Self::MissingImportedCapability {
                consumer,
                slot,
                capability,
            } => write!(
                formatter,
                "import target for consumer {consumer}, slot {slot} is missing capability {capability}"
            ),
            Self::UnknownExposureProvider { provider } => {
                write!(formatter, "unknown exposure provider: {provider}")
            }
            Self::UnknownExposedCapability {
                provider,
                capability,
            } => write!(
                formatter,
                "unknown exposed capability {capability} for provider {provider}"
            ),
            Self::ExposureExceedsMaximum {
                capability,
                requested,
                maximum,
            } => write!(
                formatter,
                "exposure {} exceeds maximum {} for capability {capability}",
                level_name(*requested),
                level_name(*maximum)
            ),
            Self::TransportConformanceFailed { capability, detail } => write!(
                formatter,
                "transport conformance failed for capability {capability}: {detail}"
            ),
            Self::TransportPrepareFailed { detail } => {
                write!(formatter, "transport prepare failed: {detail}")
            }
            Self::TransportStartFailed { detail } => {
                write!(formatter, "transport start failed: {detail}")
            }
        }
    }
}

fn level_name(level: ExposureLevel) -> &'static str {
    match level {
        ExposureLevel::CodeOnly => "code-only",
        ExposureLevel::Internal => "internal",
        ExposureLevel::External => "external",
    }
}

impl Error for AssemblyError {}

/// A nonempty, ordered collection of composition-assembly failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssemblyErrors(Box<[AssemblyError]>);

impl AssemblyErrors {
    /// Returns failures in deterministic validation order.
    pub fn errors(&self) -> &[AssemblyError] {
        &self.0
    }

    pub(crate) fn from_errors(errors: Vec<AssemblyError>) -> Option<Self> {
        (!errors.is_empty()).then(|| Self(errors.into_boxed_slice()))
    }
}

impl fmt::Display for AssemblyErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.errors().iter().enumerate() {
            if index != 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "{error}")?;
        }
        Ok(())
    }
}

impl Error for AssemblyErrors {}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::CapabilityName;

    fn box_id(value: &str) -> BoxId {
        BoxId::new(value).unwrap()
    }

    fn capability(package: &str, name: &str) -> CapabilityId {
        CapabilityId::new(box_id(package), CapabilityName::new(name).unwrap())
    }

    fn failure_cases() -> [(AssemblyError, &'static str); 13] {
        [
            (
                AssemblyError::DuplicateBox {
                    box_id: box_id("duplicate"),
                },
                "duplicate box registration: duplicate",
            ),
            (
                AssemblyError::UnknownImportConsumer {
                    consumer: box_id("unknown-consumer"),
                },
                "unknown import consumer: unknown-consumer",
            ),
            (
                AssemblyError::UnknownImportSlot {
                    consumer: box_id("consumer"),
                    slot: box_id("unknown-slot"),
                },
                "unknown import slot unknown-slot for consumer consumer",
            ),
            (
                AssemblyError::DuplicateImportResolution {
                    consumer: box_id("consumer"),
                    slot: box_id("duplicate-slot"),
                },
                "duplicate import resolution for consumer consumer, slot duplicate-slot",
            ),
            (
                AssemblyError::UnknownImportTarget {
                    consumer: box_id("consumer"),
                    slot: box_id("slot"),
                    target: box_id("unknown-target"),
                },
                "unknown import target unknown-target for consumer consumer, slot slot",
            ),
            (
                AssemblyError::MissingImportResolution {
                    consumer: box_id("consumer"),
                    slot: box_id("missing-slot"),
                },
                "missing import resolution for consumer consumer, slot missing-slot",
            ),
            (
                AssemblyError::MissingImportedCapability {
                    consumer: box_id("consumer"),
                    slot: box_id("slot"),
                    capability: capability("target", "needed"),
                },
                "import target for consumer consumer, slot slot is missing capability target.needed",
            ),
            (
                AssemblyError::UnknownExposureProvider {
                    provider: box_id("unknown-provider"),
                },
                "unknown exposure provider: unknown-provider",
            ),
            (
                AssemblyError::UnknownExposedCapability {
                    provider: box_id("provider"),
                    capability: capability("provider", "unknown"),
                },
                "unknown exposed capability provider.unknown for provider provider",
            ),
            (
                AssemblyError::ExposureExceedsMaximum {
                    capability: capability("provider", "limited"),
                    requested: ExposureLevel::External,
                    maximum: ExposureLevel::Internal,
                },
                "exposure external exceeds maximum internal for capability provider.limited",
            ),
            (
                AssemblyError::TransportConformanceFailed {
                    capability: capability("provider", "rejected"),
                    detail: Detail::new("test_conformance"),
                },
                "transport conformance failed for capability provider.rejected: test_conformance",
            ),
            (
                AssemblyError::TransportPrepareFailed {
                    detail: Detail::new("test_prepare"),
                },
                "transport prepare failed: test_prepare",
            ),
            (
                AssemblyError::TransportStartFailed {
                    detail: Detail::new("test_start"),
                },
                "transport start failed: test_start",
            ),
        ]
    }

    #[test]
    fn every_failure_is_cloneable_equal_and_has_exact_display() {
        for (error, expected) in failure_cases() {
            assert_eq!(error, error.clone());
            assert_eq!(error.to_string(), expected);
        }
        assert_eq!(level_name(ExposureLevel::CodeOnly), "code-only");
    }

    #[test]
    fn diagnostics_satisfy_error_and_thread_safety_bounds() {
        fn assert_error<T: Error>() {}
        fn assert_bounds<T: Send + Sync + 'static>() {}

        assert_error::<AssemblyError>();
        assert_error::<AssemblyErrors>();
        assert_bounds::<AssemblyError>();
        assert_bounds::<AssemblyErrors>();
    }

    #[test]
    fn empty_failure_collection_is_absent() {
        assert_eq!(AssemblyErrors::from_errors(Vec::new()), None);
    }

    #[test]
    fn aggregate_preserves_deliberately_shuffled_order_exactly() {
        let cases = failure_cases();
        let shuffled = [12, 6, 0, 4, 5, 3, 1, 2, 9, 7, 11, 8, 10];
        let ordered = shuffled.map(|index| cases[index].0.clone()).to_vec();
        let aggregate = AssemblyErrors::from_errors(ordered.clone()).unwrap();

        assert_eq!(aggregate.errors(), ordered);
        assert_eq!(aggregate, aggregate.clone());
        let expected = shuffled.map(|index| cases[index].1).join("\n");
        let rendered = aggregate.to_string();
        assert_eq!(rendered, expected);
        assert!(!rendered.ends_with('\n'));
    }
}
