use std::{error::Error, fmt};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use boxology_contract::{
    DescriptorRef, OpaqueTree, SlotValue, TypeDescriptor, ValueRef, VariantDescriptor,
    VariantPayload,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EncodeErrorCategory {
    MissingValue,
    NullConformance,
    RepresentationMismatch,
    IntegerRange,
    UnsupportedDescriptor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EncodeError(EncodeErrorCategory);

impl EncodeError {
    pub(crate) fn category(&self) -> EncodeErrorCategory {
        self.0
    }
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.0 {
            EncodeErrorCategory::MissingValue => "missing result value",
            EncodeErrorCategory::NullConformance => "null violates descriptor",
            EncodeErrorCategory::RepresentationMismatch => "representation mismatch",
            EncodeErrorCategory::IntegerRange => "integer outside descriptor range",
            EncodeErrorCategory::UnsupportedDescriptor => "unsupported descriptor",
        })
    }
}

impl Error for EncodeError {}

pub(crate) fn encode_result(
    slot: &SlotValue,
    descriptor: &TypeDescriptor,
) -> Result<Vec<u8>, EncodeError> {
    if !is_supported(descriptor) {
        return failure(EncodeErrorCategory::UnsupportedDescriptor);
    }
    let mut output = br#"{"result":{"value":"#.to_vec();
    match slot {
        SlotValue::Missing => return failure(EncodeErrorCategory::MissingValue),
        SlotValue::Null
            if matches!(
                descriptor.view(),
                DescriptorRef::Optional(_) | DescriptorRef::TriState(_)
            ) =>
        {
            output.extend_from_slice(b"null")
        }
        SlotValue::Null => return failure(EncodeErrorCategory::NullConformance),
        SlotValue::Value(value) => encode_value(&mut output, value.view(), descriptor)?,
    }
    output.extend_from_slice(b"}}");
    Ok(output)
}

pub(crate) fn encode_domain(
    error_tag: &str,
    payload: &SlotValue,
    descriptor: &TypeDescriptor,
) -> Result<Vec<u8>, EncodeError> {
    if !is_supported(descriptor) {
        return failure(EncodeErrorCategory::UnsupportedDescriptor);
    }
    let DescriptorRef::Enum(variants) = descriptor.view() else {
        return mismatch();
    };
    let mut output = br#"{"error":{"kind":"domain","value":"#.to_vec();
    enumeration(&mut output, error_tag, payload, variants)?;
    output.extend_from_slice(b"}}");
    Ok(output)
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
        DescriptorRef::Optional(inner)
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
        _ => false,
    }
}

fn encode_value(
    output: &mut Vec<u8>,
    value: ValueRef<'_>,
    descriptor: &TypeDescriptor,
) -> Result<(), EncodeError> {
    if matches!(value, ValueRef::Null) {
        return if matches!(
            descriptor.view(),
            DescriptorRef::Optional(_) | DescriptorRef::TriState(_)
        ) {
            output.extend_from_slice(b"null");
            Ok(())
        } else {
            failure(EncodeErrorCategory::NullConformance)
        };
    }
    let descriptor = match descriptor.view() {
        DescriptorRef::Optional(inner) | DescriptorRef::TriState(inner) => inner,
        DescriptorRef::List(inner) => return list(output, value, inner),
        DescriptorRef::Map(inner) => return map(output, value, inner),
        DescriptorRef::Struct(fields) => return structure(output, value, fields),
        DescriptorRef::Enum(variants) => {
            let ValueRef::Enum { tag, payload } = value else {
                return mismatch();
            };
            return enumeration(output, tag, payload, variants);
        }
        DescriptorRef::Bool => {
            let ValueRef::Bool(value) = value else {
                return mismatch();
            };
            output.extend_from_slice(if value { b"true" } else { b"false" });
            return Ok(());
        }
        DescriptorRef::String => return string(output, value),
        DescriptorRef::I8 => return signed(output, value, i8::MIN.into(), i8::MAX.into(), false),
        DescriptorRef::I16 => {
            return signed(output, value, i16::MIN.into(), i16::MAX.into(), false);
        }
        DescriptorRef::I32 => {
            return signed(output, value, i32::MIN.into(), i32::MAX.into(), false);
        }
        DescriptorRef::I64 => return signed(output, value, i64::MIN, i64::MAX, true),
        DescriptorRef::U8 => return unsigned(output, value, u8::MAX.into(), false),
        DescriptorRef::U16 => return unsigned(output, value, u16::MAX.into(), false),
        DescriptorRef::U32 => return unsigned(output, value, u32::MAX.into(), false),
        DescriptorRef::U64 => return unsigned(output, value, u64::MAX, true),
        DescriptorRef::F32 => return float32(output, value),
        DescriptorRef::F64 => return float64(output, value),
        DescriptorRef::Blob => return blob(output, value),
        _ => return failure(EncodeErrorCategory::UnsupportedDescriptor),
    };
    encode_value(output, value, descriptor)
}

