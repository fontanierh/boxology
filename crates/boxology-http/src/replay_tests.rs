use std::error::Error;

use boxology_contract::{
    ContractValue as Value, DecodeRole, FieldDescriptor, OpaqueTree, SlotValue, TypeDescriptor,
    ValueRef, VariantDescriptor, VariantPayload,
};

use crate::{
    semantic::{SemanticError, SemanticErrorCategory as C, decode_tree},
    syntax::{DEFAULT_DEPTH_LIMIT, SyntaxLimits, parse},
};

const ROLES: [DecodeRole; 2] = [DecodeRole::ProviderInput, DecodeRole::ConsumerOutput];
const SENTINEL: &str = "DO_NOT_LEAK";

fn decode(
    raw: &[u8],
    descriptor: &TypeDescriptor,
    role: DecodeRole,
) -> Result<SlotValue, SemanticError> {
    let tree =
        parse(raw, SyntaxLimits(raw.len(), DEFAULT_DEPTH_LIMIT)).expect("valid JSON fixture");
    decode_tree(tree, descriptor, role)
}

fn slot_both(raw: &[u8], descriptor: &TypeDescriptor, expected: SlotValue) {
    for role in ROLES {
        assert_eq!(decode(raw, descriptor, role), Ok(expected.clone()));
    }
}

fn value_both(raw: &[u8], descriptor: &TypeDescriptor, expected: Value) {
    slot_both(raw, descriptor, SlotValue::Value(expected));
}

fn error_role(
    raw: &[u8],
    descriptor: &TypeDescriptor,
    role: DecodeRole,
    category: C,
    forbidden: Option<&str>,
) {
    let error = decode(raw, descriptor, role).unwrap_err();
    assert_eq!(error.category(), category);
    if let Some(forbidden) = forbidden {
        assert!(!format!("{error:?}").contains(forbidden));
        assert!(!error.to_string().contains(forbidden));
    }
    assert!(error.source().is_none());
}

fn error_both(raw: &[u8], descriptor: &TypeDescriptor, category: C) {
    for role in ROLES {
        error_role(raw, descriptor, role, category, Some(SENTINEL));
    }
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

fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::object(entries.into_iter().map(|(key, value)| (key.into(), value))).unwrap()
}

