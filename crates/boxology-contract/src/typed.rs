//! Strict typed conversion for transport-neutral contract values.
//!
//! Every value of each supported scalar and `String` round-trips. The only
//! exception is a non-finite float, which fails during encoding.

use std::error::Error;
use std::fmt;

use crate::{ContractValue, SlotValue, ValueRef};

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
    UnexpectedMissing,
    KindMismatch,
    OutOfRange,
    UnexpectedPayload,
    UnknownField(String),
    UnknownVariant(String),
    UnsupportedPosition,
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
                write!(formatter, "{:?} at {:?}", self.kind, self.path)
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
