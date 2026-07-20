//! Deterministic, payload-free composition-assembly diagnostics.

use std::error::Error;
use std::fmt;

use boxology_contract::{BoxId, CapabilityId};

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
        }
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

    #[test]
    fn every_failure_is_cloneable_equal_and_has_exact_display() {
        let cases = [
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
        ];

        for (error, expected) in cases {
            assert_eq!(error, error.clone());
            assert_eq!(error.to_string(), expected);
        }
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
        let ordered = vec![
            AssemblyError::MissingImportedCapability {
                consumer: box_id("consumer-seven"),
                slot: box_id("slot-seven"),
                capability: capability("target-seven", "needed_seven"),
            },
            AssemblyError::DuplicateBox {
                box_id: box_id("duplicate-one"),
            },
            AssemblyError::UnknownImportTarget {
                consumer: box_id("consumer-five"),
                slot: box_id("slot-five"),
                target: box_id("target-five"),
            },
            AssemblyError::MissingImportResolution {
                consumer: box_id("consumer-six"),
                slot: box_id("slot-six"),
            },
            AssemblyError::DuplicateImportResolution {
                consumer: box_id("consumer-four"),
                slot: box_id("slot-four"),
            },
            AssemblyError::UnknownImportConsumer {
                consumer: box_id("consumer-two"),
            },
            AssemblyError::UnknownImportSlot {
                consumer: box_id("consumer-three"),
                slot: box_id("slot-three"),
            },
        ];
        let aggregate = AssemblyErrors::from_errors(ordered.clone()).unwrap();

        assert_eq!(aggregate.errors(), ordered);
        assert_eq!(aggregate, aggregate.clone());
        let expected = [
            "import target for consumer consumer-seven, slot slot-seven is missing capability target-seven.needed_seven",
            "duplicate box registration: duplicate-one",
            "unknown import target target-five for consumer consumer-five, slot slot-five",
            "missing import resolution for consumer consumer-six, slot slot-six",
            "duplicate import resolution for consumer consumer-four, slot slot-four",
            "unknown import consumer: consumer-two",
            "unknown import slot slot-three for consumer consumer-three",
        ]
        .join("\n");
        let rendered = aggregate.to_string();
        assert_eq!(rendered, expected);
        assert!(!rendered.ends_with('\n'));
    }
}
