use std::error::Error;
use std::fmt;

/// A transport-neutral raw-value tree.
///
/// Objects preserve entry order and duplicate keys. Numbers preserve their
/// original RFC 8259 token without imposing a magnitude limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpaqueTree {
    Null,
    Bool(bool),
    Number(OpaqueNumber),
    String(String),
    List(Vec<OpaqueTree>),
    Object(Vec<(String, OpaqueTree)>),
}

/// A validated, exactly preserved RFC 8259 number token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueNumber(String);

impl OpaqueNumber {
    /// Validates and preserves an RFC 8259 number token.
    pub fn new(text: impl Into<String>) -> Result<Self, OpaqueNumberError> {
        let text = text.into();
        if valid_number(&text) {
            Ok(Self(text))
        } else {
            Err(OpaqueNumberError { text })
        }
    }

    /// Returns the original number token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An invalid RFC 8259 number token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueNumberError {
    text: String,
}

impl fmt::Display for OpaqueNumberError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid RFC 8259 number token: {:?}", self.text)
    }
}

impl Error for OpaqueNumberError {}

/// An opaque raw-value payload whose diagnostic representation is redacted.
#[derive(Clone, PartialEq)]
pub struct OpaquePayload(OpaqueTree);

impl OpaquePayload {
    /// Wraps a raw-value tree as an opaque payload.
    pub fn new(tree: OpaqueTree) -> Self {
        Self(tree)
    }

    /// Explicitly reveals the sensitive raw-value tree.
    ///
    /// Callers must treat the returned content as sensitive data.
    pub fn reveal(&self) -> &OpaqueTree {
        &self.0
    }

    /// Returns an independently owned redacted clone for onward transmission.
    ///
    /// This operation never reveals the payload content.
    pub fn forward(&self) -> OpaquePayload {
        self.clone()
    }
}

impl fmt::Debug for OpaquePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpaquePayload(<redacted>)")
    }
}

fn valid_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(index) {
        Some(b'0') => index += 1,
        Some(b'1'..=b'9') => {
            index += 1;
            while matches!(bytes.get(index), Some(b'0'..=b'9')) {
                index += 1;
            }
        }
        _ => return false,
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    index == bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_accept_exact_rfc_grammar_and_preserve_tokens() {
        let accepted = [
            "0",
            "-0",
            "10",
            "-42",
            "0.01",
            "1.0",
            "1e0",
            "1E+9",
            "-1.25e-999999",
            "123456789012345678901234567890",
        ];
        for token in accepted {
            assert_eq!(OpaqueNumber::new(token).unwrap().as_str(), token);
        }

        let rejected = [
            "", "-", "00", "01", "-01", ".1", "1.", "1e", "1e+", "+1", " 1", "1 ", "1x", "NaN",
        ];
        for token in rejected {
            let error = OpaqueNumber::new(token).unwrap_err();
            assert!(error.to_string().contains(token));
        }
    }

    #[test]
    fn object_order_and_duplicate_keys_are_preserved() {
        let tree = OpaqueTree::Object(vec![
            ("x".into(), OpaqueTree::Null),
            ("x".into(), OpaqueTree::Bool(true)),
            (
                "y".into(),
                OpaqueTree::Number(OpaqueNumber::new("1e2").unwrap()),
            ),
        ]);
        let OpaqueTree::Object(entries) = &tree else {
            panic!()
        };
        assert_eq!(entries[0].0, "x");
        assert_eq!(entries[1].0, "x");
        assert_eq!(entries[2].0, "y");
    }

    #[test]
    fn reveal_is_exact_and_forward_is_an_independent_redacted_clone() {
        let tree = OpaqueTree::String("explicitly-revealed".into());
        let payload = OpaquePayload::new(tree.clone());
        assert_eq!(payload.reveal(), &tree);
        assert!(format!("{:?}", payload.reveal()).contains("explicitly-revealed"));
        assert_eq!(format!("{payload:?}"), "OpaquePayload(<redacted>)");

        let forwarded = payload.forward();
        assert_eq!(forwarded, payload);
        let (OpaqueTree::String(original), OpaqueTree::String(clone)) =
            (payload.reveal(), forwarded.reveal())
        else {
            panic!()
        };
        assert_ne!(original.as_ptr(), clone.as_ptr());
        assert_eq!(format!("{forwarded:?}"), "OpaquePayload(<redacted>)");
    }

    #[test]
    fn public_opaque_types_are_send_sync_and_static() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        assert_bounds::<OpaqueTree>();
        assert_bounds::<OpaqueNumber>();
        assert_bounds::<OpaqueNumberError>();
        assert_bounds::<OpaquePayload>();
    }
}
