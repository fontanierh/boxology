use std::{error::Error, fmt};

use boxology_contract::{
    ConformanceErrorKind, ContractValue, DecodeRole, DescriptorRef, FieldDescriptor, OpaqueTree,
    SlotValue, TypeDescriptor,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticErrorCategory {
    RepresentationMismatch,
    NonCanonicalInteger,
    IntegerRange,
    NonFiniteFloat,
    DuplicateObjectKey,
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
            Self::DuplicateObjectKey => "duplicate object key",
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
        tree => SlotValue::Value(decode_value(tree, descriptor, role)?),
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
    match descriptor.view() {
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
        | DescriptorRef::F64 => true,
        DescriptorRef::Optional(inner)
        | DescriptorRef::TriState(inner)
        | DescriptorRef::List(inner)
        | DescriptorRef::Map(inner) => is_supported(inner),
        DescriptorRef::Struct(fields) => {
            fields.iter().all(|field| is_supported(field.descriptor()))
        }
        _ => false,
    }
}

fn decode_value(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
    role: DecodeRole,
) -> Result<ContractValue, SemanticError> {
    if matches!(tree, OpaqueTree::Null) {
        return Ok(ContractValue::null());
    }
    match descriptor.view() {
        DescriptorRef::Optional(inner) | DescriptorRef::TriState(inner) => {
            decode_value(tree, inner, role)
        }
        DescriptorRef::List(inner) => decode_list(tree, inner, role),
        DescriptorRef::Map(inner) => decode_map(tree, inner, role),
        DescriptorRef::Struct(fields) => decode_struct(tree, fields, role),
        _ => decode_scalar(tree, descriptor),
    }
}

fn decode_list(
    tree: OpaqueTree,
    element: &TypeDescriptor,
    role: DecodeRole,
) -> Result<ContractValue, SemanticError> {
    let OpaqueTree::List(items) = tree else {
        return failure(SemanticErrorCategory::RepresentationMismatch);
    };
    items
        .into_iter()
        .map(|item| decode_value(item, element, role))
        .collect::<Result<Vec<_>, _>>()
        .map(ContractValue::list)
}

fn decode_map(
    tree: OpaqueTree,
    element: &TypeDescriptor,
    role: DecodeRole,
) -> Result<ContractValue, SemanticError> {
    let entries = object_entries(tree)?;
    let entries = entries
        .into_iter()
        .map(|(key, value)| decode_value(value, element, role).map(|value| (key, value)))
        .collect::<Result<Vec<_>, _>>()?;
    ContractValue::object(entries)
        .map_err(|_| SemanticError(SemanticErrorCategory::DuplicateObjectKey))
}

fn object_entries(tree: OpaqueTree) -> Result<Vec<(String, OpaqueTree)>, SemanticError> {
    let OpaqueTree::Object(entries) = tree else {
        return failure(SemanticErrorCategory::RepresentationMismatch);
    };
    for (index, (key, _)) in entries.iter().enumerate() {
        if entries[..index].iter().any(|(earlier, _)| earlier == key) {
            return failure(SemanticErrorCategory::DuplicateObjectKey);
        }
    }
    Ok(entries)
}

fn decode_struct(
    tree: OpaqueTree,
    fields: &[FieldDescriptor],
    role: DecodeRole,
) -> Result<ContractValue, SemanticError> {
    let mut output = Vec::new();
    for (name, tree) in object_entries(tree)? {
        let Some(field) = fields.iter().find(|field| field.name() == name) else {
            if role == DecodeRole::ProviderInput {
                return failure(SemanticErrorCategory::RepresentationMismatch);
            }
            continue;
        };
        output.push((name, decode_value(tree, field.descriptor(), role)?));
    }
    ContractValue::object(output)
        .map_err(|_| SemanticError(SemanticErrorCategory::DuplicateObjectKey))
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
    use boxology_contract::{ContractValue as Value, FieldDescriptor, OpaqueNumber, ValueRef};

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
            error_role(tree.clone(), descriptor, role, category, forbidden);
        }
    }

    fn error_role(
        tree: OpaqueTree,
        descriptor: &TypeDescriptor,
        role: DecodeRole,
        category: C,
        forbidden: Option<&str>,
    ) {
        let error = decode_tree(tree, descriptor, role).unwrap_err();
        assert_eq!(error.category(), category);
        assert_eq!(error.to_string(), category.message());
        if let Some(forbidden) = forbidden {
            assert!(!format!("{error:?}").contains(forbidden));
            assert!(!error.to_string().contains(forbidden));
        }
        assert!(error.source().is_none());
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
    error_helper!(duplicate, DuplicateObjectKey);
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

    fn slot_both(tree: OpaqueTree, descriptor: &TypeDescriptor, expected: SlotValue) {
        for role in ROLES {
            assert_eq!(
                decode_tree(tree.clone(), descriptor, role),
                Ok(expected.clone())
            );
        }
    }

    fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        Value::object(entries.into_iter().map(|(key, value)| (key.into(), value))).unwrap()
    }

    fn field(name: &str, descriptor: TypeDescriptor) -> FieldDescriptor {
        FieldDescriptor::new(name, descriptor, None)
    }

    fn structure(fields: impl IntoIterator<Item = FieldDescriptor>) -> TypeDescriptor {
        TypeDescriptor::structure(fields).unwrap()
    }

    fn tree(entries: impl IntoIterator<Item = (&'static str, OpaqueTree)>) -> OpaqueTree {
        OpaqueTree::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
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
    fn top_level_presence_wrappers_preserve_null_and_value_without_missing() {
        let string = TypeDescriptor::string();
        for descriptor in [
            TypeDescriptor::optional(string.clone()).unwrap(),
            TypeDescriptor::tri_state(string.clone()).unwrap(),
        ] {
            slot_both(OpaqueTree::Null, &descriptor, SlotValue::Null);
            ok_both(
                OpaqueTree::String("value".into()),
                &descriptor,
                Value::string("value"),
            );
        }

        let list = TypeDescriptor::list(string.clone()).unwrap();
        for descriptor in [
            TypeDescriptor::optional(list.clone()).unwrap(),
            TypeDescriptor::tri_state(list).unwrap(),
        ] {
            slot_both(OpaqueTree::Null, &descriptor, SlotValue::Null);
            ok_both(
                OpaqueTree::List(vec![OpaqueTree::String("item".into())]),
                &descriptor,
                Value::list([Value::string("item")]),
            );
        }

        let map = TypeDescriptor::map(string).unwrap();
        for descriptor in [
            TypeDescriptor::optional(map.clone()).unwrap(),
            TypeDescriptor::tri_state(map).unwrap(),
        ] {
            slot_both(OpaqueTree::Null, &descriptor, SlotValue::Null);
            ok_both(
                OpaqueTree::Object(vec![("key".into(), OpaqueTree::String("value".into()))]),
                &descriptor,
                object([("key", Value::string("value"))]),
            );
        }
    }

    #[test]
    fn lists_and_maps_preserve_empty_order_nested_values_and_optional_nulls() {
        let optional_string = TypeDescriptor::optional(TypeDescriptor::string()).unwrap();
        let list_optional = TypeDescriptor::list(optional_string.clone()).unwrap();
        ok_both(
            OpaqueTree::List(vec![OpaqueTree::Null, OpaqueTree::String("item".into())]),
            &list_optional,
            Value::list([Value::null(), Value::string("item")]),
        );
        ok_both(
            OpaqueTree::List(Vec::new()),
            &list_optional,
            Value::list([]),
        );

        let map_optional = TypeDescriptor::map(optional_string).unwrap();
        ok_both(
            OpaqueTree::Object(vec![
                ("z arbitrary key".into(), OpaqueTree::Null),
                ("".into(), OpaqueTree::String("second".into())),
            ]),
            &map_optional,
            object([
                ("z arbitrary key", Value::null()),
                ("", Value::string("second")),
            ]),
        );
        ok_both(OpaqueTree::Object(Vec::new()), &map_optional, object([]));

        let nested = TypeDescriptor::map(TypeDescriptor::list(map_optional).unwrap()).unwrap();
        ok_both(
            OpaqueTree::Object(vec![(
                "outer".into(),
                OpaqueTree::List(vec![OpaqueTree::Object(vec![
                    ("first".into(), OpaqueTree::String("one".into())),
                    ("second".into(), OpaqueTree::Null),
                ])]),
            )]),
            &nested,
            object([(
                "outer",
                Value::list([object([
                    ("first", Value::string("one")),
                    ("second", Value::null()),
                ])]),
            )]),
        );
    }

    #[test]
    fn container_failures_follow_outer_and_input_order_precedence() {
        let list_i8 = TypeDescriptor::list(TypeDescriptor::i8()).unwrap();
        let map_i8 = TypeDescriptor::map(TypeDescriptor::i8()).unwrap();
        representation(
            OpaqueTree::Object(vec![(SENTINEL.into(), OpaqueTree::Null)]),
            &list_i8,
            Some(SENTINEL),
        );
        representation(
            OpaqueTree::List(vec![OpaqueTree::String(SENTINEL.into())]),
            &map_i8,
            Some(SENTINEL),
        );
        null(OpaqueTree::List(vec![OpaqueTree::Null]), &list_i8, None);
        null(
            OpaqueTree::Object(vec![("key".into(), OpaqueTree::Null)]),
            &map_i8,
            None,
        );

        for (tree, category, token) in [
            (
                OpaqueTree::List(vec![number("128"), number("1.0")]),
                C::IntegerRange,
                "128",
            ),
            (
                OpaqueTree::List(vec![number("1.0"), number("128")]),
                C::NonCanonicalInteger,
                "1.0",
            ),
            (
                OpaqueTree::Object(vec![
                    ("first".into(), number("128")),
                    ("second".into(), number("1.0")),
                ]),
                C::IntegerRange,
                "128",
            ),
            (
                OpaqueTree::Object(vec![
                    ("first".into(), number("1.0")),
                    ("second".into(), number("128")),
                ]),
                C::NonCanonicalInteger,
                "1.0",
            ),
        ] {
            let descriptor = if matches!(tree, OpaqueTree::List(_)) {
                &list_i8
            } else {
                &map_i8
            };
            error_both(tree, descriptor, category, Some(token));
        }
    }

    #[test]
    fn duplicate_map_keys_win_before_value_lowering_without_leaking() {
        let descriptor = TypeDescriptor::map(TypeDescriptor::i8()).unwrap();
        duplicate(
            OpaqueTree::Object(vec![
                (SENTINEL.into(), OpaqueTree::String(SENTINEL.into())),
                (SENTINEL.into(), number("128")),
            ]),
            &descriptor,
            Some(SENTINEL),
        );
    }

    #[test]
    fn structs_apply_presence_recursion_roles_and_input_order() {
        let empty = structure([]);
        assert!(is_supported(&empty));
        ok_both(tree([]), &empty, object([]));
        let nested = structure([field(
            "items",
            TypeDescriptor::list(TypeDescriptor::map(TypeDescriptor::bool()).unwrap()).unwrap(),
        )]);
        let optional = TypeDescriptor::optional(TypeDescriptor::string()).unwrap();
        let tri = TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap();
        let descriptor = structure([
            field("required", TypeDescriptor::i8()),
            field("optional", optional),
            field("tri", tri),
            field("nested", nested),
        ]);
        let nested_tree = tree([(
            "items",
            OpaqueTree::List(vec![tree([("flag", OpaqueTree::Bool(true))])]),
        )]);
        let nested_value = object([(
            "items",
            Value::list([object([("flag", Value::bool(true))])]),
        )]);
        ok_both(
            tree([
                ("tri", OpaqueTree::Null),
                ("nested", nested_tree),
                ("required", number("7")),
                ("optional", OpaqueTree::String("yes".into())),
            ]),
            &descriptor,
            object([
                ("tri", Value::null()),
                ("nested", nested_value),
                ("required", Value::i64(7)),
                ("optional", Value::string("yes")),
            ]),
        );
        null(OpaqueTree::Null, &descriptor, None);
        for wrapped in [
            TypeDescriptor::optional(descriptor.clone()).unwrap(),
            TypeDescriptor::tri_state(descriptor.clone()).unwrap(),
        ] {
            slot_both(OpaqueTree::Null, &wrapped, SlotValue::Null);
        }
        representation(OpaqueTree::List(vec![]), &descriptor, None);
        representation(tree([]), &descriptor, None);
        null(
            tree([
                ("required", OpaqueTree::Null),
                ("nested", tree([("items", OpaqueTree::List(vec![]))])),
            ]),
            &descriptor,
            None,
        );
        null(
            tree([
                ("required", number("1")),
                ("optional", OpaqueTree::Null),
                ("nested", tree([("items", OpaqueTree::List(vec![]))])),
            ]),
            &descriptor,
            None,
        );
        let optional = TypeDescriptor::optional(TypeDescriptor::bool()).unwrap();
        let tri = TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap();
        let presence = structure([field("optional", optional), field("tri", tri)]);
        ok_both(tree([]), &presence, object([]));
        ok_both(
            tree([("tri", OpaqueTree::Null)]),
            &presence,
            object([("tri", Value::null())]),
        );
        ok_both(
            tree([("tri", OpaqueTree::Bool(true))]),
            &presence,
            object([("tri", Value::bool(true))]),
        );

        let tolerant = structure([
            field("first", TypeDescriptor::string()),
            field("second", TypeDescriptor::string()),
        ]);
        let unknown = tree([
            (SENTINEL, OpaqueTree::String(SENTINEL.into())),
            (SENTINEL, number("128")),
        ]);
        let input = tree([
            ("second", OpaqueTree::String("two".into())),
            ("unknown", unknown.clone()),
            ("first", OpaqueTree::String("one".into())),
        ]);
        error_role(
            input.clone(),
            &tolerant,
            DecodeRole::ProviderInput,
            C::RepresentationMismatch,
            Some(SENTINEL),
        );
        let consumer = |input| decode_tree(input, &tolerant, DecodeRole::ConsumerOutput);
        assert_eq!(
            consumer(input.clone()),
            Ok(SlotValue::Value(object([
                ("second", Value::string("two")),
                ("first", Value::string("one")),
            ])))
        );
        assert_eq!(
            consumer(input),
            consumer(tree([
                ("second", OpaqueTree::String("two".into())),
                ("unknown", OpaqueTree::Null),
                ("first", OpaqueTree::String("one".into())),
            ]))
        );

        let recursive = TypeDescriptor::list(
            TypeDescriptor::map(structure([field(
                "known",
                TypeDescriptor::optional(TypeDescriptor::bool()).unwrap(),
            )]))
            .unwrap(),
        )
        .unwrap();
        let input = OpaqueTree::List(vec![tree([("key", tree([("unknown", unknown)]))])]);
        error_role(
            input.clone(),
            &recursive,
            DecodeRole::ProviderInput,
            C::RepresentationMismatch,
            Some(SENTINEL),
        );
        assert_eq!(
            decode_tree(input, &recursive, DecodeRole::ConsumerOutput),
            Ok(SlotValue::Value(Value::list([object([(
                "key",
                object([]),
            )])])))
        );
    }

    #[test]
    fn struct_failures_follow_support_duplicate_and_supplied_entry_precedence() {
        let descriptor = structure([
            field("bad", TypeDescriptor::i8()),
            field("missing", TypeDescriptor::bool()),
        ]);
        duplicate(
            tree([
                ("bad", number("128")),
                (SENTINEL, OpaqueTree::Null),
                (SENTINEL, OpaqueTree::String(SENTINEL.into())),
            ]),
            &descriptor,
            Some(SENTINEL),
        );
        duplicate(
            tree([("bad", number("128")), ("bad", OpaqueTree::Null)]),
            &descriptor,
            Some("128"),
        );
        error_both(
            tree([("bad", number("128")), ("unknown", OpaqueTree::Null)]),
            &descriptor,
            C::IntegerRange,
            Some("128"),
        );
        let unknown_first = tree([
            ("unknown", OpaqueTree::String(SENTINEL.into())),
            ("bad", number("128")),
        ]);
        error_role(
            unknown_first.clone(),
            &descriptor,
            DecodeRole::ProviderInput,
            C::RepresentationMismatch,
            Some(SENTINEL),
        );
        error_role(
            unknown_first,
            &descriptor,
            DecodeRole::ConsumerOutput,
            C::IntegerRange,
            Some("128"),
        );
        range(tree([("bad", number("128"))]), &descriptor, Some("128"));
    }

    #[test]
    fn recursive_support_inventory_is_complete_before_payload_inspection() {
        let strings = TypeDescriptor::list(TypeDescriptor::string()).unwrap();
        let structure = structure([
            field("value", TypeDescriptor::bool()),
            field("items", strings),
        ]);
        let supported = [
            TypeDescriptor::optional(TypeDescriptor::i8()).unwrap(),
            TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap(),
            TypeDescriptor::list(TypeDescriptor::optional(TypeDescriptor::f32()).unwrap()).unwrap(),
            TypeDescriptor::map(TypeDescriptor::list(TypeDescriptor::u16()).unwrap()).unwrap(),
            TypeDescriptor::optional(
                TypeDescriptor::map(TypeDescriptor::list(TypeDescriptor::bool()).unwrap()).unwrap(),
            )
            .unwrap(),
            structure.clone(),
            TypeDescriptor::map(TypeDescriptor::list(structure).unwrap()).unwrap(),
        ];
        assert!(supported.iter().all(is_supported));
        let unsupported =
            TypeDescriptor::list(TypeDescriptor::secret(TypeDescriptor::string()).unwrap())
                .unwrap();
        assert!(!is_supported(&unsupported));
    }

    #[test]
    fn unsupported_descriptors_precede_payload_inspection() {
        let secret = TypeDescriptor::secret(TypeDescriptor::i8()).unwrap();
        let optional_secret = TypeDescriptor::optional(secret.clone()).unwrap();
        let secret_optional =
            TypeDescriptor::secret(TypeDescriptor::optional(TypeDescriptor::string()).unwrap())
                .unwrap();
        let list_secret = TypeDescriptor::list(secret.clone()).unwrap();
        let map_secret = TypeDescriptor::map(secret.clone()).unwrap();
        let deep_secret =
            TypeDescriptor::list(TypeDescriptor::map(optional_secret.clone()).unwrap()).unwrap();
        let structure = structure([field("field", optional_secret.clone())]);
        let enumeration = TypeDescriptor::enumeration([boxology_contract::VariantDescriptor::new(
            "variant",
            boxology_contract::VariantPayload::Unit,
            None,
        )])
        .unwrap();
        unsupported(OpaqueTree::Null, &structure, None);
        unsupported(
            tree([
                (SENTINEL, OpaqueTree::String(SENTINEL.into())),
                (SENTINEL, OpaqueTree::Null),
            ]),
            &structure,
            Some(SENTINEL),
        );
        unsupported(
            tree([("field", OpaqueTree::String(SENTINEL.into()))]),
            &structure,
            Some(SENTINEL),
        );
        unsupported(OpaqueTree::Null, &optional_secret, None);
        unsupported(OpaqueTree::Bool(true), &list_secret, None);
        unsupported(
            OpaqueTree::Object(vec![
                (SENTINEL.into(), OpaqueTree::String(SENTINEL.into())),
                (SENTINEL.into(), OpaqueTree::Null),
            ]),
            &map_secret,
            Some(SENTINEL),
        );
        unsupported(
            OpaqueTree::List(vec![OpaqueTree::String(SENTINEL.into())]),
            &list_secret,
            Some(SENTINEL),
        );
        let descriptors = [
            TypeDescriptor::blob(),
            secret.clone(),
            optional_secret.clone(),
            secret_optional,
            list_secret,
            map_secret,
            TypeDescriptor::map(optional_secret.clone()).unwrap(),
            deep_secret,
            TypeDescriptor::list(TypeDescriptor::blob()).unwrap(),
            structure.clone(),
            TypeDescriptor::map(structure).unwrap(),
            TypeDescriptor::structure([field("event", enumeration.clone())]).unwrap(),
            TypeDescriptor::map(enumeration.clone()).unwrap(),
            TypeDescriptor::structure([field("blob", TypeDescriptor::blob())]).unwrap(),
            enumeration.clone(),
            TypeDescriptor::list(enumeration).unwrap(),
        ];
        for descriptor in descriptors {
            unsupported(
                OpaqueTree::Object(vec![(SENTINEL.into(), OpaqueTree::String(SENTINEL.into()))]),
                &descriptor,
                Some(SENTINEL),
            );
        }
    }
}
