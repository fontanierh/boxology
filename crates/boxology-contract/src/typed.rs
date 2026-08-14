//! Strict typed conversion for transport-neutral contract values.
//!
//! Every value of each supported scalar and `String` round-trips. The only
//! exception is a non-finite float, which fails during encoding.
//! Presence wrappers may nest as Rust types; descriptor construction remains
//! responsible for rejecting wrappers in illegal positions.
//! Maps accept any source object order and re-encode keys in sorted order.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::{ContractValue, Field, SlotValue, ValueRef};

/// One segment in a contract-value diagnostic path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathSegment {
    Field(String),
    Index(usize),
    MapKey(String),
    Variant(String),
}

/// The category of a typed encoding failure.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeErrorKind {
    NonFiniteF32,
    NonFiniteF64,
    UnsupportedPosition,
}

/// The category of a typed decoding failure.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeErrorKind {
    MissingRequired,
    UnexpectedNull,
    /// An optional object field used `null`; absence must be represented by omission.
    OptionalFieldNull,
    UnexpectedMissing,
    KindMismatch,
    OutOfRange,
    UnexpectedPayload,
    UnknownField(String),
    UnknownVariant(String),
    UnsupportedPosition,
}

trait DiagnosticKind {
    fn guidance(&self) -> Option<&'static str> {
        None
    }
}

impl DiagnosticKind for EncodeErrorKind {}

impl DiagnosticKind for DecodeErrorKind {
    fn guidance(&self) -> Option<&'static str> {
        match self {
            Self::OptionalFieldNull => {
                Some("omit an absent optional field instead of encoding null")
            }
            _ => None,
        }
    }
}

macro_rules! error_type {
    ($name:ident, $kind:ident) => {
        /// A typed conversion failure with a schema path and payload-free category.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            path: Vec<PathSegment>,
            kind: $kind,
        }

        impl $name {
            pub fn new(kind: $kind) -> Self {
                Self {
                    path: Vec::new(),
                    kind,
                }
            }

            pub fn under(mut self, segment: PathSegment) -> Self {
                self.path.insert(0, segment);
                self
            }

            pub fn kind(&self) -> &$kind {
                &self.kind
            }

            pub fn path(&self) -> &[PathSegment] {
                &self.path
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                if let Some(guidance) = self.kind.guidance() {
                    write!(formatter, "{:?}: {guidance} at {:?}", self.kind, self.path)
                } else {
                    write!(formatter, "{:?} at {:?}", self.kind, self.path)
                }
            }
        }

        impl Error for $name {}
    };
}

error_type!(EncodeError, EncodeErrorKind);
error_type!(DecodeError, DecodeErrorKind);

/// A Rust type with an exact transport-neutral contract representation.
pub trait ContractType: Sized {
    fn encode_value(&self) -> Result<ContractValue, EncodeError>;
    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError>;

    fn encode(&self) -> Result<SlotValue, EncodeError> {
        Ok(SlotValue::Value(self.encode_value()?))
    }

    fn decode(slot: &SlotValue) -> Result<Self, DecodeError> {
        match slot {
            SlotValue::Missing => Err(DecodeError::new(DecodeErrorKind::MissingRequired)),
            SlotValue::Null => Err(DecodeError::new(DecodeErrorKind::UnexpectedNull)),
            SlotValue::Value(value) => Self::decode_value(value),
        }
    }

    fn encode_field(&self) -> Result<Option<ContractValue>, EncodeError> {
        Ok(Some(self.encode_value()?))
    }

    fn decode_field(field: Option<&ContractValue>) -> Result<Self, DecodeError> {
        match field {
            None => Err(DecodeError::new(DecodeErrorKind::MissingRequired)),
            Some(value) => Self::decode_value(value),
        }
    }
}

fn mismatch<T>() -> Result<T, DecodeError> {
    Err(DecodeError::new(DecodeErrorKind::KindMismatch))
}