fn enumeration(
    output: &mut Vec<u8>,
    tag: &str,
    payload: &SlotValue,
    variants: &[VariantDescriptor],
) -> Result<(), EncodeError> {
    output.extend_from_slice(br#"{"tag":"#);
    text(output, tag);
    output.extend_from_slice(b",\"payload\":");
    if let Some(variant) = variants.iter().find(|variant| variant.tag() == tag) {
        match (variant.payload(), payload) {
            (VariantPayload::Unit, SlotValue::Null) => output.extend_from_slice(b"null"),
            (VariantPayload::Unit, SlotValue::Missing) => {
                return failure(EncodeErrorCategory::MissingValue);
            }
            (VariantPayload::Unit, SlotValue::Value(_)) => return mismatch(),
            (VariantPayload::Value(_), SlotValue::Missing) => {
                return failure(EncodeErrorCategory::MissingValue);
            }
            (VariantPayload::Value(descriptor), SlotValue::Null)
                if matches!(descriptor.view(), DescriptorRef::Optional(_)) =>
            {
                output.extend_from_slice(b"null")
            }
            (VariantPayload::Value(_), SlotValue::Null) => {
                return failure(EncodeErrorCategory::NullConformance);
            }
            (VariantPayload::Value(descriptor), SlotValue::Value(value)) => {
                encode_value(output, value.view(), descriptor)?;
            }
        }
    } else {
        let SlotValue::Value(value) = payload else {
            return mismatch();
        };
        let ValueRef::Opaque(payload) = value.view() else {
            return mismatch();
        };
        opaque(output, payload.reveal());
    }
    output.push(b'}');
    Ok(())
}

fn opaque(output: &mut Vec<u8>, tree: &OpaqueTree) {
    match tree {
        OpaqueTree::Null => output.extend_from_slice(b"null"),
        OpaqueTree::Bool(value) => {
            output.extend_from_slice(if *value { b"true" } else { b"false" })
        }
        OpaqueTree::Number(value) => output.extend_from_slice(value.as_str().as_bytes()),
        OpaqueTree::String(value) => text(output, value),
        OpaqueTree::List(items) => {
            output.push(b'[');
            separated(items, output, opaque);
            output.push(b']');
        }
        OpaqueTree::Object(entries) => {
            output.push(b'{');
            for (index, (key, value)) in entries.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                text(output, key);
                output.push(b':');
                opaque(output, value);
            }
            output.push(b'}');
        }
    }
}

fn separated<T>(items: &[T], output: &mut Vec<u8>, encode: fn(&mut Vec<u8>, &T)) {
    for (index, item) in items.iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        encode(output, item);
    }
}

fn list(
    output: &mut Vec<u8>,
    value: ValueRef<'_>,
    element: &TypeDescriptor,
) -> Result<(), EncodeError> {
    let ValueRef::List(items) = value else {
        return mismatch();
    };
    output.push(b'[');
    for (index, item) in items.iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        encode_value(output, item.view(), element)?;
    }
    output.push(b']');
    Ok(())
}

fn map(
    output: &mut Vec<u8>,
    value: ValueRef<'_>,
    element: &TypeDescriptor,
) -> Result<(), EncodeError> {
    let ValueRef::Object(object) = value else {
        return mismatch();
    };
    let mut entries: Vec<_> = object.entries().collect();
    entries.sort_unstable_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    output.push(b'{');
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index != 0 {
            output.push(b',');
        }
        string(output, ValueRef::String(key))?;
        output.push(b':');
        encode_value(output, value.view(), element)?;
    }
    output.push(b'}');
    Ok(())
}

