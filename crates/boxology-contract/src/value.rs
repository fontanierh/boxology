use std::error::Error;
use std::fmt;

use crate::OpaquePayload;

/// A failure to construct a contract value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueError {
    /// An `f32` was NaN or infinite.
    NonFiniteF32,
    /// An `f64` was NaN or infinite.
    NonFiniteF64,
    /// An object contained the same key more than once.
    DuplicateObjectKey { key: String },
}

impl fmt::Display for ValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteF32 => formatter.write_str("f32 contract values must be finite"),
            Self::NonFiniteF64 => formatter.write_str("f64 contract values must be finite"),
            Self::DuplicateObjectKey { key } => {
                write!(formatter, "duplicate object key: {key:?}")
            }
        }
    }
}

impl Error for ValueError {}

/// A value crossing a top-level call slot.
///
/// `Null` and `Value(ContractValue::null())` are deliberately distinct. The
/// latter is useful when a descriptor-guided nested value is carried as data.
#[derive(Debug, Clone, PartialEq)]
pub enum SlotValue {
    /// No value was supplied.
    Missing,
    /// An explicit top-level null was supplied.
    Null,
    /// A present contract value was supplied.
    Value(ContractValue),
}

/// A transport-neutral contract value with an invariant-preserving private representation.
///
/// All floating-point values are finite, so structural equality is reflexive
/// in practice. IEEE equality still treats `0.0` and `-0.0` as equal.
#[derive(Clone, PartialEq)]
pub struct ContractValue {
    repr: Repr,
}

#[derive(Clone, PartialEq)]
enum Repr {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<ContractValue>),
    Object(Vec<(String, ContractValue)>),
    Enum {
        tag: String,
        payload: Box<SlotValue>,
    },
    Opaque(OpaquePayload),
    Sensitive(Box<ContractValue>),
}

impl fmt::Debug for ContractValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            Repr::Null => formatter.write_str("Null"),
            Repr::Bool(value) => formatter.debug_tuple("Bool").field(value).finish(),
            Repr::I64(value) => formatter.debug_tuple("I64").field(value).finish(),
            Repr::U64(value) => formatter.debug_tuple("U64").field(value).finish(),
            Repr::F32(value) => formatter.debug_tuple("F32").field(value).finish(),
            Repr::F64(value) => formatter.debug_tuple("F64").field(value).finish(),
            Repr::String(value) => formatter.debug_tuple("String").field(value).finish(),
            Repr::Bytes(value) => formatter.debug_tuple("Bytes").field(value).finish(),
            Repr::List(items) => formatter.debug_tuple("List").field(items).finish(),
            Repr::Object(entries) => formatter.debug_tuple("Object").field(entries).finish(),
            Repr::Enum { tag, payload } => formatter
                .debug_struct("Enum")
                .field("tag", tag)
                .field("payload", payload)
                .finish(),
            Repr::Opaque(_) => formatter.write_str("Opaque(<redacted>)"),
            Repr::Sensitive(_) => formatter.write_str("Sensitive(<redacted>)"),
        }
    }
}

impl ContractValue {
    /// Constructs a null value.
    pub fn null() -> Self {
        Self { repr: Repr::Null }
    }

    /// Constructs a boolean value.
    pub fn bool(value: bool) -> Self {
        Self {
            repr: Repr::Bool(value),
        }
    }

    /// Constructs a signed 64-bit integer value.
    pub fn i64(value: i64) -> Self {
        Self {
            repr: Repr::I64(value),
        }
    }

    /// Constructs an unsigned 64-bit integer value.
    pub fn u64(value: u64) -> Self {
        Self {
            repr: Repr::U64(value),
        }
    }

    /// Constructs a finite 32-bit floating-point value.
    pub fn f32(value: f32) -> Result<Self, ValueError> {
        value
            .is_finite()
            .then_some(Self {
                repr: Repr::F32(value),
            })
            .ok_or(ValueError::NonFiniteF32)
    }

