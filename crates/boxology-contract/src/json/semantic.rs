use std::{error::Error, fmt};

use crate::{
    ConformanceErrorKind, ContractValue, DecodeRole, DescriptorRef, FieldDescriptor, OpaquePayload,
    OpaqueTree, SlotValue, TypeDescriptor, VariantDescriptor, VariantPayload,
};
use base64::{
    Engine as _, alphabet,
    engine::{DecodePaddingMode, general_purpose::GeneralPurposeConfig},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Stable categories for descriptor-guided JSON projection failures.
pub enum SemanticErrorKind {
    /// JSON shape or value kind did not match the descriptor.
    RepresentationMismatch,
    /// A wide integer string was not in canonical decimal form.
    NonCanonicalInteger,
    /// An integer exceeded the descriptor's declared width.
    IntegerRange,
    /// A JSON number decoded to a non-finite float.
    NonFiniteFloat,
    /// An object contained a duplicate decoded key.
    DuplicateObjectKey,
    /// Null is not admitted by the descriptor at this position.
    NullConformance,
    /// The descriptor contains a shape the current codec does not support.
    UnsupportedDescriptor,
}

impl SemanticErrorKind {
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
/// A payload-free descriptor-guided JSON projection failure.
pub struct SemanticError(SemanticErrorKind);

impl SemanticError {
    /// Returns the stable failure category without retaining contract data.
    pub fn kind(&self) -> SemanticErrorKind {
        self.0
    }
}

impl fmt::Display for SemanticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0.message())
    }
}

impl Error for SemanticError {}