fn structure(
    output: &mut Vec<u8>,
    value: ValueRef<'_>,
    fields: &[boxology_contract::FieldDescriptor],
) -> Result<(), EncodeError> {
    let ValueRef::Object(object) = value else {
        return mismatch();
    };
    if object
        .entries()
        .any(|(name, _)| !fields.iter().any(|field| field.name() == name))
    {
        return mismatch();
    }
    output.push(b'{');
    let mut emitted = false;
    for field in fields {
        let descriptor = field.descriptor();
        let Some(value) = object.get(field.name()) else {
            if matches!(
                descriptor.view(),
                DescriptorRef::Optional(_) | DescriptorRef::TriState(_)
            ) {
                continue;
            }
            return failure(EncodeErrorCategory::MissingValue);
        };
        if matches!(value.view(), ValueRef::Null)
            && matches!(descriptor.view(), DescriptorRef::Optional(_))
        {
            return failure(EncodeErrorCategory::NullConformance);
        }
        if emitted {
            output.push(b',');
        }
        emitted = true;
        string(output, ValueRef::String(field.name()))?;
        output.push(b':');
        encode_value(output, value.view(), descriptor)?;
    }
    output.push(b'}');
    Ok(())
}

fn string(output: &mut Vec<u8>, value: ValueRef<'_>) -> Result<(), EncodeError> {
    let ValueRef::String(value) = value else {
        return mismatch();
    };
    text(output, value);
    Ok(())
}

