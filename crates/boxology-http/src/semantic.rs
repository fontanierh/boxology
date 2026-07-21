use std::{error::Error, fmt};

use boxology_contract::{
    ConformanceErrorKind, ContractValue, DecodeRole, DescriptorRef, OpaqueTree, SlotValue,
    TypeDescriptor,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticErrorCategory {
    RepresentationMismatch,
    NonCanonicalInteger,
    IntegerRange,
    NonFiniteFloat,
    NullConformance,
    UnsupportedDescriptor,
}

impl SemanticErrorCategory {
    fn message(self) -> &'static str {
        match self {
            Self::RepresentationMismatch => "representation mismatch",
            Self::NonCanonicalInteger => "non-canonical integer",
            Self::IntegerRange => "integer outside descriptor range",
            Self::NonFiniteFloat => "non-finite float",
            Self::NullConformance => "null violates descriptor",
            Self::UnsupportedDescriptor => "unsupported descriptor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SemanticError(SemanticErrorCategory);

impl SemanticError {
    pub(crate) fn category(&self) -> SemanticErrorCategory {
        self.0
    }
}

impl fmt::Display for SemanticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.message())
    }
}

impl Error for SemanticError {}

pub(crate) fn decode_tree(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
    role: DecodeRole,
) -> Result<SlotValue, SemanticError> {
    if !is_supported(descriptor) {
        return failure(SemanticErrorCategory::UnsupportedDescriptor);
    }
    let slot = match tree {
        OpaqueTree::Null => SlotValue::Null,
        tree => SlotValue::Value(decode_scalar(tree, descriptor)?),
    };
    descriptor.conform(role, slot).map_err(|error| {
        let category = if matches!(error.kind(), ConformanceErrorKind::UnexpectedNull) {
            SemanticErrorCategory::NullConformance
        } else {
            SemanticErrorCategory::RepresentationMismatch
        };
        SemanticError(category)
    })
}

fn is_supported(descriptor: &TypeDescriptor) -> bool {
    matches!(
        descriptor.view(),
        DescriptorRef::Bool
            | DescriptorRef::String
            | DescriptorRef::I8
            | DescriptorRef::I16
            | DescriptorRef::I32
            | DescriptorRef::I64
            | DescriptorRef::U8
            | DescriptorRef::U16
            | DescriptorRef::U32
            | DescriptorRef::U64
            | DescriptorRef::F32
            | DescriptorRef::F64
    )
}

fn decode_scalar(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
) -> Result<ContractValue, SemanticError> {
    match descriptor.view() {
        DescriptorRef::Bool => match tree {
            OpaqueTree::Bool(value) => Ok(ContractValue::bool(value)),
            _ => failure(SemanticErrorCategory::RepresentationMismatch),
        },
        DescriptorRef::String => match tree {
            OpaqueTree::String(value) => Ok(ContractValue::string(value)),
            _ => failure(SemanticErrorCategory::RepresentationMismatch),
        },
        DescriptorRef::I8 => signed(tree, i8::MIN.into(), i8::MAX.into()),
        DescriptorRef::I16 => signed(tree, i16::MIN.into(), i16::MAX.into()),
        DescriptorRef::I32 => signed(tree, i32::MIN.into(), i32::MAX.into()),
        DescriptorRef::U8 => unsigned(tree, u8::MAX.into()),
        DescriptorRef::U16 => unsigned(tree, u16::MAX.into()),
        DescriptorRef::U32 => unsigned(tree, u32::MAX.into()),
        DescriptorRef::I64 => wide_signed(tree),
        DescriptorRef::U64 => wide_unsigned(tree),
        DescriptorRef::F32 => float32(tree),
        DescriptorRef::F64 => float64(tree),
        _ => failure(SemanticErrorCategory::UnsupportedDescriptor),
    }
}

fn integer_token(tree: OpaqueTree) -> Result<String, SemanticError> {
    match tree {
        OpaqueTree::Number(number) if !number.as_str().contains(['.', 'e', 'E']) => {
            Ok(number.as_str().into())
        }
        OpaqueTree::Number(_) => failure(SemanticErrorCategory::NonCanonicalInteger),
        _ => failure(SemanticErrorCategory::RepresentationMismatch),
    }
}

fn signed(tree: OpaqueTree, min: i64, max: i64) -> Result<ContractValue, SemanticError> {
    let value = integer_token(tree)?
        .parse::<i64>()
        .map_err(|_| SemanticError(SemanticErrorCategory::IntegerRange))?;
    (min..=max)
        .contains(&value)
        .then_some(ContractValue::i64(value))
        .ok_or(SemanticError(SemanticErrorCategory::IntegerRange))
}