    /// Constructs a finite 64-bit floating-point value.
    pub fn f64(value: f64) -> Result<Self, ValueError> {
        value
            .is_finite()
            .then_some(Self {
                repr: Repr::F64(value),
            })
            .ok_or(ValueError::NonFiniteF64)
    }

    /// Constructs a UTF-8 string value.
    pub fn string(value: impl Into<String>) -> Self {
        Self {
            repr: Repr::String(value.into()),
        }
    }

    /// Constructs a byte string value.
    pub fn bytes(value: impl Into<Vec<u8>>) -> Self {
        Self {
            repr: Repr::Bytes(value.into()),
        }
    }

    /// Constructs a list. Only contract values can be nested, so `Missing`
    /// cannot appear inside a list.
    ///
    /// ```compile_fail
    /// use boxology_contract::{ContractValue, SlotValue};
    /// let _ = ContractValue::list([SlotValue::Missing]);
    /// ```
    pub fn list(items: impl IntoIterator<Item = ContractValue>) -> Self {
        Self {
            repr: Repr::List(items.into_iter().collect()),
        }
    }

    /// Constructs an insertion-ordered object with unique keys. Object values
    /// cannot be `Missing` because they are contract values rather than slots.
    ///
    /// ```compile_fail
    /// use boxology_contract::{ContractValue, SlotValue};
    /// let _ = ContractValue::object([("key".into(), SlotValue::Missing)]);
    /// ```
    pub fn object(
        entries: impl IntoIterator<Item = (String, ContractValue)>,
    ) -> Result<Self, ValueError> {
        let mut collected = Vec::new();
        for (key, value) in entries {
            if collected
                .iter()
                .any(|(existing, _): &(String, ContractValue)| existing == &key)
            {
                return Err(ValueError::DuplicateObjectKey { key });
            }
            collected.push((key, value));
        }
        Ok(Self {
            repr: Repr::Object(collected),
        })
    }

    /// Constructs an enum node. A missing payload is legal at this call-slot boundary.
    pub fn enum_value(tag: impl Into<String>, payload: SlotValue) -> Self {
        Self {
            repr: Repr::Enum {
                tag: tag.into(),
                payload: Box::new(payload),
            },
        }
    }

    /// Constructs an opaque transport-neutral value.
    pub fn opaque(payload: OpaquePayload) -> Self {
        Self {
            repr: Repr::Opaque(payload),
        }
    }

    /// Marks an entire value subtree as sensitive for diagnostic redaction.
    pub fn sensitive(inner: ContractValue) -> Self {
        Self {
            repr: Repr::Sensitive(Box::new(inner)),
        }
    }

    /// Borrows this value through its read-only semantic view.
    pub fn view(&self) -> ValueRef<'_> {
        match &self.repr {
            Repr::Null => ValueRef::Null,
            Repr::Bool(value) => ValueRef::Bool(*value),
            Repr::I64(value) => ValueRef::I64(*value),
            Repr::U64(value) => ValueRef::U64(*value),
            Repr::F32(value) => ValueRef::F32(*value),
            Repr::F64(value) => ValueRef::F64(*value),
            Repr::String(value) => ValueRef::String(value),
            Repr::Bytes(value) => ValueRef::Bytes(value),
            Repr::List(items) => ValueRef::List(items),
            Repr::Object(entries) => ValueRef::Object(ObjectRef { entries }),
            Repr::Enum { tag, payload } => ValueRef::Enum { tag, payload },
            Repr::Opaque(payload) => ValueRef::Opaque(payload),
            Repr::Sensitive(inner) => ValueRef::Sensitive(inner),
        }
    }
}