macro_rules! exact_scalar {
    ($type:ty, $constructor:ident, $variant:ident) => {
        impl ContractType for $type {
            fn encode_value(&self) -> Result<ContractValue, EncodeError> {
                Ok(ContractValue::$constructor(*self))
            }

            fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
                match value.view() {
                    ValueRef::$variant(value) => Ok(value),
                    _ => mismatch(),
                }
            }
        }
    };
}

exact_scalar!(bool, bool, Bool);
exact_scalar!(i64, i64, I64);
exact_scalar!(u64, u64, U64);

macro_rules! narrow_integers {
    ($constructor:ident, $variant:ident; $($type:ty),+ $(,)?) => {$(
        impl ContractType for $type {
            fn encode_value(&self) -> Result<ContractValue, EncodeError> {
                Ok(ContractValue::$constructor((*self).into()))
            }

            fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
                match value.view() {
                    ValueRef::$variant(value) => value
                        .try_into()
                        .map_err(|_| DecodeError::new(DecodeErrorKind::OutOfRange)),
                    _ => mismatch(),
                }
            }
        }
    )+};
}

narrow_integers!(i64, I64; i8, i16, i32);
narrow_integers!(u64, U64; u8, u16, u32);

macro_rules! floats {
    ($type:ty, $constructor:ident, $variant:ident, $kind:ident) => {
        impl ContractType for $type {
            fn encode_value(&self) -> Result<ContractValue, EncodeError> {
                ContractValue::$constructor(*self)
                    .map_err(|_| EncodeError::new(EncodeErrorKind::$kind))
            }

            fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
                match value.view() {
                    ValueRef::$variant(value) => Ok(value),
                    _ => mismatch(),
                }
            }
        }
    };
}

floats!(f32, f32, F32, NonFiniteF32);
floats!(f64, f64, F64, NonFiniteF64);

impl ContractType for String {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        Ok(ContractValue::string(self))
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        match value.view() {
            ValueRef::String(value) => Ok(value.into()),
            _ => mismatch(),
        }
    }
}

impl<T: ContractType> ContractType for Option<T> {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        match self {
            None => Ok(ContractValue::null()),
            Some(value) => value.encode_value(),
        }
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        match value.view() {
            ValueRef::Null => Ok(None),
            _ => T::decode_value(value).map(Some),
        }
    }

    fn encode(&self) -> Result<SlotValue, EncodeError> {
        match self {
            None => Ok(SlotValue::Null),
            Some(value) => Ok(SlotValue::Value(value.encode_value()?)),
        }
    }

    fn decode(slot: &SlotValue) -> Result<Self, DecodeError> {
        match slot {
            SlotValue::Missing => Err(DecodeError::new(DecodeErrorKind::UnexpectedMissing)),
            SlotValue::Null => Ok(None),
            SlotValue::Value(value) => T::decode_value(value).map(Some),
        }
    }

    fn encode_field(&self) -> Result<Option<ContractValue>, EncodeError> {
        match self {
            None => Ok(None),
            Some(value) => value.encode_value().map(Some),
        }
    }

    fn decode_field(field: Option<&ContractValue>) -> Result<Self, DecodeError> {
        match field {
            None => Ok(None),
            Some(value) if matches!(value.view(), ValueRef::Null) => {
                Err(DecodeError::new(DecodeErrorKind::OptionalFieldNull))
            }
            Some(value) => T::decode_value(value).map(Some),
        }
    }
}