fn unsigned(tree: OpaqueTree, max: u64) -> Result<ContractValue, SemanticError> {
    let value = integer_token(tree)?
        .parse::<u64>()
        .map_err(|_| SemanticError(SemanticErrorCategory::IntegerRange))?;
    (value <= max)
        .then_some(ContractValue::u64(value))
        .ok_or(SemanticError(SemanticErrorCategory::IntegerRange))
}

macro_rules! wide_integer {
    ($name:ident, $type:ty, $constructor:path, $signed:literal) => {
        fn $name(tree: OpaqueTree) -> Result<ContractValue, SemanticError> {
            let OpaqueTree::String(text) = tree else {
                return failure(SemanticErrorCategory::RepresentationMismatch);
            };
            if !canonical_integer(&text, $signed) {
                return failure(SemanticErrorCategory::NonCanonicalInteger);
            }
            text.parse::<$type>()
                .map($constructor)
                .map_err(|_| SemanticError(SemanticErrorCategory::IntegerRange))
        }
    };
}

wide_integer!(wide_signed, i64, ContractValue::i64, true);
wide_integer!(wide_unsigned, u64, ContractValue::u64, false);

macro_rules! float {
    ($name:ident, $type:ty, $constructor:path) => {
        fn $name(tree: OpaqueTree) -> Result<ContractValue, SemanticError> {
            let OpaqueTree::Number(number) = tree else {
                return failure(SemanticErrorCategory::RepresentationMismatch);
            };
            number
                .as_str()
                .parse::<$type>()
                .ok()
                .and_then(|value| $constructor(value).ok())
                .ok_or(SemanticError(SemanticErrorCategory::NonFiniteFloat))
        }
    };
}

float!(float32, f32, ContractValue::f32);
float!(float64, f64, ContractValue::f64);

fn canonical_integer(text: &str, signed: bool) -> bool {
    let digits = if signed {
        text.strip_prefix('-').unwrap_or(text)
    } else {
        text
    };
    !digits.is_empty()
        && digits.is_ascii()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && !(text.starts_with('-') && digits == "0")
}

fn failure<T>(category: SemanticErrorCategory) -> Result<T, SemanticError> {
    Err(SemanticError(category))
}

#[cfg(test)]
mod tests {
    use super::SemanticErrorCategory as C;
    use super::*;
    use boxology_contract::{
        ContractValue as Value, FieldDescriptor, OpaqueNumber, ValueRef, VariantDescriptor,
        VariantPayload,
    };

    const ROLES: [DecodeRole; 2] = [DecodeRole::ProviderInput, DecodeRole::ConsumerOutput];
    const SENTINEL: &str = "DO_NOT_LEAK";

    fn number(text: &str) -> OpaqueTree {
        OpaqueTree::Number(OpaqueNumber::new(text).unwrap())
    }

    fn ok_both(tree: OpaqueTree, descriptor: &TypeDescriptor, expected: Value) {
        for role in ROLES {
            assert_eq!(
                decode_tree(tree.clone(), descriptor, role),
                Ok(SlotValue::Value(expected.clone()))
            );
        }
    }

    fn error_both(
        tree: OpaqueTree,
        descriptor: &TypeDescriptor,
        category: C,
        forbidden: Option<&str>,
    ) {
        for role in ROLES {
            let error = decode_tree(tree.clone(), descriptor, role).unwrap_err();
            assert_eq!(error.category(), category);
            assert_eq!(error.to_string(), category.message());
            if let Some(forbidden) = forbidden {
                assert!(!format!("{error:?}").contains(forbidden));
                assert!(!error.to_string().contains(forbidden));
            }
            assert!(error.source().is_none());
        }
    }

    macro_rules! error_helper {
        ($name:ident, $category:ident) => {
            fn $name(tree: OpaqueTree, descriptor: &TypeDescriptor, forbidden: Option<&str>) {
                error_both(tree, descriptor, C::$category, forbidden);
            }
        };
    }

    error_helper!(representation, RepresentationMismatch);
    error_helper!(noncanonical, NonCanonicalInteger);
    error_helper!(range, IntegerRange);
    error_helper!(nonfinite, NonFiniteFloat);
    error_helper!(null, NullConformance);
    error_helper!(unsupported, UnsupportedDescriptor);