/// A borrowed read-only view of a contract value.
#[derive(Clone, Copy)]
pub enum ValueRef<'a> {
    Null,
    Bool(bool),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    String(&'a str),
    Bytes(&'a [u8]),
    List(&'a [ContractValue]),
    Object(ObjectRef<'a>),
    Enum {
        tag: &'a str,
        payload: &'a SlotValue,
    },
    Opaque(&'a OpaquePayload),
    Sensitive(&'a ContractValue),
}

/// A borrowed read-only view of an insertion-ordered object.
#[derive(Clone, Copy)]
pub struct ObjectRef<'a> {
    entries: &'a [(String, ContractValue)],
}

impl<'a> ObjectRef<'a> {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&'a ContractValue> {
        self.entries
            .iter()
            .find_map(|(entry_key, value)| (entry_key == key).then_some(value))
    }

    pub fn entries(self) -> impl Iterator<Item = (&'a str, &'a ContractValue)> + 'a {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpaqueTree;

    #[test]
    fn rejects_every_non_finite_float() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(ContractValue::f32(value), Err(ValueError::NonFiniteF32));
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(ContractValue::f64(value), Err(ValueError::NonFiniteF64));
        }
    }

    #[test]
    fn objects_reject_duplicates_and_preserve_order() {
        let duplicate = |entries| ContractValue::object(entries);
        assert_eq!(
            duplicate(vec![
                ("x".into(), ContractValue::null()),
                ("x".into(), ContractValue::bool(true))
            ]),
            Err(ValueError::DuplicateObjectKey { key: "x".into() })
        );
        assert_eq!(
            duplicate(vec![
                ("a".into(), ContractValue::null()),
                ("b".into(), ContractValue::null()),
                ("c".into(), ContractValue::null()),
                ("b".into(), ContractValue::null())
            ]),
            Err(ValueError::DuplicateObjectKey { key: "b".into() })
        );
        let value = ContractValue::object([
            ("".into(), ContractValue::u64(1)),
            ("second".into(), ContractValue::u64(2)),
        ])
        .unwrap();
        let ValueRef::Object(object) = value.view() else {
            panic!()
        };
        assert_eq!(
            escaping_entries(object)
                .map(|(key, _)| key)
                .collect::<Vec<_>>(),
            ["", "second"]
        );
        assert_eq!(object.get("second"), Some(&ContractValue::u64(2)));
        assert_eq!(object.len(), 2);
        let empty = ContractValue::object(Vec::<(String, ContractValue)>::new()).unwrap();
        let ValueRef::Object(empty) = empty.view() else {
            panic!()
        };
        assert!(empty.is_empty());
    }

    fn escaping_entries<'a>(
        object: ObjectRef<'a>,
    ) -> impl Iterator<Item = (&'a str, &'a ContractValue)> + 'a {
        object.entries()
    }

    #[test]
    fn public_visitors_rebuild_every_value_kind_and_nesting() {
        let values = ContractValue::list([
            ContractValue::null(),
            ContractValue::bool(true),
            ContractValue::i64(-1),
            ContractValue::u64(2),
            ContractValue::f32(3.5).unwrap(),
            ContractValue::f64(4.5).unwrap(),
            ContractValue::string("text"),
            ContractValue::bytes([5, 6]),
            ContractValue::list([ContractValue::bool(false)]),
            ContractValue::object([("nested".into(), ContractValue::string("value"))]).unwrap(),
            ContractValue::enum_value("missing", SlotValue::Missing),
            ContractValue::enum_value("null", SlotValue::Null),
            ContractValue::enum_value(
                "value",
                SlotValue::Value(ContractValue::list([ContractValue::i64(7)])),
            ),
            ContractValue::opaque(OpaquePayload::new(OpaqueTree::List(vec![
                OpaqueTree::Bool(true),
                OpaqueTree::String("opaque".into()),
            ]))),
            ContractValue::sensitive(ContractValue::sensitive(
                ContractValue::object([("secret".into(), ContractValue::string("hidden"))])
                    .unwrap(),
            )),
        ]);
        assert_eq!(rebuild(&values), values);
    }

    fn rebuild(value: &ContractValue) -> ContractValue {
        match value.view() {
            ValueRef::Null => ContractValue::null(),
            ValueRef::Bool(value) => ContractValue::bool(value),
            ValueRef::I64(value) => ContractValue::i64(value),
            ValueRef::U64(value) => ContractValue::u64(value),
            ValueRef::F32(value) => ContractValue::f32(value).unwrap(),
            ValueRef::F64(value) => ContractValue::f64(value).unwrap(),
            ValueRef::String(value) => ContractValue::string(value),
            ValueRef::Bytes(value) => ContractValue::bytes(value),
            ValueRef::List(items) => ContractValue::list(items.iter().map(rebuild)),
            ValueRef::Object(object) => ContractValue::object(
                object
                    .entries()
                    .map(|(key, value)| (key.into(), rebuild(value))),
            )
            .unwrap(),
            ValueRef::Enum { tag, payload } => {
                ContractValue::enum_value(tag, rebuild_slot(payload))
            }
            ValueRef::Opaque(payload) => ContractValue::opaque(payload.forward()),
            ValueRef::Sensitive(inner) => ContractValue::sensitive(rebuild(inner)),
        }
    }

    fn rebuild_slot(slot: &SlotValue) -> SlotValue {
        match slot {
            SlotValue::Missing => SlotValue::Missing,
            SlotValue::Null => SlotValue::Null,
            SlotValue::Value(value) => SlotValue::Value(rebuild(value)),
        }
    }

    #[test]
    fn debug_redacts_sensitive_and_opaque_subtrees() {
        const SENTINEL: &str = "never-print-this-value";
        let secret = || ContractValue::sensitive(ContractValue::string(SENTINEL));
        let values = [
            ContractValue::object([("secret".into(), secret())]).unwrap(),
            ContractValue::list([secret()]),
            ContractValue::enum_value("secret", SlotValue::Value(secret())),
        ];
        for value in values {
            let output = format!("{value:?}");
            assert!(!output.contains(SENTINEL));
            assert!(output.contains("<redacted>"));
        }
        assert_eq!(format!("{:?}", secret()), "Sensitive(<redacted>)");
        let visible = format!("{:?}", ContractValue::string("ordinary"));
        assert!(visible.contains("ordinary"));

        let payload = OpaquePayload::new(OpaqueTree::Object(vec![(
            "raw".into(),
            OpaqueTree::String(SENTINEL.into()),
        )]));
        for payload in [payload.clone(), payload.forward()] {
            let values = [
                ContractValue::list([ContractValue::opaque(payload.forward())]),
                ContractValue::object([(
                    "opaque".into(),
                    ContractValue::opaque(payload.forward()),
                )])
                .unwrap(),
                ContractValue::enum_value(
                    "opaque",
                    SlotValue::Value(ContractValue::opaque(payload.forward())),
                ),
                ContractValue::sensitive(ContractValue::opaque(payload.forward())),
            ];
            for value in values {
                let contract_debug = format!("{value:?}");
                let slot_debug = format!("{:?}", SlotValue::Value(value));
                assert!(!contract_debug.contains(SENTINEL));
                assert!(!slot_debug.contains(SENTINEL));
                assert!(contract_debug.contains("<redacted>"));
                assert!(slot_debug.contains("<redacted>"));
            }
        }
        assert_eq!(
            format!("{:?}", ContractValue::opaque(payload)),
            "Opaque(<redacted>)"
        );
    }

    #[test]
    fn generated_values_round_trip_through_public_views() {
        let mut rng = SplitMix64(0x57a1_1eed_cafe_f00d);
        let mut kinds = 0_u16;
        for index in 0..256 {
            let kind = index % KIND_COUNT;
            kinds |= 1 << kind;
            let value = generated_kind(&mut rng, 4, kind);
            assert_eq!(rebuild(&value), value, "case {index}");
        }
        assert_eq!(kinds, (1 << KIND_COUNT) - 1);
    }

    #[test]
    fn generated_sensitive_positions_never_leak() {
        let mut rng = SplitMix64(0xd15c_a11e_5afe_f00d);
        let mut positions = 0_u8;
        for _ in 0..256 {
            let (value, position) = generated_hidden(&mut rng);
            positions |= 1 << position;
            let output = format!("{value:?}");
            assert!(!output.contains(SENTINEL));
            assert!(output.contains("<redacted>"));
        }
        assert_eq!(positions, 0b1111);
    }

    #[test]
    fn generated_construction_does_not_panic_and_duplicates_are_exact() {
        let mut rng = SplitMix64(0xc0de_cafe_1234_5678);
        for index in 0..256 {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                generated_kind(&mut rng, 4, index % KIND_COUNT)
            }));
            assert!(result.is_ok(), "constructor panic in case {index}");

            let mut entries = generated_entries(&mut rng, 3, false);
            let key = entries[rng.range(entries.len())].0.clone();
            entries.push((key.clone(), generated(&mut rng, 2)));
            assert_eq!(
                ContractValue::object(entries),
                Err(ValueError::DuplicateObjectKey { key })
            );
        }
    }

    const KIND_COUNT: usize = 13;
    const SENTINEL: &str = "property-secret-sentinel";

    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut value = self.0;
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^ (value >> 31)
        }

        fn range(&mut self, upper: usize) -> usize {
            (self.next() % upper as u64) as usize
        }
    }

    fn generated(rng: &mut SplitMix64, depth: usize) -> ContractValue {
        let kinds = if depth == 0 { 8 } else { KIND_COUNT };
        let kind = rng.range(kinds);
        generated_kind(rng, depth, kind)
    }

    fn generated_kind(rng: &mut SplitMix64, depth: usize, kind: usize) -> ContractValue {
        match kind {
            0 => ContractValue::null(),
            1 => ContractValue::bool(rng.next() & 1 == 1),
            2 => ContractValue::i64(rng.next() as i64),
            3 => ContractValue::u64(rng.next()),
            4 => ContractValue::f32((rng.next() as i32) as f32).unwrap(),
            5 => ContractValue::f64((rng.next() as i64) as f64).unwrap(),
            6 => ContractValue::string(format!("s{:x}", rng.next())),
            7 => ContractValue::bytes(rng.next().to_le_bytes()),
            8 => ContractValue::list(
                (0..rng.range(5)).map(|_| generated(rng, depth.saturating_sub(1))),
            ),
            9 => ContractValue::object(generated_entries(rng, depth, true)).unwrap(),
            10 => {
                let payload = match rng.range(3) {
                    0 => SlotValue::Missing,
                    1 => SlotValue::Null,
                    _ => SlotValue::Value(generated(rng, depth.saturating_sub(1))),
                };
                ContractValue::enum_value(format!("e{:x}", rng.next()), payload)
            }
            11 => ContractValue::opaque(OpaquePayload::new(OpaqueTree::List(vec![
                OpaqueTree::Bool(rng.next() & 1 == 1),
                OpaqueTree::String(format!("o{:x}", rng.next())),
            ]))),
            12 => ContractValue::sensitive(generated(rng, depth.saturating_sub(1))),
            _ => unreachable!(),
        }
    }

    fn generated_entries(
        rng: &mut SplitMix64,
        depth: usize,
        may_be_empty: bool,
    ) -> Vec<(String, ContractValue)> {
        let count = rng.range(4) + usize::from(!may_be_empty);
        (0..count)
            .map(|index| {
                (
                    format!("k{index}-{:x}", rng.next()),
                    generated(rng, depth.saturating_sub(1)),
                )
            })
            .collect()
    }

    fn generated_hidden(rng: &mut SplitMix64) -> (ContractValue, usize) {
        let position = rng.range(4);
        let secret = ContractValue::sensitive(ContractValue::string(SENTINEL));
        let value = match position {
            0 => secret,
            1 => ContractValue::list([generated(rng, 0), secret]),
            2 => ContractValue::object([
                ("noise".into(), generated(rng, 0)),
                ("secret".into(), secret),
            ])
            .unwrap(),
            3 => ContractValue::enum_value("secret", SlotValue::Value(secret)),
            _ => unreachable!(),
        };
        (value, position)
    }

    #[test]
    fn slot_null_forms_remain_distinct() {
        assert_ne!(SlotValue::Null, SlotValue::Value(ContractValue::null()));
    }

    #[test]
    fn public_values_are_send_sync_and_static() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        assert_bounds::<ContractValue>();
        assert_bounds::<SlotValue>();
    }
}
