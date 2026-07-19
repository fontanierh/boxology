use std::error::Error;
use std::fmt;

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
#[derive(Debug, Clone, PartialEq)]
pub struct ContractValue {
    repr: Repr,
}

#[derive(Debug, Clone, PartialEq)]
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
        }
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