impl<T: ContractType> ContractType for Field<T> {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        Err(EncodeError::new(EncodeErrorKind::UnsupportedPosition))
    }

    fn decode_value(_value: &ContractValue) -> Result<Self, DecodeError> {
        Err(DecodeError::new(DecodeErrorKind::UnsupportedPosition))
    }

    fn encode(&self) -> Result<SlotValue, EncodeError> {
        match self {
            Field::Missing => Ok(SlotValue::Missing),
            Field::Null => Ok(SlotValue::Null),
            Field::Value(value) => Ok(SlotValue::Value(value.encode_value()?)),
        }
    }

    fn decode(slot: &SlotValue) -> Result<Self, DecodeError> {
        match slot {
            SlotValue::Missing => Ok(Field::Missing),
            SlotValue::Null => Ok(Field::Null),
            SlotValue::Value(value) => T::decode_value(value).map(Field::Value),
        }
    }

    fn encode_field(&self) -> Result<Option<ContractValue>, EncodeError> {
        match self {
            Field::Missing => Ok(None),
            Field::Null => Ok(Some(ContractValue::null())),
            Field::Value(value) => value.encode_value().map(Some),
        }
    }

    fn decode_field(field: Option<&ContractValue>) -> Result<Self, DecodeError> {
        match field {
            None => Ok(Field::Missing),
            Some(value) if matches!(value.view(), ValueRef::Null) => Ok(Field::Null),
            Some(value) => T::decode_value(value).map(Field::Value),
        }
    }
}

impl<T: ContractType> ContractType for Vec<T> {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        let values = self
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value
                    .encode_value()
                    .map_err(|error| error.under(PathSegment::Index(index)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ContractValue::list(values))
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        match value.view() {
            ValueRef::Null => Err(DecodeError::new(DecodeErrorKind::UnexpectedNull)),
            ValueRef::List(values) => values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    T::decode_value(value).map_err(|error| error.under(PathSegment::Index(index)))
                })
                .collect(),
            _ => mismatch(),
        }
    }
}

impl<T: ContractType> ContractType for BTreeMap<String, T> {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        let entries = self
            .iter()
            .map(|(key, value)| {
                value
                    .encode_value()
                    .map(|value| (key.clone(), value))
                    .map_err(|error| error.under(PathSegment::MapKey(key.clone())))
            })
            .collect::<Result<Vec<_>, _>>()?;
        ContractValue::object(entries).map_err(|_| unreachable!())
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        match value.view() {
            ValueRef::Null => Err(DecodeError::new(DecodeErrorKind::UnexpectedNull)),
            ValueRef::Object(object) => object
                .entries()
                .map(|(key, value)| {
                    T::decode_value(value)
                        .map(|value| (key.into(), value))
                        .map_err(|error| error.under(PathSegment::MapKey(key.into())))
                })
                .collect(),
            _ => mismatch(),
        }
    }
}

/// A value whose diagnostic representations must remain redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Explicitly reveals the wrapped value.
    ///
    /// Callers must treat the returned content as sensitive data.
    pub fn reveal(&self) -> &T {
        &self.0
    }

    /// Consumes the wrapper and explicitly reveals the wrapped value.
    ///
    /// Callers must treat the returned content as sensitive data.
    pub fn into_revealed(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

impl<T: ContractType> ContractType for Secret<T> {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        self.0.encode_value().map(ContractValue::sensitive)
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        match value.view() {
            ValueRef::Null => Err(DecodeError::new(DecodeErrorKind::UnexpectedNull)),
            ValueRef::Sensitive(inner) => T::decode_value(inner).map(Self),
            _ => mismatch(),
        }
    }
}

/// An ordinary, non-sensitive byte string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob(Vec<u8>);

impl Blob {
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl ContractType for Blob {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        Ok(ContractValue::bytes(self.0.clone()))
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        match value.view() {
            ValueRef::Bytes(bytes) => Ok(Self(bytes.to_vec())),
            _ => mismatch(),
        }
    }
}