    fn supported_descriptors() -> Vec<TypeDescriptor> {
        vec![
            TypeDescriptor::bool(),
            TypeDescriptor::string(),
            TypeDescriptor::i8(),
            TypeDescriptor::i16(),
            TypeDescriptor::i32(),
            TypeDescriptor::i64(),
            TypeDescriptor::u8(),
            TypeDescriptor::u16(),
            TypeDescriptor::u32(),
            TypeDescriptor::u64(),
            TypeDescriptor::f32(),
            TypeDescriptor::f64(),
        ]
    }

    fn f32_both(token: &str, expected: f32) {
        for role in ROLES {
            let slot = decode_tree(number(token), &TypeDescriptor::f32(), role).unwrap();
            let SlotValue::Value(value) = slot else {
                panic!("float decoded as null");
            };
            let ValueRef::F32(actual) = value.view() else {
                panic!("float decoded at the wrong width");
            };
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
    }

    fn f64_both(token: &str, expected: f64) {
        for role in ROLES {
            let slot = decode_tree(number(token), &TypeDescriptor::f64(), role).unwrap();
            let SlotValue::Value(value) = slot else {
                panic!("float decoded as null");
            };
            let ValueRef::F64(actual) = value.view() else {
                panic!("float decoded at the wrong width");
            };
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
    }

    #[test]
    fn supported_boundaries_decode_in_both_roles() {
        let boolean = TypeDescriptor::bool();
        for value in [false, true] {
            ok_both(OpaqueTree::Bool(value), &boolean, Value::bool(value));
        }
        let string = TypeDescriptor::string();
        let ada = crate::syntax::parse(
            br#""Ada""#,
            crate::syntax::SyntaxLimits(5, crate::syntax::DEFAULT_DEPTH_LIMIT),
        )
        .unwrap();
        ok_both(ada, &string, Value::string("Ada"));
        let empty = OpaqueTree::String(String::new());
        ok_both(empty, &string, Value::string(""));
        for (descriptor, token, value) in [
            (TypeDescriptor::i8(), "-128", -128),
            (TypeDescriptor::i8(), "127", 127),
            (TypeDescriptor::i16(), "-32768", -32768),
            (TypeDescriptor::i16(), "32767", 32767),
            (TypeDescriptor::i32(), "-2147483648", i32::MIN.into()),
            (TypeDescriptor::i32(), "2147483647", i32::MAX.into()),
        ] {
            ok_both(number(token), &descriptor, Value::i64(value));
        }
        for (descriptor, token, value) in [
            (TypeDescriptor::u8(), "0", 0),
            (TypeDescriptor::u8(), "255", 255),
            (TypeDescriptor::u16(), "0", 0),
            (TypeDescriptor::u16(), "65535", 65535),
            (TypeDescriptor::u32(), "0", 0),
            (TypeDescriptor::u32(), "4294967295", u32::MAX.into()),
        ] {
            ok_both(number(token), &descriptor, Value::u64(value));
        }
        for value in [0, 1, i64::MIN, i64::MAX, -1] {
            ok_both(
                OpaqueTree::String(value.to_string()),
                &TypeDescriptor::i64(),
                Value::i64(value),
            );
        }
        for value in [0, 1, u64::MAX] {
            ok_both(
                OpaqueTree::String(value.to_string()),
                &TypeDescriptor::u64(),
                Value::u64(value),
            );
        }
    }

    #[test]
    fn integer_rejections_are_exact_and_payload_free() {
        let wrong = OpaqueTree::Object(vec![(SENTINEL.into(), OpaqueTree::Null)]);
        for descriptor in supported_descriptors() {
            representation(wrong.clone(), &descriptor, Some(SENTINEL));
            null(OpaqueTree::Null, &descriptor, None);
        }
        for (tree, descriptor, token) in [
            (
                OpaqueTree::String("bool-kind".into()),
                TypeDescriptor::bool(),
                "bool-kind",
            ),
            (OpaqueTree::Bool(true), TypeDescriptor::string(), "true"),
            (
                OpaqueTree::String("signed-string".into()),
                TypeDescriptor::i32(),
                "signed-string",
            ),
            (
                OpaqueTree::String("unsigned-string".into()),
                TypeDescriptor::u32(),
                "unsigned-string",
            ),
            (number("42"), TypeDescriptor::i64(), "42"),
            (number("43"), TypeDescriptor::u64(), "43"),
        ] {
            representation(tree, &descriptor, Some(token));
        }
        for (descriptor, token) in [
            (TypeDescriptor::i8(), "-129"),
            (TypeDescriptor::i8(), "128"),
            (TypeDescriptor::i16(), "-32769"),
            (TypeDescriptor::i16(), "32768"),
            (TypeDescriptor::i32(), "-2147483649"),
            (TypeDescriptor::i32(), "2147483648"),
            (TypeDescriptor::u8(), "256"),
            (TypeDescriptor::u8(), "-1"),
            (TypeDescriptor::u16(), "65536"),
            (TypeDescriptor::u16(), "-1"),
            (TypeDescriptor::u32(), "4294967296"),
            (TypeDescriptor::u32(), "-1"),
        ] {
            range(number(token), &descriptor, Some(token));
        }
        for descriptor in [TypeDescriptor::i32(), TypeDescriptor::u32()] {
            for token in ["1.0", "1e0"] {
                noncanonical(number(token), &descriptor, Some(token));
            }
        }
    }

    #[test]
    fn wide_integer_strings_are_canonical_and_range_checked() {
        for text in ["", "+1", "01", "-01", "-0", " 1", "1 ", "١"] {
            noncanonical(
                OpaqueTree::String(text.into()),
                &TypeDescriptor::i64(),
                (!text.is_empty()).then_some(text),
            );
        }
        for text in ["", "+1", "01", "-01", "-0", "-1", " 1", "1 ", "١"] {
            noncanonical(
                OpaqueTree::String(text.into()),
                &TypeDescriptor::u64(),
                (!text.is_empty()).then_some(text),
            );
        }
        for (descriptor, text) in [
            (TypeDescriptor::i64(), "9223372036854775808"),
            (TypeDescriptor::i64(), "-9223372036854775809"),
            (TypeDescriptor::u64(), "18446744073709551616"),
        ] {
            range(OpaqueTree::String(text.into()), &descriptor, Some(text));
        }
    }

    #[test]
    fn finite_floats_decode_at_the_declared_width_in_both_roles() {
        for (token, expected) in [
            ("1", 1.0),
            ("0.5", 0.5),
            ("1e2", 100.0),
            ("3.4028235e38", f32::MAX),
            ("-3.4028235e38", -f32::MAX),
            ("16777217", 16_777_216.0),
            ("0", 0.0),
            ("-0", -0.0),
        ] {
            f32_both(token, expected);
        }
        for (token, expected) in [
            ("-2", -2.0),
            ("0.25", 0.25),
            ("1e-3", 0.001),
            ("1.7976931348623157e308", f64::MAX),
            ("-1.7976931348623157e308", -f64::MAX),
            ("16777217", 16_777_217.0),
            ("0", 0.0),
            ("-0", -0.0),
        ] {
            f64_both(token, expected);
        }
    }

    #[test]
    fn float_failures_are_exact_payload_free_and_role_symmetric() {
        for (descriptor, token) in [
            (TypeDescriptor::f32(), "1e39"),
            (TypeDescriptor::f32(), "-1e39"),
            (TypeDescriptor::f64(), "1e309"),
            (TypeDescriptor::f64(), "-1e309"),
        ] {
            nonfinite(number(token), &descriptor, Some(token));
        }
        for descriptor in [TypeDescriptor::f32(), TypeDescriptor::f64()] {
            representation(OpaqueTree::String("1.25".into()), &descriptor, Some("1.25"));
            representation(
                OpaqueTree::String(SENTINEL.into()),
                &descriptor,
                Some(SENTINEL),
            );
            representation(OpaqueTree::Bool(true), &descriptor, None);
            representation(
                OpaqueTree::List(vec![OpaqueTree::String(SENTINEL.into())]),
                &descriptor,
                Some(SENTINEL),
            );
        }
    }

    #[test]
    fn unsupported_descriptors_precede_payload_inspection() {
        let field = FieldDescriptor::new("field", TypeDescriptor::bool(), None);
        let variant = VariantDescriptor::new("variant", VariantPayload::Unit, None);
        let secret = TypeDescriptor::secret(TypeDescriptor::i8()).unwrap();
        let descriptors = [
            TypeDescriptor::blob(),
            secret.clone(),
            TypeDescriptor::optional(TypeDescriptor::i8()).unwrap(),
            TypeDescriptor::tri_state(TypeDescriptor::i8()).unwrap(),
            TypeDescriptor::list(TypeDescriptor::i8()).unwrap(),
            TypeDescriptor::map(TypeDescriptor::i8()).unwrap(),
            TypeDescriptor::structure([field]).unwrap(),
            TypeDescriptor::enumeration([variant]).unwrap(),
            TypeDescriptor::list(secret).unwrap(),
        ];
        let payload = OpaqueTree::String(SENTINEL.into());
        for descriptor in descriptors {
            unsupported(payload.clone(), &descriptor, Some(SENTINEL));
        }
    }
}