#[test]
fn scalar_bytes_replay_every_width_boundary_and_string_form() {
    for (raw, descriptor, expected) in [
        (
            b"true".as_slice(),
            TypeDescriptor::bool(),
            Value::bool(true),
        ),
        (b"false", TypeDescriptor::bool(), Value::bool(false)),
        (
            br#""caf\u00e9 \"x\"""#,
            TypeDescriptor::string(),
            Value::string("café \"x\""),
        ),
        (
            "\"café\"".as_bytes(),
            TypeDescriptor::string(),
            Value::string("café"),
        ),
        (b"-128", TypeDescriptor::i8(), Value::i64(i8::MIN.into())),
        (b"127", TypeDescriptor::i8(), Value::i64(i8::MAX.into())),
        (
            b"-32768",
            TypeDescriptor::i16(),
            Value::i64(i16::MIN.into()),
        ),
        (b"32767", TypeDescriptor::i16(), Value::i64(i16::MAX.into())),
        (
            b"-2147483648",
            TypeDescriptor::i32(),
            Value::i64(i32::MIN.into()),
        ),
        (
            b"2147483647",
            TypeDescriptor::i32(),
            Value::i64(i32::MAX.into()),
        ),
        (b"0", TypeDescriptor::u8(), Value::u64(0)),
        (b"255", TypeDescriptor::u8(), Value::u64(u8::MAX.into())),
        (b"0", TypeDescriptor::u16(), Value::u64(0)),
        (b"65535", TypeDescriptor::u16(), Value::u64(u16::MAX.into())),
        (b"0", TypeDescriptor::u32(), Value::u64(0)),
        (
            b"4294967295",
            TypeDescriptor::u32(),
            Value::u64(u32::MAX.into()),
        ),
        (
            br#""-9223372036854775808""#,
            TypeDescriptor::i64(),
            Value::i64(i64::MIN),
        ),
        (
            br#""9223372036854775807""#,
            TypeDescriptor::i64(),
            Value::i64(i64::MAX),
        ),
        (br#""0""#, TypeDescriptor::u64(), Value::u64(0)),
        (
            br#""18446744073709551615""#,
            TypeDescriptor::u64(),
            Value::u64(u64::MAX),
        ),
    ] {
        value_both(raw, &descriptor, expected);
    }

    for (raw, descriptor, expected) in [
        (
            b"3.4028235e38".as_slice(),
            TypeDescriptor::f32(),
            f32::MAX as f64,
        ),
        (b"-3.4028235e38", TypeDescriptor::f32(), (-f32::MAX) as f64),
        (b"0.5", TypeDescriptor::f32(), 0.5),
        (b"1e2", TypeDescriptor::f32(), 100.0),
        (b"1.7976931348623157e308", TypeDescriptor::f64(), f64::MAX),
        (b"-1.7976931348623157e308", TypeDescriptor::f64(), -f64::MAX),
        (b"0.25", TypeDescriptor::f64(), 0.25),
        (b"1e-3", TypeDescriptor::f64(), 0.001),
    ] {
        for role in ROLES {
            let SlotValue::Value(value) = decode(raw, &descriptor, role).unwrap() else {
                panic!("float decoded as null");
            };
            match value.view() {
                ValueRef::F32(value) => assert_eq!(value.to_bits(), (expected as f32).to_bits()),
                ValueRef::F64(value) => assert_eq!(value.to_bits(), expected.to_bits()),
                _ => panic!("float decoded at wrong width"),
            }
        }
    }
    for descriptor in [TypeDescriptor::f32(), TypeDescriptor::f64()] {
        for raw in [b"0".as_slice(), b"-0"] {
            for role in ROLES {
                let SlotValue::Value(value) = decode(raw, &descriptor, role).unwrap() else {
                    panic!("zero decoded as null");
                };
                let negative = raw == b"-0";
                let expected32 = if negative { -0.0f32 } else { 0.0f32 };
                let expected64 = if negative { -0.0f64 } else { 0.0f64 };
                match value.view() {
                    ValueRef::F32(value) => assert_eq!(value.to_bits(), expected32.to_bits()),
                    ValueRef::F64(value) => assert_eq!(value.to_bits(), expected64.to_bits()),
                    _ => panic!("zero decoded at wrong width"),
                }
            }
        }
    }
}

#[test]
fn presence_bytes_replay_top_fields_children_and_nested_wrappers() {
    value_both(b"true", &TypeDescriptor::bool(), Value::bool(true));
    error_both(b"null", &TypeDescriptor::bool(), C::NullConformance);
    for wrapper in [
        TypeDescriptor::optional(TypeDescriptor::string()).unwrap(),
        TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap(),
    ] {
        slot_both(b"null", &wrapper, SlotValue::Null);
        value_both(br#""value""#, &wrapper, Value::string("value"));
    }

    let descriptor = structure([
        field("required", TypeDescriptor::bool()),
        field(
            "optional",
            TypeDescriptor::optional(TypeDescriptor::string()).unwrap(),
        ),
        field(
            "tri",
            TypeDescriptor::tri_state(TypeDescriptor::string()).unwrap(),
        ),
    ]);
    error_both(b"{}", &descriptor, C::RepresentationMismatch);
    error_both(br#"{"required":null}"#, &descriptor, C::NullConformance);
    error_both(
        br#"{"required":true,"optional":null}"#,
        &descriptor,
        C::NullConformance,
    );
    value_both(
        br#"{"required":true}"#,
        &descriptor,
        object([("required", Value::bool(true))]),
    );
    value_both(
        br#"{"required":true,"optional":"o","tri":null}"#,
        &descriptor,
        object([
            ("required", Value::bool(true)),
            ("optional", Value::string("o")),
            ("tri", Value::null()),
        ]),
    );
    value_both(
        br#"{"required":true,"tri":"t"}"#,
        &descriptor,
        object([("required", Value::bool(true)), ("tri", Value::string("t"))]),
    );

    for descriptor in [
        TypeDescriptor::list(TypeDescriptor::bool()).unwrap(),
        TypeDescriptor::map(TypeDescriptor::bool()).unwrap(),
    ] {
        let raw = if matches!(descriptor.view(), boxology_contract::DescriptorRef::List(_)) {
            b"[null]".as_slice()
        } else {
            br#"{"x":null}"#
        };
        error_both(raw, &descriptor, C::NullConformance);
    }
    let optional = TypeDescriptor::optional(TypeDescriptor::string()).unwrap();
    value_both(
        b"[null]",
        &TypeDescriptor::list(optional.clone()).unwrap(),
        Value::list([Value::null()]),
    );
    let nested = TypeDescriptor::list(TypeDescriptor::map(optional).unwrap()).unwrap();
    value_both(
        br#"[{"a":null,"b":"x"}]"#,
        &nested,
        Value::list([object([("a", Value::null()), ("b", Value::string("x"))])]),
    );
}

#[test]
fn aggregate_bytes_preserve_order_roles_and_unknown_enum_opacity() {
    let event = enumeration([
        variant("Unit", VariantPayload::Unit),
        variant(
            "Data",
            VariantPayload::Value(structure([
                field("name", TypeDescriptor::string()),
                field("count", TypeDescriptor::u8()),
            ])),
        ),
    ]);
    let aggregate = structure([field(
        "items",
        TypeDescriptor::list(TypeDescriptor::map(event.clone()).unwrap()).unwrap(),
    )]);
    let raw = br#"{"items":[{"z":{"tag":"Unit","payload":null},"\u0061":{"tag":"Data","payload":{"count":2,"unknown":{"DO_NOT_LEAK":1,"DO_NOT_LEAK":2},"name":"Ada"}}},{}]}"#;
    error_role(
        raw,
        &aggregate,
        DecodeRole::ProviderInput,
        C::RepresentationMismatch,
        Some(SENTINEL),
    );
    assert_eq!(
        decode(raw, &aggregate, DecodeRole::ConsumerOutput),
        Ok(SlotValue::Value(object([(
            "items",
            Value::list([
                object([
                    ("z", Value::enum_value("Unit", SlotValue::Null)),
                    (
                        "a",
                        Value::enum_value(
                            "Data",
                            SlotValue::Value(object([
                                ("count", Value::u64(2)),
                                ("name", Value::string("Ada")),
                            ])),
                        ),
                    ),
                ]),
                object([])
            ]),
        )])))
    );
    value_both(
        br#"{"payload":{"name":"Ada","count":2},"tag":"Data"}"#,
        &event,
        Value::enum_value(
            "Data",
            SlotValue::Value(object([
                ("name", Value::string("Ada")),
                ("count", Value::u64(2)),
            ])),
        ),
    );
    value_both(
        br#"{"tag":"Unit","payload":null}"#,
        &event,
        Value::enum_value("Unit", SlotValue::Null),
    );

    for raw in [
        br#"{"tag":"Future","payload":{"b":1,"a":1.0,"a":1e0,"nested":[null,{"x":2,"x":3}],"secret":"DO_NOT_LEAK"}}"#
            .as_slice(),
        br#"{"tag":"FutureNull","payload":null}"#,
    ] {
        let OpaqueTree::Object(entries) =
            parse(raw, SyntaxLimits(raw.len(), DEFAULT_DEPTH_LIMIT)).unwrap()
        else {
            panic!("unknown fixture is not an object");
        };
        let expected = &entries.iter().find(|(key, _)| key == "payload").unwrap().1;
        error_role(
            raw,
            &event,
            DecodeRole::ProviderInput,
            C::RepresentationMismatch,
            Some("Future"),
        );
        let SlotValue::Value(value) = decode(raw, &event, DecodeRole::ConsumerOutput).unwrap()
        else {
            panic!("unknown enum decoded as null");
        };
        let ValueRef::Enum { payload, .. } = value.view() else {
            panic!("unknown enum decoded at wrong shape");
        };
        let SlotValue::Value(payload) = payload else {
            panic!("unknown raw null collapsed to null slot");
        };
        let ValueRef::Opaque(payload) = payload.view() else {
            panic!("unknown payload was not opaque");
        };
        assert_eq!(payload.reveal(), expected);
        assert!(!format!("{payload:?}").contains(SENTINEL));
    }
}