/// A domain error with a stable variant tag crossing `ErasedCallError::Domain`.
pub trait ContractError: ContractType {
    fn error_tag(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conform::{Shape, conform_slot};
    use crate::{OpaquePayload, OpaqueTree};

    fn round_trip<T: ContractType + fmt::Debug + PartialEq>(value: T) {
        assert_eq!(T::decode(&value.encode().unwrap()).unwrap(), value);
    }

    #[test]
    fn every_supported_scalar_and_string_round_trips() {
        round_trip(false);
        round_trip(true);
        round_trip(i8::MIN);
        round_trip(i16::MAX);
        round_trip(i32::MIN);
        round_trip(i64::MAX);
        round_trip(u8::MAX);
        round_trip(u16::MIN);
        round_trip(u32::MAX);
        round_trip(u64::MAX);
        round_trip(-0.0_f32);
        round_trip(f64::MIN);
        round_trip(String::from("owned UTF-8 α"));
    }

    fn out_of_range<T: ContractType + fmt::Debug>(value: ContractValue) {
        assert_eq!(
            T::decode_value(&value).unwrap_err().kind(),
            &DecodeErrorKind::OutOfRange
        );
    }

    #[test]
    fn narrow_integer_boundaries_are_exact() {
        macro_rules! signed {
            ($($type:ty),+) => {$(
                assert_eq!(<$type>::decode_value(&ContractValue::i64(<$type>::MIN.into())), Ok(<$type>::MIN));
                assert_eq!(<$type>::decode_value(&ContractValue::i64(<$type>::MAX.into())), Ok(<$type>::MAX));
                out_of_range::<$type>(ContractValue::i64(i64::from(<$type>::MIN) - 1));
                out_of_range::<$type>(ContractValue::i64(i64::from(<$type>::MAX) + 1));
            )+}; }
        macro_rules! unsigned {
            ($($type:ty),+) => {$(
                assert_eq!(<$type>::decode_value(&ContractValue::u64(0)), Ok(0));
                assert_eq!(<$type>::decode_value(&ContractValue::u64(<$type>::MAX.into())), Ok(<$type>::MAX));
                out_of_range::<$type>(ContractValue::u64(u64::from(<$type>::MAX) + 1));
            )+}; }
        signed!(i8, i16, i32);
        unsigned!(u8, u16, u32);
    }

    #[test]
    fn non_finite_float_encoding_is_fallible() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(
                value.encode_value().unwrap_err().kind(),
                &EncodeErrorKind::NonFiniteF32
            );
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                value.encode_value().unwrap_err().kind(),
                &EncodeErrorKind::NonFiniteF64
            );
        }
    }

    fn rejects_other_kinds<T: ContractType + fmt::Debug>(values: &[ContractValue], own: usize) {
        for (index, value) in values.iter().enumerate() {
            if index != own {
                assert_eq!(
                    T::decode_value(value).unwrap_err().kind(),
                    &DecodeErrorKind::KindMismatch
                );
            }
        }
    }

    #[test]
    fn scalars_reject_every_other_value_kind() {
        let values = vec![
            ContractValue::null(),
            ContractValue::bool(true),
            ContractValue::i64(0),
            ContractValue::u64(0),
            ContractValue::f32(0.0).unwrap(),
            ContractValue::f64(0.0).unwrap(),
            ContractValue::string("text"),
            ContractValue::bytes([]),
            ContractValue::list([]),
            ContractValue::object([("x".into(), ContractValue::null())]).unwrap(),
            ContractValue::enum_value("tag", SlotValue::Null),
            ContractValue::opaque(OpaquePayload::new(OpaqueTree::String("raw".into()))),
            ContractValue::sensitive(ContractValue::string("secret")),
        ];
        rejects_other_kinds::<bool>(&values, 1);
        rejects_other_kinds::<i8>(&values, 2);
        rejects_other_kinds::<i16>(&values, 2);
        rejects_other_kinds::<i32>(&values, 2);
        rejects_other_kinds::<i64>(&values, 2);
        rejects_other_kinds::<u8>(&values, 3);
        rejects_other_kinds::<u16>(&values, 3);
        rejects_other_kinds::<u32>(&values, 3);
        rejects_other_kinds::<u64>(&values, 3);
        rejects_other_kinds::<f32>(&values, 4);
        rejects_other_kinds::<f64>(&values, 5);
        rejects_other_kinds::<String>(&values, 6);
    }

    #[test]
    fn required_defaults_distinguish_missing_null_and_wrong_kind() {
        assert_eq!(
            bool::decode(&SlotValue::Missing).unwrap_err().kind(),
            &DecodeErrorKind::MissingRequired
        );
        assert_eq!(
            bool::decode(&SlotValue::Null).unwrap_err().kind(),
            &DecodeErrorKind::UnexpectedNull
        );
        assert_eq!(
            bool::decode(&SlotValue::Value(ContractValue::null()))
                .unwrap_err()
                .kind(),
            &DecodeErrorKind::KindMismatch
        );
        assert_eq!(
            bool::decode_field(None).unwrap_err().kind(),
            &DecodeErrorKind::MissingRequired
        );
        let field = 7_u8.encode_field().unwrap();
        assert_eq!(u8::decode_field(field.as_ref()), Ok(7));
    }

    #[test]
    fn error_paths_prepend_and_diagnostics_expose_no_payloads() {
        let decode = DecodeError::new(DecodeErrorKind::UnknownField("leaf".into()))
            .under(PathSegment::Index(3))
            .under(PathSegment::Field("root".into()));
        assert_eq!(decode.kind(), &DecodeErrorKind::UnknownField("leaf".into()));
        assert_eq!(
            decode.path(),
            &[PathSegment::Field("root".into()), PathSegment::Index(3)]
        );
        assert_eq!(
            decode.to_string(),
            "UnknownField(\"leaf\") at [Field(\"root\"), Index(3)]"
        );

        let encode = EncodeError::new(EncodeErrorKind::NonFiniteF64)
            .under(PathSegment::MapKey("item".into()));
        assert_eq!(encode.kind(), &EncodeErrorKind::NonFiniteF64);
        assert_eq!(encode.path(), &[PathSegment::MapKey("item".into())]);

        const SENTINEL: &str = "runtime-payload-must-not-escape";
        for payload in [
            ContractValue::sensitive(ContractValue::string(SENTINEL)),
            ContractValue::opaque(OpaquePayload::new(OpaqueTree::String(SENTINEL.into()))),
        ] {
            let error = bool::decode_value(&payload).unwrap_err();
            for diagnostic in [format!("{error:?}"), error.to_string()] {
                assert!(!diagnostic.contains(SENTINEL));
            }
        }
    }

    #[test]
    fn public_diagnostic_types_are_send_sync_and_static() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        assert_bounds::<PathSegment>();
        assert_bounds::<EncodeErrorKind>();
        assert_bounds::<EncodeError>();
        assert_bounds::<DecodeErrorKind>();
        assert_bounds::<DecodeError>();
    }

    fn decode_error<T: fmt::Debug>(result: Result<T, DecodeError>, kind: DecodeErrorKind) {
        assert_eq!(result.unwrap_err().kind(), &kind);
    }

    #[test]
    fn presence_grid_is_exact_at_slots_fields_and_value_positions() {
        let null = ContractValue::null();
        assert_eq!(Option::<u8>::decode(&SlotValue::Null), Ok(None));
        assert_eq!(None::<u8>.encode().unwrap(), SlotValue::Null);
        decode_error(
            Option::<u8>::decode(&SlotValue::Missing),
            DecodeErrorKind::UnexpectedMissing,
        );
        decode_error(
            Option::<u8>::decode(&SlotValue::Value(null.clone())),
            DecodeErrorKind::KindMismatch,
        );
        assert_eq!(Option::<u8>::decode_value(&null), Ok(None));
        assert_eq!(None::<u8>.encode_value().unwrap(), null);
        assert_eq!(Option::<u8>::decode_field(None), Ok(None));
        assert_eq!(None::<u8>.encode_field().unwrap(), None);
        let some = Some(7_u8);
        assert_eq!(
            some.encode().unwrap(),
            SlotValue::Value(ContractValue::u64(7))
        );
        assert_eq!(Option::<u8>::decode(&some.encode().unwrap()), Ok(some));
        let some_field = some.encode_field().unwrap();
        assert_eq!(some_field, Some(ContractValue::u64(7)));
        assert_eq!(Option::<u8>::decode_field(some_field.as_ref()), Ok(some));
        let optional_null = Option::<u8>::decode_field(Some(&ContractValue::null()))
            .unwrap_err()
            .under(PathSegment::Field("base".into()));
        assert_eq!(optional_null.kind(), &DecodeErrorKind::OptionalFieldNull);
        assert_eq!(
            optional_null.to_string(),
            "OptionalFieldNull: omit an absent optional field instead of encoding null at [Field(\"base\")]"
        );

        for (field, slot) in [
            (Field::Missing, SlotValue::Missing),
            (Field::Null, SlotValue::Null),
            (Field::Value(7_u8), SlotValue::Value(ContractValue::u64(7))),
        ] {
            assert_eq!(field.encode().unwrap(), slot);
            assert_eq!(Field::<u8>::decode(&slot), Ok(field));
        }
        assert_eq!(Field::<u8>::decode_field(None), Ok(Field::Missing));
        assert_eq!(Field::<u8>::decode_field(Some(&null)), Ok(Field::Null));
        assert_eq!(
            Field::<u8>::Null.encode_field().unwrap(),
            Some(null.clone())
        );
        assert_eq!(Field::<u8>::Missing.encode_field().unwrap(), None);
        let value_field = Field::Value(7_u8).encode_field().unwrap();
        assert_eq!(value_field, Some(ContractValue::u64(7)));
        assert_eq!(
            Field::<u8>::decode_field(value_field.as_ref()),
            Ok(Field::Value(7))
        );
        decode_error(
            Field::<u8>::decode(&SlotValue::Value(null.clone())),
            DecodeErrorKind::KindMismatch,
        );
        decode_error(
            Field::<u8>::decode_field(Some(&ContractValue::bool(true))),
            DecodeErrorKind::KindMismatch,
        );
        for field in [Field::Missing, Field::Null, Field::Value(1_u8)] {
            assert_eq!(
                field.encode_value().unwrap_err().kind(),
                &EncodeErrorKind::UnsupportedPosition
            );
        }
        for value in [null, ContractValue::u64(1)] {
            decode_error(
                Field::<u8>::decode_value(&value),
                DecodeErrorKind::UnsupportedPosition,
            );
        }
    }

    #[test]
    fn nested_optional_lists_round_trip_and_paths_include_indices() {
        let values = vec![Some(1_u8), None, Some(3)];
        round_trip(values.clone());
        let encoded = values.encode_value().unwrap();
        let ValueRef::List(items) = encoded.view() else {
            panic!()
        };
        assert!(matches!(items[1].view(), ValueRef::Null));

        let encode = vec![0.0_f32, f32::NAN].encode_value().unwrap_err();
        assert_eq!(encode.kind(), &EncodeErrorKind::NonFiniteF32);
        assert_eq!(encode.path(), &[PathSegment::Index(1)]);
        let bad = ContractValue::list([ContractValue::u64(1), ContractValue::u64(256)]);
        let decode = Vec::<u8>::decode_value(&bad).unwrap_err();
        assert_eq!(decode.kind(), &DecodeErrorKind::OutOfRange);
        assert_eq!(decode.path(), &[PathSegment::Index(1)]);
        decode_error(
            Vec::<u8>::decode_value(&ContractValue::null()),
            DecodeErrorKind::UnexpectedNull,
        );
        decode_error(
            Vec::<u8>::decode_value(&ContractValue::bool(false)),
            DecodeErrorKind::KindMismatch,
        );
    }

    #[test]
    fn maps_round_trip_sort_input_and_report_key_paths() {
        let map = BTreeMap::from([("z".into(), 2_u8), ("a".into(), 1)]);
        round_trip(map);
        let input = ContractValue::object([
            ("z".into(), ContractValue::u64(2)),
            ("a".into(), ContractValue::u64(1)),
        ])
        .unwrap();
        let decoded = BTreeMap::<String, u8>::decode_value(&input).unwrap();
        let encoded = decoded.encode_value().unwrap();
        let ValueRef::Object(object) = encoded.view() else {
            panic!()
        };
        assert_eq!(
            object.entries().map(|(key, _)| key).collect::<Vec<_>>(),
            ["a", "z"]
        );

        let encode = BTreeMap::from([("secret".into(), f64::NAN)])
            .encode_value()
            .unwrap_err();
        assert_eq!(encode.kind(), &EncodeErrorKind::NonFiniteF64);
        assert_eq!(encode.path(), &[PathSegment::MapKey("secret".into())]);
        let bad = ContractValue::object([("bad".into(), ContractValue::u64(256))]).unwrap();
        let decode = BTreeMap::<String, u8>::decode_value(&bad).unwrap_err();
        assert_eq!(decode.kind(), &DecodeErrorKind::OutOfRange);
        assert_eq!(decode.path(), &[PathSegment::MapKey("bad".into())]);
        decode_error(
            BTreeMap::<String, u8>::decode_value(&ContractValue::null()),
            DecodeErrorKind::UnexpectedNull,
        );
        decode_error(
            BTreeMap::<String, u8>::decode_value(&ContractValue::bool(false)),
            DecodeErrorKind::KindMismatch,
        );
    }

    fn walker_agrees<T: ContractType + fmt::Debug>(shape: &Shape, slot: SlotValue) {
        let typed = T::decode(&slot).unwrap().encode().unwrap();
        for role in [
            crate::DecodeRole::ProviderInput,
            crate::DecodeRole::ConsumerOutput,
        ] {
            assert_eq!(conform_slot(shape, role, slot.clone()).unwrap(), typed);
        }
    }

    #[test]
    fn typed_presence_acceptance_agrees_with_the_walker() {
        let optional = Shape::optional(Shape::i64()).unwrap();
        walker_agrees::<Option<i64>>(&optional, SlotValue::Null);
        walker_agrees::<Option<i64>>(&optional, SlotValue::Value(ContractValue::i64(4)));
        let tri_state = Shape::tri_state(Shape::i64()).unwrap();
        walker_agrees::<Field<i64>>(&tri_state, SlotValue::Missing);
        walker_agrees::<Field<i64>>(&tri_state, SlotValue::Null);
        walker_agrees::<Field<i64>>(&tri_state, SlotValue::Value(ContractValue::i64(4)));
    }

    #[test]
    fn secrets_round_trip_in_every_supported_position() {
        let secret = Secret::new(String::from("classified"));
        let encoded = secret.encode_value().unwrap();
        assert_eq!(Secret::<String>::decode_value(&encoded), Ok(secret.clone()));
        round_trip(secret.clone());
        round_trip(Some(secret.clone()));
        round_trip(Field::Value(secret.clone()));
        round_trip(vec![secret.clone(), Secret::new("second".into())]);
        round_trip(BTreeMap::from([("key".into(), secret)]));
    }

    #[test]
    fn secret_decode_is_strict_and_unwraps_exactly_one_layer() {
        const SENTINEL: &str = "secret-visitor-sentinel";
        let encoded = Secret::new(String::from(SENTINEL)).encode_value().unwrap();
        let ValueRef::Sensitive(inner) = encoded.view() else {
            panic!()
        };
        assert!(matches!(inner.view(), ValueRef::String(SENTINEL)));

        decode_error(
            Secret::<String>::decode_value(&ContractValue::null()),
            DecodeErrorKind::UnexpectedNull,
        );
        for value in [
            ContractValue::string(SENTINEL),
            ContractValue::opaque(OpaquePayload::new(OpaqueTree::String(SENTINEL.into()))),
        ] {
            decode_error(
                Secret::<String>::decode_value(&value),
                DecodeErrorKind::KindMismatch,
            );
        }

        let nested = Secret::new(Secret::new(String::from(SENTINEL)));
        let nested_value = nested.encode_value().unwrap();
        assert_eq!(
            Secret::<Secret<String>>::decode_value(&nested_value),
            Ok(nested)
        );
        decode_error(
            Secret::<Secret<String>>::decode_value(&encoded),
            DecodeErrorKind::KindMismatch,
        );
    }

    #[test]
    fn secret_diagnostics_never_reveal_payloads() {
        const SENTINEL: &str = "never-print-secret-payload";
        let secret = Secret::new(String::from(SENTINEL));
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert_eq!(secret.to_string(), "Secret(<redacted>)");

        let sensitive = secret.encode_value().unwrap();
        let values = [
            sensitive.clone(),
            ContractValue::list([sensitive.clone()]),
            ContractValue::object([("secret".into(), sensitive.clone())]).unwrap(),
            ContractValue::enum_value("secret", SlotValue::Value(sensitive)),
        ];
        for value in values {
            for diagnostic in [
                format!("{value:?}"),
                format!("{:?}", SlotValue::Value(value)),
            ] {
                assert!(!diagnostic.contains(SENTINEL));
                assert!(diagnostic.contains("<redacted>"));
            }
        }

        let sensitive = ContractValue::sensitive(ContractValue::string(SENTINEL));
        let decode = Secret::<u8>::decode_value(&sensitive).unwrap_err();
        for diagnostic in [format!("{decode:?}"), decode.to_string()] {
            assert!(!diagnostic.contains(SENTINEL));
        }
        let encode = Secret::new(f32::NAN).encode_value().unwrap_err();
        assert_eq!(encode.kind(), &EncodeErrorKind::NonFiniteF32);
        for diagnostic in [format!("{encode:?}"), encode.to_string()] {
            assert!(!diagnostic.contains("NaN"));
        }
    }

    #[test]
    fn blob_is_an_exact_owned_byte_value() {
        let blob = Blob::new([0, 1, 255]);
        assert_eq!(blob.as_bytes(), &[0, 1, 255]);
        assert_eq!(blob.clone().into_bytes(), vec![0, 1, 255]);
        round_trip(blob.clone());
        let encoded = blob.encode_value().unwrap();
        assert!(matches!(encoded.view(), ValueRef::Bytes([0, 1, 255])));
        decode_error(
            Blob::decode_value(&ContractValue::null()),
            DecodeErrorKind::KindMismatch,
        );
        decode_error(
            Blob::decode_value(&ContractValue::string("bytes")),
            DecodeErrorKind::KindMismatch,
        );
        decode_error(
            Blob::decode(&SlotValue::Null),
            DecodeErrorKind::UnexpectedNull,
        );
    }

    #[derive(Debug)]
    struct TestDomainError;

    impl ContractType for TestDomainError {
        fn encode_value(&self) -> Result<ContractValue, EncodeError> {
            Ok(ContractValue::string("test-domain"))
        }

        fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
            String::decode_value(value).map(|_| Self)
        }
    }

    impl ContractError for TestDomainError {
        fn error_tag(&self) -> &str {
            "test-domain"
        }
    }

    #[test]
    fn new_public_types_have_expected_bounds_and_error_tag() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        fn assert_error<T: ContractError + Send + Sync + 'static>(error: &T) -> &str {
            error.error_tag()
        }
        assert_bounds::<Blob>();
        assert_bounds::<Secret<String>>();
        assert_eq!(assert_error(&TestDomainError), "test-domain");
    }
}
