use std::{error::Error, fmt};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use boxology_contract::{DescriptorRef, SlotValue, TypeDescriptor, ValueRef};

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
        SlotValue::Null if matches!(descriptor.view(), DescriptorRef::Optional(_)) => {
            output.extend_from_slice(b"null")
        }
        SlotValue::Null => return failure(EncodeErrorCategory::NullConformance),
        SlotValue::Value(value) => encode_value(&mut output, value.view(), descriptor)?,
    }
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
        DescriptorRef::Optional(inner) => is_supported(inner),
        _ => false,
    }
}

fn encode_value(
    output: &mut Vec<u8>,
    value: ValueRef<'_>,
    descriptor: &TypeDescriptor,
) -> Result<(), EncodeError> {
    let descriptor = match descriptor.view() {
        DescriptorRef::Optional(inner) => inner,
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

fn string(output: &mut Vec<u8>, value: ValueRef<'_>) -> Result<(), EncodeError> {
    let ValueRef::String(value) = value else {
        return mismatch();
    };
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
    Ok(())
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
        ContractValue as Value, DecodeRole, FieldDescriptor, OpaquePayload, OpaqueTree,
        TypeDescriptor as D, VariantDescriptor, VariantPayload,
    };

    const PREFIX: &[u8] = br#"{"result":{"value":"#;

    fn encoded(slot: SlotValue, descriptor: &D) -> Vec<u8> {
        encode_result(&slot, descriptor).unwrap()
    }

    fn value(value: Value) -> SlotValue {
        SlotValue::Value(value)
    }

    fn scalar(descriptor: D, value: Value, token: &str) {
        let first = encoded(SlotValue::Value(value), &descriptor);
        assert_eq!(first, [PREFIX, token.as_bytes(), b"}}"].concat());
        let tree = crate::syntax::parse(
            &first[PREFIX.len()..first.len() - 2],
            crate::syntax::SyntaxLimits(first.len(), crate::syntax::DEFAULT_DEPTH_LIMIT),
        )
        .unwrap();
        let decoded =
            crate::semantic::decode_tree(tree, &descriptor, DecodeRole::ConsumerOutput).unwrap();
        assert_eq!(encoded(decoded, &descriptor), first);
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
        let field = FieldDescriptor::new("x", D::bool(), None);
        let variant = VariantDescriptor::new("x", VariantPayload::Unit, None);
        let unsupported = |slot, descriptor| category(slot, descriptor, C::UnsupportedDescriptor);
        for descriptor in [
            D::list(D::bool()).unwrap(),
            D::map(D::bool()).unwrap(),
            D::structure([field]).unwrap(),
            D::enumeration([variant]).unwrap(),
            D::secret(D::bool()).unwrap(),
        ] {
            unsupported(value(Value::bool(true)), descriptor);
        }
        let tri = D::tri_state(D::bool()).unwrap();
        unsupported(SlotValue::Null, tri);
        unsupported(SlotValue::Missing, D::list(D::bool()).unwrap());
        let optional_list = D::optional(D::list(D::bool()).unwrap()).unwrap();
        unsupported(SlotValue::Null, optional_list);
    }
}