/// Projects an already parsed JSON tree through a descriptor and decode role.
///
/// Most consumers should call [`super::decode`]; this lower-level entry point
/// lets bindings parse a larger protocol envelope only once.
pub fn decode_tree(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
    role: DecodeRole,
) -> Result<SlotValue, SemanticError> {
    if !is_supported(descriptor) {
        return failure(SemanticErrorKind::UnsupportedDescriptor);
    }
    let slot = match (tree, descriptor.view()) {
        (tree @ OpaqueTree::Null, DescriptorRef::Enum(variants)) => {
            SlotValue::Value(decode_enum(tree, variants, role)?)
        }
        (tree @ OpaqueTree::Null, DescriptorRef::Secret(_)) => {
            SlotValue::Value(decode_value(tree, descriptor, role)?)
        }
        (OpaqueTree::Null, _) => SlotValue::Null,
        (tree, _) => SlotValue::Value(decode_value(tree, descriptor, role)?),
    };
    descriptor.conform(role, slot).map_err(|error| {
        let category = if matches!(error.kind(), ConformanceErrorKind::UnexpectedNull) {
            SemanticErrorKind::NullConformance
        } else {
            SemanticErrorKind::RepresentationMismatch
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
        | DescriptorRef::F64
        | DescriptorRef::Blob => true,
        DescriptorRef::Secret(inner)
        | DescriptorRef::Optional(inner)
        | DescriptorRef::TriState(inner)
        | DescriptorRef::List(inner)
        | DescriptorRef::Map(inner) => is_supported(inner),
        DescriptorRef::Struct(fields) => {
            fields.iter().all(|field| is_supported(field.descriptor()))
        }
        DescriptorRef::Enum(variants) => variants.iter().all(|variant| match variant.payload() {
            VariantPayload::Unit => true,
            VariantPayload::Value(descriptor) => is_supported(descriptor),
        }),
    }
}

fn decode_value(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
    role: DecodeRole,
) -> Result<ContractValue, SemanticError> {
    if let DescriptorRef::Secret(inner) = descriptor.view() {
        return decode_value(tree, inner, role).map(ContractValue::sensitive);
    }
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
        DescriptorRef::Enum(variants) => decode_enum(tree, variants, role),
        _ => decode_scalar(tree, descriptor),
    }
}

fn decode_list(
    tree: OpaqueTree,
    element: &TypeDescriptor,
    role: DecodeRole,
) -> Result<ContractValue, SemanticError> {
    let OpaqueTree::List(items) = tree else {
        return failure(SemanticErrorKind::RepresentationMismatch);
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
    ContractValue::object(entries).map_err(|_| SemanticError(SemanticErrorKind::DuplicateObjectKey))
}

fn object_entries(tree: OpaqueTree) -> Result<Vec<(String, OpaqueTree)>, SemanticError> {
    let OpaqueTree::Object(entries) = tree else {
        return failure(SemanticErrorKind::RepresentationMismatch);
    };
    for (index, (key, _)) in entries.iter().enumerate() {
        if entries[..index].iter().any(|(earlier, _)| earlier == key) {
            return failure(SemanticErrorKind::DuplicateObjectKey);
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
                return failure(SemanticErrorKind::RepresentationMismatch);
            }
            continue;
        };
        output.push((name, decode_value(tree, field.descriptor(), role)?));
    }
    ContractValue::object(output).map_err(|_| SemanticError(SemanticErrorKind::DuplicateObjectKey))
}

fn decode_enum(
    tree: OpaqueTree,
    variants: &[VariantDescriptor],
    role: DecodeRole,
) -> Result<ContractValue, SemanticError> {
    let entries = object_entries(tree)?;
    if entries.len() != 2
        || entries
            .iter()
            .any(|(key, _)| key != "tag" && key != "payload")
    {
        return failure(SemanticErrorKind::RepresentationMismatch);
    }
    let tag = entries.iter().find(|(key, _)| key == "tag").unwrap();
    let OpaqueTree::String(tag) = &tag.1 else {
        return failure(SemanticErrorKind::RepresentationMismatch);
    };
    let tag = tag.clone();
    let payload = entries
        .into_iter()
        .find(|(key, _)| key == "payload")
        .unwrap()
        .1;
    let Some(variant) = variants.iter().find(|variant| variant.tag() == tag) else {
        return match role {
            DecodeRole::ProviderInput => failure(SemanticErrorKind::RepresentationMismatch),
            DecodeRole::ConsumerOutput => Ok(ContractValue::enum_value(
                tag,
                SlotValue::Value(ContractValue::opaque(OpaquePayload::new(payload))),
            )),
        };
    };
    let payload = match variant.payload() {
        VariantPayload::Unit if matches!(payload, OpaqueTree::Null) => SlotValue::Null,
        VariantPayload::Unit => return failure(SemanticErrorKind::RepresentationMismatch),
        VariantPayload::Value(descriptor)
            if matches!(payload, OpaqueTree::Null)
                && matches!(
                    descriptor.view(),
                    DescriptorRef::Optional(_) | DescriptorRef::TriState(_)
                ) =>
        {
            SlotValue::Null
        }
        VariantPayload::Value(descriptor) => {
            SlotValue::Value(decode_value(payload, descriptor, role)?)
        }
    };
    Ok(ContractValue::enum_value(tag, payload))
}

fn decode_scalar(
    tree: OpaqueTree,
    descriptor: &TypeDescriptor,
) -> Result<ContractValue, SemanticError> {
    match descriptor.view() {
        DescriptorRef::Bool => match tree {
            OpaqueTree::Bool(value) => Ok(ContractValue::bool(value)),
            _ => failure(SemanticErrorKind::RepresentationMismatch),
        },
        DescriptorRef::String => match tree {
            OpaqueTree::String(value) => Ok(ContractValue::string(value)),
            _ => failure(SemanticErrorKind::RepresentationMismatch),
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
        DescriptorRef::Blob => blob(tree),
        _ => failure(SemanticErrorKind::UnsupportedDescriptor),
    }
}

fn blob(tree: OpaqueTree) -> Result<ContractValue, SemanticError> {
    let entries = object_entries(tree)?;
    let [(key, value)] = entries.as_slice() else {
        return failure(SemanticErrorKind::RepresentationMismatch);
    };
    if key != "base64" {
        return failure(SemanticErrorKind::RepresentationMismatch);
    }
    let OpaqueTree::String(encoded) = value else {
        return failure(SemanticErrorKind::RepresentationMismatch);
    };
    let config = GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical)
        .with_decode_allow_trailing_bits(false);
    let engine = base64::engine::general_purpose::GeneralPurpose::new(&alphabet::STANDARD, config);
    engine
        .decode(encoded)
        .map(ContractValue::bytes)
        .map_err(|_| SemanticError(SemanticErrorKind::RepresentationMismatch))
}

fn integer_token(tree: OpaqueTree) -> Result<String, SemanticError> {
    match tree {
        OpaqueTree::Number(number) if !number.as_str().contains(['.', 'e', 'E']) => {
            Ok(number.as_str().into())
        }
        OpaqueTree::Number(_) => failure(SemanticErrorKind::NonCanonicalInteger),
        _ => failure(SemanticErrorKind::RepresentationMismatch),
    }
}

fn signed(tree: OpaqueTree, min: i64, max: i64) -> Result<ContractValue, SemanticError> {
    let value = integer_token(tree)?
        .parse::<i64>()
        .map_err(|_| SemanticError(SemanticErrorKind::IntegerRange))?;
    (min..=max)
        .contains(&value)
        .then_some(ContractValue::i64(value))
        .ok_or(SemanticError(SemanticErrorKind::IntegerRange))
}

fn unsigned(tree: OpaqueTree, max: u64) -> Result<ContractValue, SemanticError> {
    let value = integer_token(tree)?
        .parse::<u64>()
        .map_err(|_| SemanticError(SemanticErrorKind::IntegerRange))?;
    (value <= max)
        .then_some(ContractValue::u64(value))
        .ok_or(SemanticError(SemanticErrorKind::IntegerRange))
}

macro_rules! wide_integer {
    ($name:ident, $type:ty, $constructor:path, $signed:literal) => {
        fn $name(tree: OpaqueTree) -> Result<ContractValue, SemanticError> {
            let OpaqueTree::String(text) = tree else {
                return failure(SemanticErrorKind::RepresentationMismatch);
            };
            if !canonical_integer(&text, $signed) {
                return failure(SemanticErrorKind::NonCanonicalInteger);
            }
            text.parse::<$type>()
                .map($constructor)
                .map_err(|_| SemanticError(SemanticErrorKind::IntegerRange))
        }
    };
}

wide_integer!(wide_signed, i64, ContractValue::i64, true);
wide_integer!(wide_unsigned, u64, ContractValue::u64, false);

macro_rules! float {
    ($name:ident, $type:ty, $constructor:path) => {
        fn $name(tree: OpaqueTree) -> Result<ContractValue, SemanticError> {
            let OpaqueTree::Number(number) = tree else {
                return failure(SemanticErrorKind::RepresentationMismatch);
            };
            number
                .as_str()
                .parse::<$type>()
                .ok()
                .and_then(|value| $constructor(value).ok())
                .ok_or(SemanticError(SemanticErrorKind::NonFiniteFloat))
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

fn failure<T>(category: SemanticErrorKind) -> Result<T, SemanticError> {
    Err(SemanticError(category))
}

#[cfg(test)]
mod tests {
    use super::SemanticErrorKind as C;
    use super::*;
    use crate::{
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
        assert_eq!(error.kind(), category);
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

    fn variant(tag: &str, payload: VariantPayload) -> VariantDescriptor {
        VariantDescriptor::new(tag, payload, None)
    }

    fn enumeration(variants: impl IntoIterator<Item = VariantDescriptor>) -> TypeDescriptor {
        TypeDescriptor::enumeration(variants).unwrap()
    }

    fn envelope(tag: &str, payload: OpaqueTree) -> OpaqueTree {
        tree([
            ("tag", OpaqueTree::String(tag.into())),
            ("payload", payload),
        ])
    }

    fn enum_value(tag: &str, payload: SlotValue) -> Value {
        Value::enum_value(tag, payload)
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
        let ada = crate::json::syntax::parse(
            br#""Ada""#,
            crate::json::syntax::Limits::new(5, crate::json::syntax::DEFAULT_DEPTH_LIMIT),
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
    fn known_enums_decode_all_supported_payload_shapes_and_orders() {
        let empty = enumeration([]);
        assert!(is_supported(&empty));
        error_role(
            envelope("none", OpaqueTree::Null),
            &empty,
            DecodeRole::ProviderInput,
            C::RepresentationMismatch,
            None,
        );

        let descriptor = enumeration([
            variant("EmptyName", VariantPayload::Unit),
            variant("Count", VariantPayload::Value(TypeDescriptor::i8())),
            variant(
                "Maybe",
                VariantPayload::Value(TypeDescriptor::optional(TypeDescriptor::string()).unwrap()),
            ),
            variant(
                "Record",
                VariantPayload::Value(structure([field("ok", TypeDescriptor::bool())])),
            ),
            variant(
                "Items",
                VariantPayload::Value(
                    TypeDescriptor::list(TypeDescriptor::map(TypeDescriptor::u8()).unwrap())
                        .unwrap(),
                ),
            ),
            variant(
                "Nested",
                VariantPayload::Value(enumeration([variant("Inner", VariantPayload::Unit)])),
            ),
        ]);
        let hello = br#"{"tag":"EmptyName","payload":null}"#;
        let hello = crate::json::syntax::parse(
            hello,
            crate::json::syntax::Limits::new(hello.len(), crate::json::syntax::DEFAULT_DEPTH_LIMIT),
        )
        .unwrap();
        ok_both(hello, &descriptor, enum_value("EmptyName", SlotValue::Null));
        ok_both(
            tree([
                ("payload", number("7")),
                ("tag", OpaqueTree::String("Count".into())),
            ]),
            &descriptor,
            enum_value("Count", SlotValue::Value(Value::i64(7))),
        );
        ok_both(
            envelope("Maybe", OpaqueTree::Null),
            &descriptor,
            enum_value("Maybe", SlotValue::Null),
        );
        ok_both(
            envelope("Record", tree([("ok", OpaqueTree::Bool(true))])),
            &descriptor,
            enum_value(
                "Record",
                SlotValue::Value(object([("ok", Value::bool(true))])),
            ),
        );
        ok_both(
            envelope("Items", OpaqueTree::List(vec![tree([("x", number("2"))])])),
            &descriptor,
            enum_value(
                "Items",
                SlotValue::Value(Value::list([object([("x", Value::u64(2))])])),
            ),
        );
        ok_both(
            envelope("Nested", envelope("Inner", OpaqueTree::Null)),
            &descriptor,
            enum_value(
                "Nested",
                SlotValue::Value(enum_value("Inner", SlotValue::Null)),
            ),
        );
    }

    #[test]
    fn enums_recurse_through_aggregates_with_the_same_role() {
        let event = enumeration([
            variant("Known", VariantPayload::Unit),
            variant(
                "Object",
                VariantPayload::Value(structure([field("name", TypeDescriptor::string())])),
            ),
        ]);
        let descriptor = structure([field(
            "events",
            TypeDescriptor::list(TypeDescriptor::map(event).unwrap()).unwrap(),
        )]);
        let input = tree([(
            "events",
            OpaqueTree::List(vec![tree([
                ("known", envelope("Known", OpaqueTree::Null)),
                (
                    "object",
                    envelope(
                        "Object",
                        tree([
                            ("name", OpaqueTree::String("Ada".into())),
                            ("unknown", OpaqueTree::String(SENTINEL.into())),
                        ]),
                    ),
                ),
            ])]),
        )]);
        error_role(
            input.clone(),
            &descriptor,
            DecodeRole::ProviderInput,
            C::RepresentationMismatch,
            Some(SENTINEL),
        );
        assert_eq!(
            decode_tree(input, &descriptor, DecodeRole::ConsumerOutput),
            Ok(SlotValue::Value(object([(
                "events",
                Value::list([object([
                    ("known", enum_value("Known", SlotValue::Null)),
                    (
                        "object",
                        enum_value(
                            "Object",
                            SlotValue::Value(object([("name", Value::string("Ada"))])),
                        ),
                    ),
                ])]),
            )])))
        );
    }

    #[test]
    fn unknown_consumer_payload_is_exact_opaque_and_provider_never_inspects_it() {
        let descriptor = enumeration([variant("Known", VariantPayload::Unit)]);
        let raw = br#"{"tag":"FUTURE_SECRET_TAG","payload":{"b":1.0,"a":1,"a":1e0,"nested":[null,{"x":2,"x":3}]}}"#;
        let parsed = crate::json::syntax::parse(
            raw,
            crate::json::syntax::Limits::new(raw.len(), crate::json::syntax::DEFAULT_DEPTH_LIMIT),
        )
        .unwrap();
        let OpaqueTree::Object(entries) = &parsed else {
            panic!("fixture is not an object");
        };
        let expected = entries
            .iter()
            .find(|(key, _)| key == "payload")
            .unwrap()
            .1
            .clone();
        error_role(
            parsed.clone(),
            &descriptor,
            DecodeRole::ProviderInput,
            C::RepresentationMismatch,
            Some("FUTURE_SECRET_TAG"),
        );
        let SlotValue::Value(value) =
            decode_tree(parsed, &descriptor, DecodeRole::ConsumerOutput).unwrap()
        else {
            panic!("unknown enum decoded as null");
        };
        let ValueRef::Enum { tag, payload } = value.view() else {
            panic!("unknown enum decoded at wrong shape");
        };
        assert_eq!(tag, "FUTURE_SECRET_TAG");
        let SlotValue::Value(payload) = payload else {
            panic!("unknown payload decoded as null slot");
        };
        let ValueRef::Opaque(payload) = payload.view() else {
            panic!("unknown payload was not opaque");
        };
        assert_eq!(payload.reveal(), &expected);
        assert!(!format!("{payload:?}").contains("FUTURE_SECRET_TAG"));

        let SlotValue::Value(value) = decode_tree(
            envelope("FutureNull", OpaqueTree::Null),
            &descriptor,
            DecodeRole::ConsumerOutput,
        )
        .unwrap() else {
            panic!("unknown null enum decoded as null");
        };
        let ValueRef::Enum { payload, .. } = value.view() else {
            panic!("unknown null decoded at wrong shape");
        };
        let SlotValue::Value(payload) = payload else {
            panic!("raw null was collapsed to a null slot");
        };
        let ValueRef::Opaque(payload) = payload.view() else {
            panic!("raw null was not opaque");
        };
        assert_eq!(payload.reveal(), &OpaqueTree::Null);
    }

    #[test]
    fn enum_envelopes_and_known_payloads_preserve_error_precedence() {
        let descriptor = enumeration([
            variant("Unit", VariantPayload::Unit),
            variant("Required", VariantPayload::Value(TypeDescriptor::i8())),
            variant(
                "Map",
                VariantPayload::Value(TypeDescriptor::map(TypeDescriptor::i8()).unwrap()),
            ),
            variant(
                "Struct",
                VariantPayload::Value(structure([field("x", TypeDescriptor::i8())])),
            ),
        ]);
        for malformed in [
            OpaqueTree::Null,
            OpaqueTree::List(vec![]),
            tree([]),
            tree([("tag", OpaqueTree::String("Unit".into()))]),
            tree([("payload", OpaqueTree::Null)]),
            tree([
                ("tag", OpaqueTree::String("Unit".into())),
                ("payload", OpaqueTree::Null),
                ("extra", OpaqueTree::Null),
            ]),
            tree([("tag", OpaqueTree::Null), ("payload", OpaqueTree::Null)]),
            tree([
                ("kind", OpaqueTree::String("Unit".into())),
                ("payload", OpaqueTree::Null),
            ]),
        ] {
            representation(malformed, &descriptor, None);
        }
        for duplicate_envelope in [
            tree([
                ("tag", OpaqueTree::String("Unit".into())),
                ("tag", OpaqueTree::String(SENTINEL.into())),
                ("payload", OpaqueTree::Null),
            ]),
            tree([
                ("tag", OpaqueTree::String("Unit".into())),
                ("payload", OpaqueTree::Null),
                ("payload", OpaqueTree::String(SENTINEL.into())),
            ]),
            tree([
                ("tag", OpaqueTree::String("Unit".into())),
                ("payload", OpaqueTree::Null),
                (SENTINEL, OpaqueTree::Null),
                (SENTINEL, OpaqueTree::Bool(true)),
            ]),
        ] {
            duplicate(duplicate_envelope, &descriptor, Some(SENTINEL));
        }
        representation(
            envelope("Unit", OpaqueTree::String(SENTINEL.into())),
            &descriptor,
            Some(SENTINEL),
        );
        representation(
            envelope("Required", OpaqueTree::Bool(true)),
            &descriptor,
            None,
        );
        null(envelope("Required", OpaqueTree::Null), &descriptor, None);
        range(
            envelope("Required", number("128")),
            &descriptor,
            Some("128"),
        );
        duplicate(
            envelope(
                "Map",
                tree([(SENTINEL, number("128")), (SENTINEL, OpaqueTree::Null)]),
            ),
            &descriptor,
            Some(SENTINEL),
        );
        duplicate(
            envelope(
                "Struct",
                tree([("x", number("128")), ("x", OpaqueTree::Null)]),
            ),
            &descriptor,
            Some("128"),
        );
    }

    #[test]
    fn recursive_support_inventory_is_complete_before_payload_inspection() {
        let strings = TypeDescriptor::list(TypeDescriptor::string()).unwrap();
        let structure = structure([
            field("value", TypeDescriptor::bool()),
            field("items", strings),
        ]);
        let enumeration = enumeration([
            variant("unit", VariantPayload::Unit),
            variant("value", VariantPayload::Value(structure.clone())),
        ]);
        let supported = [
            TypeDescriptor::blob(),
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
            enumeration.clone(),
            TypeDescriptor::list(TypeDescriptor::map(enumeration).unwrap()).unwrap(),
            TypeDescriptor::list(TypeDescriptor::blob()).unwrap(),
            TypeDescriptor::enumeration([variant(
                "blob",
                VariantPayload::Value(TypeDescriptor::blob()),
            )])
            .unwrap(),
            TypeDescriptor::enumeration([]).unwrap(),
        ];
        assert!(supported.iter().all(is_supported));
        let secret =
            TypeDescriptor::list(TypeDescriptor::secret(TypeDescriptor::string()).unwrap())
                .unwrap();
        assert!(is_supported(&secret));
    }

    #[test]
    fn secrets_lower_to_sensitive_values_in_both_roles() {
        let secret = TypeDescriptor::secret(TypeDescriptor::string()).unwrap();
        for role in ROLES {
            assert_eq!(
                decode_tree(OpaqueTree::String(SENTINEL.into()), &secret, role),
                Ok(SlotValue::Value(Value::sensitive(Value::string(SENTINEL))))
            );
        }
    }

    #[test]
    fn optional_and_struct_secrets_preserve_presence_and_wrapping() {
        let secret = TypeDescriptor::secret(TypeDescriptor::string()).unwrap();
        let optional = TypeDescriptor::optional(secret.clone()).unwrap();
        slot_both(OpaqueTree::Null, &optional, SlotValue::Null);
        ok_both(
            OpaqueTree::String("present".into()),
            &optional,
            Value::sensitive(Value::string("present")),
        );
        ok_both(
            tree([("secret", OpaqueTree::String("inside".into()))]),
            &structure([field("secret", secret)]),
            object([("secret", Value::sensitive(Value::string("inside")))]),
        );
    }

    #[test]
    fn nullable_secrets_keep_sensitive_null_at_every_nesting_level() {
        let nullable_secret =
            TypeDescriptor::secret(TypeDescriptor::optional(TypeDescriptor::string()).unwrap())
                .unwrap();
        slot_both(
            OpaqueTree::Null,
            &nullable_secret,
            SlotValue::Value(Value::sensitive(Value::null())),
        );
        let list = TypeDescriptor::list(nullable_secret.clone()).unwrap();
        ok_both(
            OpaqueTree::List(vec![OpaqueTree::Null]),
            &list,
            Value::list([Value::sensitive(Value::null())]),
        );
        let enumeration = enumeration([variant("secret", VariantPayload::Value(nullable_secret))]);
        ok_both(
            envelope("secret", OpaqueTree::Null),
            &enumeration,
            enum_value("secret", SlotValue::Value(Value::sensitive(Value::null()))),
        );
        representation(
            OpaqueTree::List(vec![tree([(
                SENTINEL,
                OpaqueTree::String(SENTINEL.into()),
            )])]),
            &list,
            Some(SENTINEL),
        );
    }

    #[test]
    fn secret_failures_are_redacted_and_nonsecret_values_stay_plain() {
        let secret = TypeDescriptor::secret(TypeDescriptor::string()).unwrap();
        null(OpaqueTree::Null, &secret, None);
        representation(
            tree([(SENTINEL, OpaqueTree::String(SENTINEL.into()))]),
            &secret,
            Some(SENTINEL),
        );
        ok_both(
            OpaqueTree::String("plain".into()),
            &TypeDescriptor::string(),
            Value::string("plain"),
        );
    }
}
