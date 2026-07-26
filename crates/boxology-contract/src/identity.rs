use std::error::Error;
use std::fmt;

/// A stable box, package, import-slot, and derived-output identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BoxId(String);

impl BoxId {
    /// Validates and constructs an identifier matching `[a-z][a-z0-9-]*`.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        if valid_identifier(&value, b'-') {
            Ok(Self(value))
        } else {
            Err(IdentityError::InvalidBoxId { value })
        }
    }

    /// Returns the identifier spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BoxId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A capability's box-local name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CapabilityName(String);

impl CapabilityName {
    /// Validates and constructs a name matching `[a-z][a-z0-9_]*`.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        if valid_identifier(&value, b'_') {
            Ok(Self(value))
        } else {
            Err(IdentityError::InvalidCapabilityName { value })
        }
    }

    /// Returns the local name spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapabilityName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A capability's box-qualified identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CapabilityId {
    box_id: BoxId,
    name: CapabilityName,
}

impl CapabilityId {
    /// Joins validated box and local-name segments.
    pub fn new(box_id: BoxId, name: CapabilityName) -> Self {
        Self { box_id, name }
    }

    /// Returns the box segment.
    pub fn box_id(&self) -> &BoxId {
        &self.box_id
    }

    /// Returns the box-local capability segment.
    pub fn name(&self) -> &CapabilityName {
        &self.name
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.box_id, self.name)
    }
}

/// An opaque identifier for one generated contract state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContractRevision(String);

impl ContractRevision {
    /// Constructs a non-empty revision without interpreting its format.
    pub fn new(value: impl Into<String>) -> Result<Self, IdentityError> {
        let value = value.into();
        if value.is_empty() {
            Err(IdentityError::EmptyRevision)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the exact revision spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContractRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A failure to construct a contract identity.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdentityError {
    /// A box identifier did not match `[a-z][a-z0-9-]*`.
    InvalidBoxId {
        /// The rejected spelling.
        value: String,
    },
    /// A capability name did not match `[a-z][a-z0-9_]*`.
    InvalidCapabilityName {
        /// The rejected spelling.
        value: String,
    },
    /// A contract revision was empty.
    EmptyRevision,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBoxId { value } => write!(formatter, "invalid box id: {value:?}"),
            Self::InvalidCapabilityName { value } => {
                write!(formatter, "invalid capability name: {value:?}")
            }
            Self::EmptyRevision => formatter.write_str("contract revision must not be empty"),
        }
    }
}

impl Error for IdentityError {}

fn valid_identifier(value: &str, separator: u8) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == separator)
}

#[cfg(test)]
mod tests {
    use super::{BoxId, CapabilityId, CapabilityName, ContractRevision, IdentityError};

    #[test]
    fn box_id_grammar_is_exact() {
        for valid in ["a", "box", "box-1", "a0-b9", "box-", "a--b"] {
            let id = BoxId::new(valid).unwrap();
            assert_eq!(id.as_str(), valid);
            assert_eq!(id.to_string(), valid);
        }
        for invalid in [
            "", "A", "Box", "boX", "1box", "-box", "_box", "box%", "box name", "box.name",
            "box_name", "boéx",
        ] {
            assert_eq!(
                BoxId::new(invalid),
                Err(IdentityError::InvalidBoxId {
                    value: invalid.into()
                })
            );
        }
    }

    #[test]
    fn capability_name_grammar_is_exact() {
        for valid in ["a", "capability", "capability_1", "a0_b9", "name_", "a__b"] {
            let name = CapabilityName::new(valid).unwrap();
            assert_eq!(name.as_str(), valid);
            assert_eq!(name.to_string(), valid);
        }
        for invalid in [
            "",
            "A",
            "Name",
            "naMe",
            "1name",
            "_name",
            "-name",
            "name%",
            "name space",
            "name.part",
            "name-part",
            "naïve",
        ] {
            assert_eq!(
                CapabilityName::new(invalid),
                Err(IdentityError::InvalidCapabilityName {
                    value: invalid.into()
                })
            );
        }
    }

    #[test]
    fn capability_id_joins_and_exposes_both_segments() {
        let box_id = BoxId::new("billing-box").unwrap();
        let name = CapabilityName::new("create_invoice").unwrap();
        let id = CapabilityId::new(box_id.clone(), name.clone());

        assert_eq!(id.box_id(), &box_id);
        assert_eq!(id.name(), &name);
        assert_eq!(id.to_string(), "billing-box.create_invoice");
    }

    #[test]
    fn contract_revision_is_non_empty_and_otherwise_opaque() {
        assert_eq!(ContractRevision::new(""), Err(IdentityError::EmptyRevision));
        for spelling in ["sha256:abc/DEF", " vNEXT ", " "] {
            let revision = ContractRevision::new(spelling).unwrap();
            assert_eq!(revision.as_str(), spelling);
            assert_eq!(revision.to_string(), spelling);
        }
    }

    #[test]
    fn identity_errors_are_equal_and_display_rejected_values() {
        let box_error = BoxId::new("Bad.Box").unwrap_err();
        assert_eq!(
            box_error,
            IdentityError::InvalidBoxId {
                value: "Bad.Box".into()
            }
        );
        assert_eq!(box_error.to_string(), "invalid box id: \"Bad.Box\"");

        let name_error = CapabilityName::new("Bad-Name").unwrap_err();
        assert_eq!(
            name_error,
            IdentityError::InvalidCapabilityName {
                value: "Bad-Name".into()
            }
        );
        assert_eq!(
            name_error.to_string(),
            "invalid capability name: \"Bad-Name\""
        );
        assert_eq!(
            IdentityError::EmptyRevision.to_string(),
            "contract revision must not be empty"
        );
    }

    #[test]
    fn identity_types_have_public_thread_safe_static_bounds() {
        fn assert_bounds<T: Send + Sync + 'static>() {}

        assert_bounds::<BoxId>();
        assert_bounds::<CapabilityName>();
        assert_bounds::<CapabilityId>();
        assert_bounds::<ContractRevision>();
        assert_bounds::<IdentityError>();
    }
}
