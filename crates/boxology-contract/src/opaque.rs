use std::error::Error;
use std::fmt;

use crate::{ContractValue, SlotValue, ValueRef};

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

    /// Captures a slot without assuming its descriptor or transport syntax.
    ///
    /// `Missing` and `Null` both become `OpaqueTree::Null`: their distinction
    /// is intentionally lost when the payload shape is unknown.
    pub(crate) fn capture(slot: &SlotValue) -> Self {
        Self(capture_slot(slot))
    }
}

impl fmt::Debug for OpaquePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpaquePayload(<redacted>)")
    }
}

fn capture_slot(slot: &SlotValue) -> OpaqueTree {
    match slot {
        SlotValue::Missing | SlotValue::Null => OpaqueTree::Null,
        SlotValue::Value(value) => capture_value(value),
    }
}

fn capture_value(value: &ContractValue) -> OpaqueTree {
    match value.view() {
        ValueRef::Null => OpaqueTree::Null,
        ValueRef::Bool(value) => OpaqueTree::Bool(value),
        ValueRef::I64(value) => number(value.to_string()),
        ValueRef::U64(value) => number(value.to_string()),
        ValueRef::F32(value) => number(value.to_string()),
        ValueRef::F64(value) => number(value.to_string()),
        ValueRef::String(value) => OpaqueTree::String(value.into()),
        ValueRef::Bytes(value) => OpaqueTree::Object(vec![(
            "base64".into(),
            OpaqueTree::String(standard_base64(value)),
        )]),
        ValueRef::List(values) => OpaqueTree::List(values.iter().map(capture_value).collect()),
        ValueRef::Object(object) => OpaqueTree::Object(
            object
                .entries()
                .map(|(key, value)| (key.into(), capture_value(value)))
                .collect(),
        ),
        ValueRef::Enum { tag, payload } => OpaqueTree::Object(vec![
            ("tag".into(), OpaqueTree::String(tag.into())),
            ("payload".into(), capture_slot(payload)),
        ]),
        ValueRef::Opaque(payload) => payload.reveal().clone(),
        ValueRef::Sensitive(inner) => capture_value(inner),
    }
}

fn number(text: String) -> OpaqueTree {
    OpaqueTree::Number(
        OpaqueNumber::new(text).expect("finite contract numbers format as RFC 8259 tokens"),
    )
}

fn standard_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(third & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
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
    use crate::{ContractValue, SlotValue};

    fn value(value: ContractValue) -> SlotValue {
        SlotValue::Value(value)
    }

    fn captured(slot: &SlotValue) -> OpaqueTree {
        OpaquePayload::capture(slot).reveal().clone()
    }

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

    #[test]
    fn capture_covers_slot_states_and_every_scalar_kind() {
        assert_eq!(captured(&SlotValue::Missing), OpaqueTree::Null);
        assert_eq!(captured(&SlotValue::Null), OpaqueTree::Null);
        assert_eq!(captured(&value(ContractValue::null())), OpaqueTree::Null);
        assert_eq!(
            captured(&value(ContractValue::bool(true))),
            OpaqueTree::Bool(true)
        );
        assert_eq!(
            captured(&value(ContractValue::string("text"))),
            OpaqueTree::String("text".into())
        );

        let cases = [
            (ContractValue::i64(i64::MIN), i64::MIN.to_string()),
            (ContractValue::u64(u64::MAX), u64::MAX.to_string()),
            (ContractValue::f32(-0.0).unwrap(), "-0".into()),
            (ContractValue::f64(1.5).unwrap(), "1.5".into()),
        ];
        for (input, token) in cases {
            let OpaqueTree::Number(number) = captured(&value(input)) else {
                panic!()
            };
            assert_eq!(number.as_str(), token);
            assert!(OpaqueNumber::new(token).is_ok());
        }
    }

    #[test]
    fn capture_uses_standard_padded_base64_for_bytes() {
        let cases: &[(&[u8], &str)] = &[
            (b"", ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"foobar", "Zm9vYmFy"),
            (&[0xfb, 0xff], "+/8="),
        ];
        for (bytes, encoded) in cases {
            assert_eq!(
                captured(&value(ContractValue::bytes(bytes.to_vec()))),
                OpaqueTree::Object(vec![(
                    "base64".into(),
                    OpaqueTree::String((*encoded).into()),
                )])
            );
        }
    }

    #[test]
    fn capture_recurses_through_lists_objects_and_enum_payloads_in_order() {
        let input = ContractValue::object([
            (
                "list".into(),
                ContractValue::list([ContractValue::bool(false), ContractValue::string("item")]),
            ),
            (
                "enum".into(),
                ContractValue::enum_value(
                    "known",
                    value(
                        ContractValue::object([("inner".into(), ContractValue::i64(3))]).unwrap(),
                    ),
                ),
            ),
            (
                "unit".into(),
                ContractValue::enum_value("unit", SlotValue::Missing),
            ),
        ])
        .unwrap();
        let expected = OpaqueTree::Object(vec![
            (
                "list".into(),
                OpaqueTree::List(vec![
                    OpaqueTree::Bool(false),
                    OpaqueTree::String("item".into()),
                ]),
            ),
            (
                "enum".into(),
                OpaqueTree::Object(vec![
                    ("tag".into(), OpaqueTree::String("known".into())),
                    (
                        "payload".into(),
                        OpaqueTree::Object(vec![(
                            "inner".into(),
                            OpaqueTree::Number(OpaqueNumber::new("3").unwrap()),
                        )]),
                    ),
                ]),
            ),
            (
                "unit".into(),
                OpaqueTree::Object(vec![
                    ("tag".into(), OpaqueTree::String("unit".into())),
                    ("payload".into(), OpaqueTree::Null),
                ]),
            ),
        ]);
        assert_eq!(captured(&value(input)), expected);
    }

    #[test]
    fn capture_splices_opaque_and_redacts_sensitive_values() {
        const SENTINEL: &str = "captured-sensitive-runtime-value";
        let raw = OpaqueTree::Object(vec![
            ("duplicate".into(), OpaqueTree::String(SENTINEL.into())),
            ("duplicate".into(), OpaqueTree::Bool(true)),
        ]);
        let captured_opaque = OpaquePayload::capture(&value(ContractValue::opaque(
            OpaquePayload::new(raw.clone()),
        )));
        assert_eq!(captured_opaque.reveal(), &raw);

        let captured_sensitive = OpaquePayload::capture(&value(ContractValue::sensitive(
            ContractValue::string(SENTINEL),
        )));
        assert_eq!(
            captured_sensitive.reveal(),
            &OpaqueTree::String(SENTINEL.into())
        );
        let diagnostics = [
            format!("{captured_opaque:?}"),
            format!("{captured_sensitive:?}"),
            format!(
                "{:?}",
                SlotValue::Value(ContractValue::opaque(captured_sensitive.forward()))
            ),
        ];
        for diagnostic in diagnostics {
            assert!(!diagnostic.contains(SENTINEL));
            assert!(diagnostic.contains("<redacted>"));
        }
    }
}