fn text(output: &mut Vec<u8>, value: &str) {
    output.push(b'"');
    for character in value.chars() {
        match character {
            '"' => output.extend_from_slice(br#"\""#),
            '\\' => output.extend_from_slice(br#"\\"#),
            '\u{08}' => output.extend_from_slice(br#"\b"#),
            '\t' => output.extend_from_slice(br#"\t"#),
            '\n' => output.extend_from_slice(br#"\n"#),
            '\u{0c}' => output.extend_from_slice(br#"\f"#),
            '\r' => output.extend_from_slice(br#"\r"#),
            '\0'..='\u{1f}' => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                output.extend_from_slice(br#"\u00"#);
                output.push(HEX[(character as usize) >> 4]);
                output.push(HEX[(character as usize) & 15]);
            }
            _ => {
                let mut bytes = [0; 4];
                output.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
            }
        }
    }
    output.push(b'"');
}

fn signed(
    output: &mut Vec<u8>,
    value: ValueRef<'_>,
    min: i64,
    max: i64,
    quoted: bool,
) -> Result<(), EncodeError> {
    let ValueRef::I64(value) = value else {
        return mismatch();
    };
    if !(min..=max).contains(&value) {
        return failure(EncodeErrorCategory::IntegerRange);
    }
    token(output, &value.to_string(), quoted);
    Ok(())
}

fn unsigned(
    output: &mut Vec<u8>,
    value: ValueRef<'_>,
    max: u64,
    quoted: bool,
) -> Result<(), EncodeError> {
    let ValueRef::U64(value) = value else {
        return mismatch();
    };
    if value > max {
        return failure(EncodeErrorCategory::IntegerRange);
    }
    token(output, &value.to_string(), quoted);
    Ok(())
}

fn token(output: &mut Vec<u8>, token: &str, quoted: bool) {
    if quoted {
        output.push(b'"')
    }
    output.extend_from_slice(token.as_bytes());
    if quoted {
        output.push(b'"')
    }
}

fn float32(output: &mut Vec<u8>, value: ValueRef<'_>) -> Result<(), EncodeError> {
    let ValueRef::F32(value) = value else {
        return mismatch();
    };
    output.extend_from_slice(ryu::Buffer::new().format_finite(value).as_bytes());
    Ok(())
}

fn float64(output: &mut Vec<u8>, value: ValueRef<'_>) -> Result<(), EncodeError> {
    let ValueRef::F64(value) = value else {
        return mismatch();
    };
    output.extend_from_slice(ryu::Buffer::new().format_finite(value).as_bytes());
    Ok(())
}

fn blob(output: &mut Vec<u8>, value: ValueRef<'_>) -> Result<(), EncodeError> {
    let ValueRef::Bytes(value) = value else {
        return mismatch();
    };
    output.extend_from_slice(br#"{"base64":""#);
    output.extend_from_slice(STANDARD.encode(value).as_bytes());
    output.extend_from_slice(br#""}"#);
    Ok(())
}

fn mismatch<T>() -> Result<T, EncodeError> {
    failure(EncodeErrorCategory::RepresentationMismatch)
}
fn failure<T>(category: EncodeErrorCategory) -> Result<T, EncodeError> {
    Err(EncodeError(category))
}

#[cfg(test)]
mod tests {
    use super::EncodeErrorCategory as C;
    use super::*;
    use boxology_contract::{
        ContractValue as Value, DecodeRole, FieldDescriptor, OpaqueNumber, OpaquePayload,
        OpaqueTree, TypeDescriptor as D, VariantDescriptor, VariantPayload,
    };

    const PREFIX: &[u8] = br#"{"result":{"value":"#;

    fn encoded(slot: SlotValue, descriptor: &D) -> Vec<u8> {
        encode_result(&slot, descriptor).unwrap()
    }

    fn value(value: Value) -> SlotValue {
        SlotValue::Value(value)
    }

    fn scalar(descriptor: D, value: Value, token: &str) {
        canonical(SlotValue::Value(value), &descriptor, token);
    }

    fn canonical(slot: SlotValue, descriptor: &D, token: &str) {
        let first = encoded(slot, descriptor);
        assert_eq!(first, [PREFIX, token.as_bytes(), b"}}"].concat());
        let tree = crate::syntax::parse(
            &first[PREFIX.len()..first.len() - 2],
            crate::syntax::SyntaxLimits(first.len(), crate::syntax::DEFAULT_DEPTH_LIMIT),
        )
        .unwrap();
        let decoded =
            crate::semantic::decode_tree(tree, descriptor, DecodeRole::ConsumerOutput).unwrap();
        assert_eq!(encoded(decoded, descriptor), first);
    }

    fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        Value::object(entries.into_iter().map(|(key, value)| (key.into(), value))).unwrap()
    }

    fn field(name: &str, descriptor: D) -> FieldDescriptor {
        FieldDescriptor::new(name, descriptor, None)
    }

    fn variant(tag: &str, payload: VariantPayload) -> VariantDescriptor {
        VariantDescriptor::new(tag, payload, None)
    }

    fn enum_descriptor(variants: impl IntoIterator<Item = VariantDescriptor>) -> D {
        D::enumeration(variants).unwrap()
    }

    fn enum_slot(tag: &str, payload: SlotValue) -> SlotValue {
        value(Value::enum_value(tag, payload))
    }

    fn canonical_domain(tag: &str, payload: SlotValue, descriptor: &D, token: &str) {
        const PREFIX: &[u8] = br#"{"error":{"kind":"domain","value":"#;
        let first = encode_domain(tag, &payload, descriptor).unwrap();
        assert_eq!(first, [PREFIX, token.as_bytes(), b"}}"].concat());
        let tree = crate::syntax::parse(
            &first[PREFIX.len()..first.len() - 2],
            crate::syntax::SyntaxLimits(first.len(), crate::syntax::DEFAULT_DEPTH_LIMIT),
        )
        .unwrap();
        let decoded =
            crate::semantic::decode_tree(tree, descriptor, DecodeRole::ConsumerOutput).unwrap();
        let SlotValue::Value(value) = decoded else {
            panic!("domain enum must decode as a value")
        };
        let ValueRef::Enum { tag, payload } = value.view() else {
            panic!("domain descriptor must decode an enum")
        };
        assert_eq!(encode_domain(tag, payload, descriptor).unwrap(), first);
    }

    #[test]
    fn canonical_scalar_goldens_replay_byte_identically() {
        for (descriptor, value, token) in [
            (D::bool(), Value::bool(false), "false"),
            (D::bool(), Value::bool(true), "true"),
            (D::string(), Value::string(""), r#""""#),
            (D::string(), Value::string("plain"), r#""plain""#),
            (D::i8(), Value::i64(0), "0"),
            (D::i8(), Value::i64(i8::MIN.into()), "-128"),
            (D::i8(), Value::i64(i8::MAX.into()), "127"),
            (D::i16(), Value::i64(0), "0"),
            (D::i16(), Value::i64(i16::MIN.into()), "-32768"),
            (D::i16(), Value::i64(i16::MAX.into()), "32767"),
            (D::i32(), Value::i64(0), "0"),
            (D::i32(), Value::i64(i32::MIN.into()), "-2147483648"),
            (D::i32(), Value::i64(i32::MAX.into()), "2147483647"),
            (D::i64(), Value::i64(0), r#""0""#),
            (D::i64(), Value::i64(i64::MIN), r#""-9223372036854775808""#),
            (D::i64(), Value::i64(i64::MAX), r#""9223372036854775807""#),
            (D::u8(), Value::u64(0), "0"),
            (D::u8(), Value::u64(u8::MAX.into()), "255"),
            (D::u16(), Value::u64(0), "0"),
            (D::u16(), Value::u64(u16::MAX.into()), "65535"),
            (D::u32(), Value::u64(0), "0"),
            (D::u32(), Value::u64(u32::MAX.into()), "4294967295"),
            (D::u64(), Value::u64(0), r#""0""#),
            (D::u64(), Value::u64(u64::MAX), r#""18446744073709551615""#),
        ] {
            scalar(descriptor, value, token);
        }
    }

    #[test]
    fn strings_use_exact_d3_escaping() {
        let controls: String = (0..=0x1f).map(char::from).collect();
        scalar(
            D::string(),
            Value::string(format!("{controls}\"\\/café💡")),
            r#""\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\b\t\n\u000b\f\r\u000e\u000f\u0010\u0011\u0012\u0013\u0014\u0015\u0016\u0017\u0018\u0019\u001a\u001b\u001c\u001d\u001e\u001f\"\\/café💡""#,
        );
    }

    #[test]
    fn floats_use_descriptor_width_ryu_goldens() {
        let f32s = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (f32::MAX, "3.4028235e38"),
            (f32::MIN, "-3.4028235e38"),
            (f32::MIN_POSITIVE, "1.1754944e-38"),
            (f32::from_bits(1), "1e-45"),
            (0.1, "0.1"),
        ];
        for (number, token) in f32s {
            scalar(D::f32(), Value::f32(number).unwrap(), token);
        }
        let f64s = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (f64::MAX, "1.7976931348623157e308"),
            (f64::MIN, "-1.7976931348623157e308"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (f64::from_bits(1), "5e-324"),
        ];
        for (number, token) in f64s {
            scalar(D::f64(), Value::f64(number).unwrap(), token);
        }
    }

    #[test]
    fn blob_presence_and_hello_envelopes_are_exact() {
        for (bytes, token) in [
            (b"".as_slice(), r#"{"base64":""}"#),
            (b"a", r#"{"base64":"YQ=="}"#),
            (b"ab", r#"{"base64":"YWI="}"#),
            (b"abc", r#"{"base64":"YWJj"}"#),
            (&[0, 255, 16], r#"{"base64":"AP8Q"}"#),
        ] {
            scalar(D::blob(), Value::bytes(bytes), token);
        }
        let optional = D::optional(D::string()).unwrap();
        assert_eq!(
            encoded(SlotValue::Null, &optional),
            br#"{"result":{"value":null}}"#
        );
        assert_eq!(
            encoded(value(Value::string("Hello, Ada!")), &D::string()),
            br#"{"result":{"value":"Hello, Ada!"}}"#,
        );
    }

    fn category(slot: SlotValue, descriptor: D, expected: C) {
        let error = encode_result(&slot, &descriptor).unwrap_err();
        assert_eq!(error.category(), expected);
        assert!(!format!("{error:?}{error}").contains("DO_NOT_LEAK"));
        assert!(error.source().is_none());
    }

    #[test]
    fn failures_are_closed_and_category_only() {
        category(SlotValue::Missing, D::bool(), C::MissingValue);
        category(SlotValue::Null, D::bool(), C::NullConformance);
        for (slot, descriptor) in [
            (value(Value::string("DO_NOT_LEAK")), D::bool()),
            (value(Value::bool(true)), D::string()),
            (value(Value::u64(1)), D::i32()),
            (value(Value::i64(1)), D::u32()),
            (value(Value::f64(1.0).unwrap()), D::f32()),
            (value(Value::f32(1.0).unwrap()), D::f64()),
            (value(Value::string("DO_NOT_LEAK")), D::blob()),
            (
                value(Value::opaque(OpaquePayload::new(OpaqueTree::Null))),
                D::bool(),
            ),
            (value(Value::sensitive(Value::bool(true))), D::bool()),
        ] {
            category(slot, descriptor, C::RepresentationMismatch);
        }
        for (slot, descriptor) in [
            (value(Value::i64(i8::MIN as i64 - 1)), D::i8()),
            (value(Value::u64(u8::MAX as u64 + 1)), D::u8()),
        ] {
            category(slot, descriptor, C::IntegerRange);
        }
        let unsupported = |slot, descriptor| category(slot, descriptor, C::UnsupportedDescriptor);
        unsupported(value(Value::bool(true)), D::secret(D::bool()).unwrap());
    }

    #[test]
    fn aggregates_have_exact_canonical_bytes_and_replay() {
        canonical(value(Value::list([])), &D::list(D::bool()).unwrap(), "[]");
        canonical(value(object([])), &D::map(D::bool()).unwrap(), "{}");
        canonical(value(object([])), &D::structure([]).unwrap(), "{}");

        let optional = D::optional(D::string()).unwrap();
        canonical(
            value(Value::list([Value::null(), Value::string("x")])),
            &D::list(optional).unwrap(),
            r#"[null,"x"]"#,
        );
        canonical(
            value(object([("x", Value::null())])),
            &D::map(D::optional(D::bool()).unwrap()).unwrap(),
            r#"{"x":null}"#,
        );
        canonical(
            value(object([
                ("é", Value::bool(true)),
                ("aa", Value::bool(true)),
                ("a", Value::bool(true)),
                ("\"", Value::bool(true)),
                ("\n", Value::bool(true)),
                ("", Value::bool(true)),
            ])),
            &D::map(D::bool()).unwrap(),
            r#"{"":true,"\n":true,"\"":true,"a":true,"aa":true,"é":true}"#,
        );

        let ordered = D::structure([field("z", D::bool()), field("a", D::string())]).unwrap();
        canonical(
            value(object([
                ("a", Value::string("x")),
                ("z", Value::bool(true)),
            ])),
            &ordered,
            r#"{"z":true,"a":"x"}"#,
        );

        let nested = D::structure([field(
            "root",
            D::list(D::map(D::optional(D::string()).unwrap()).unwrap()).unwrap(),
        )])
        .unwrap();
        canonical(
            value(object([(
                "root",
                Value::list([object([("y", Value::string("yes")), ("x", Value::null())])]),
            )])),
            &nested,
            r#"{"root":[{"x":null,"y":"yes"}]}"#,
        );
    }

    #[test]
    fn top_level_and_struct_presence_are_exact() {
        let optional = D::optional(D::string()).unwrap();
        let tri = D::tri_state(D::string()).unwrap();
        for descriptor in [&optional, &tri] {
            canonical(SlotValue::Null, descriptor, "null");
            canonical(value(Value::string("x")), descriptor, r#""x""#);
            category(SlotValue::Missing, descriptor.clone(), C::MissingValue);
        }

        let structure = D::structure([
            field("required", D::bool()),
            field("optional", optional),
            field("tri", tri),
        ])
        .unwrap();
        canonical(
            value(object([("required", Value::bool(true))])),
            &structure,
            r#"{"required":true}"#,
        );
        canonical(
            value(object([
                ("required", Value::bool(true)),
                ("optional", Value::string("o")),
            ])),
            &structure,
            r#"{"required":true,"optional":"o"}"#,
        );
        canonical(
            value(object([
                ("tri", Value::null()),
                ("optional", Value::string("o")),
                ("required", Value::bool(true)),
            ])),
            &structure,
            r#"{"required":true,"optional":"o","tri":null}"#,
        );
        canonical(
            value(object([
                ("required", Value::bool(true)),
                ("tri", Value::string("t")),
            ])),
            &structure,
            r#"{"required":true,"tri":"t"}"#,
        );
        category(
            value(object([
                ("required", Value::bool(true)),
                ("optional", Value::null()),
            ])),
            structure.clone(),
            C::NullConformance,
        );
        category(value(object([])), structure.clone(), C::MissingValue);
        category(
            value(object([("required", Value::null())])),
            structure,
            C::NullConformance,
        );
    }

    #[test]
    fn aggregate_failures_follow_declared_precedence() {
        let list = D::list(D::i8()).unwrap();
        let map = D::map(D::i8()).unwrap();
        let structure = D::structure([field("a", D::i8()), field("b", D::i8())]).unwrap();
        for descriptor in [list.clone(), map.clone(), structure.clone()] {
            category(SlotValue::Missing, descriptor.clone(), C::MissingValue);
            category(SlotValue::Null, descriptor.clone(), C::NullConformance);
            category(
                value(Value::string("DO_NOT_LEAK")),
                descriptor,
                C::RepresentationMismatch,
            );
        }
        category(
            value(object([
                ("a", Value::i64(1)),
                ("unknown", Value::bool(true)),
            ])),
            structure.clone(),
            C::RepresentationMismatch,
        );
        category(
            value(Value::list([Value::i64(128), Value::bool(true)])),
            list,
            C::IntegerRange,
        );
        category(
            value(object([("b", Value::bool(true)), ("a", Value::i64(128))])),
            map,
            C::IntegerRange,
        );
        category(
            value(object([("b", Value::bool(true)), ("a", Value::i64(128))])),
            structure,
            C::IntegerRange,
        );

        let secret = D::secret(D::bool()).unwrap();
        for descriptor in [
            D::list(secret.clone()).unwrap(),
            D::map(secret.clone()).unwrap(),
            D::structure([field("x", secret)]).unwrap(),
        ] {
            for slot in [
                SlotValue::Missing,
                SlotValue::Null,
                value(Value::bool(true)),
            ] {
                category(slot, descriptor.clone(), C::UnsupportedDescriptor);
            }
        }
    }

    #[test]
    fn known_enums_and_domain_errors_have_exact_recursive_bytes() {
        let record = D::structure([field("z", D::bool()), field("a", D::string())]).unwrap();
        let inner = enum_descriptor([
            variant("idle", VariantPayload::Unit),
            variant("text", VariantPayload::Value(D::string())),
        ]);
        let descriptor = enum_descriptor([
            variant("unit", VariantPayload::Unit),
            variant("scalar", VariantPayload::Value(D::i8())),
            variant(
                "optional",
                VariantPayload::Value(D::optional(D::string()).unwrap()),
            ),
            variant("record", VariantPayload::Value(record)),
            variant("nested", VariantPayload::Value(inner)),
            variant("tag\n\"", VariantPayload::Unit),
        ]);
        for (slot, token) in [
            (
                enum_slot("unit", SlotValue::Null),
                r#"{"tag":"unit","payload":null}"#,
            ),
            (
                enum_slot("scalar", value(Value::i64(-7))),
                r#"{"tag":"scalar","payload":-7}"#,
            ),
            (
                enum_slot("optional", SlotValue::Null),
                r#"{"tag":"optional","payload":null}"#,
            ),
            (
                enum_slot(
                    "record",
                    value(object([
                        ("a", Value::string("x")),
                        ("z", Value::bool(true)),
                    ])),
                ),
                r#"{"tag":"record","payload":{"z":true,"a":"x"}}"#,
            ),
            (
                enum_slot("nested", enum_slot("text", value(Value::string("inside")))),
                r#"{"tag":"nested","payload":{"tag":"text","payload":"inside"}}"#,
            ),
            (
                enum_slot("tag\n\"", SlotValue::Null),
                r#"{"tag":"tag\n\"","payload":null}"#,
            ),
        ] {
            canonical(slot, &descriptor, token);
        }
        canonical_domain(
            "unit",
            SlotValue::Null,
            &descriptor,
            r#"{"tag":"unit","payload":null}"#,
        );
        canonical_domain(
            "record",
            value(object([
                ("a", Value::string("why")),
                ("z", Value::bool(false)),
            ])),
            &descriptor,
            r#"{"tag":"record","payload":{"z":false,"a":"why"}}"#,
        );
    }

    fn opaque_payload() -> SlotValue {
        value(Value::opaque(OpaquePayload::new(OpaqueTree::Object(vec![
            (
                "same".into(),
                OpaqueTree::Number(OpaqueNumber::new("1e0").unwrap()),
            ),
            (
                "same".into(),
                OpaqueTree::Number(OpaqueNumber::new("1.0").unwrap()),
            ),
            (
                "k\n".into(),
                OpaqueTree::List(vec![
                    OpaqueTree::String("v\t".into()),
                    OpaqueTree::Bool(true),
                    OpaqueTree::Null,
                ]),
            ),
        ]))))
    }

    #[test]
    fn unknown_enums_forward_only_opaque_payloads_without_canonicalizing_them() {
        let descriptor = enum_descriptor([variant("known", VariantPayload::Unit)]);
        let token = r#"{"tag":"future","payload":{"same":1e0,"same":1.0,"k\n":["v\t",true,null]}}"#;
        canonical(enum_slot("future", opaque_payload()), &descriptor, token);
        canonical_domain("future", opaque_payload(), &descriptor, token);
        canonical(
            enum_slot("future", opaque_payload()),
            &enum_descriptor([]),
            token,
        );
    }

    fn domain_category(tag: &str, payload: SlotValue, descriptor: D, expected: C) {
        let error = encode_domain(tag, &payload, &descriptor).unwrap_err();
        assert_eq!(error.category(), expected);
        let rendered = format!("{error:?}{error}");
        for secret in ["DO_NOT_LEAK_TAG", "DO_NOT_LEAK_VALUE", "DO_NOT_LEAK_KEY"] {
            assert!(!rendered.contains(secret));
        }
    }

    #[test]
    fn enum_failures_enforce_presence_opacity_support_and_redaction_precedence() {
        let descriptor = enum_descriptor([
            variant("unit", VariantPayload::Unit),
            variant("value", VariantPayload::Value(D::bool())),
        ]);
        for (tag, payload, expected) in [
            ("unit", SlotValue::Missing, C::MissingValue),
            ("unit", value(Value::bool(true)), C::RepresentationMismatch),
            ("value", SlotValue::Missing, C::MissingValue),
            ("value", SlotValue::Null, C::NullConformance),
            ("future", SlotValue::Missing, C::RepresentationMismatch),
            ("future", SlotValue::Null, C::RepresentationMismatch),
            (
                "DO_NOT_LEAK_TAG",
                value(Value::string("DO_NOT_LEAK_VALUE")),
                C::RepresentationMismatch,
            ),
        ] {
            category(
                enum_slot(tag, payload.clone()),
                descriptor.clone(),
                expected,
            );
            domain_category(tag, payload, descriptor.clone(), expected);
        }
        category(
            value(Value::bool(true)),
            descriptor.clone(),
            C::RepresentationMismatch,
        );
        domain_category(
            "DO_NOT_LEAK_TAG",
            value(Value::string("DO_NOT_LEAK_KEY")),
            enum_descriptor([]),
            C::RepresentationMismatch,
        );
        category(
            enum_slot("DO_NOT_LEAK_TAG", value(Value::string("DO_NOT_LEAK_VALUE"))),
            enum_descriptor([]),
            C::RepresentationMismatch,
        );
        domain_category(
            "unit",
            SlotValue::Null,
            D::bool(),
            C::RepresentationMismatch,
        );

        let unsupported = enum_descriptor([variant(
            "bad",
            VariantPayload::Value(
                D::structure([field("DO_NOT_LEAK_KEY", D::secret(D::bool()).unwrap())]).unwrap(),
            ),
        )]);
        for payload in [
            SlotValue::Missing,
            SlotValue::Null,
            value(Value::string("DO_NOT_LEAK_VALUE")),
        ] {
            category(
                enum_slot("DO_NOT_LEAK_TAG", payload.clone()),
                unsupported.clone(),
                C::UnsupportedDescriptor,
            );
            domain_category(
                "DO_NOT_LEAK_TAG",
                payload,
                unsupported.clone(),
                C::UnsupportedDescriptor,
            );
        }
    }
}
