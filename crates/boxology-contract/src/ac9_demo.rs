//! S1 AC9 structural demonstration only.
//!
//! Mechanical generated-surface enforcement belongs to S2/S5, and real
//! generated fixtures remain owned by #100.

use crate::conform::{ConformanceErrorKind, Shape, VariantShape, conform_slot};
use crate::{
    ContractError, ContractType, ContractValue, DecodeError, DecodeErrorKind, DecodeRole,
    EncodeError, Field, ObjectRef, OpaquePayload, OpaqueTree, PathSegment, SlotValue, ValueRef,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DemoStruct {
    id: u64,
    label: Option<String>,
    note: Field<String>,
}

fn push_field<T: ContractType>(
    entries: &mut Vec<(String, ContractValue)>,
    name: &str,
    value: &T,
) -> Result<(), EncodeError> {
    if let Some(value) = value
        .encode_field()
        .map_err(|error| error.under(PathSegment::Field(name.into())))?
    {
        entries.push((name.into(), value));
    }
    Ok(())
}

fn decode_field<T: ContractType>(object: ObjectRef<'_>, name: &str) -> Result<T, DecodeError> {
    T::decode_field(object.get(name)).map_err(|error| error.under(PathSegment::Field(name.into())))
}

impl ContractType for DemoStruct {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        let mut entries = Vec::new();
        push_field(&mut entries, "id", &self.id)?;
        push_field(&mut entries, "label", &self.label)?;
        push_field(&mut entries, "note", &self.note)?;
        ContractValue::object(entries).map_err(|_| unreachable!())
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        let ValueRef::Object(object) = value.view() else {
            return if matches!(value.view(), ValueRef::Null) {
                Err(DecodeError::new(DecodeErrorKind::UnexpectedNull))
            } else {
                Err(DecodeError::new(DecodeErrorKind::KindMismatch))
            };
        };
        for (name, _) in object.entries() {
            if !matches!(name, "id" | "label" | "note") {
                return Err(DecodeError::new(DecodeErrorKind::UnknownField(name.into()))
                    .under(PathSegment::Field(name.into())));
            }
        }
        Ok(Self {
            id: decode_field(object, "id")?,
            label: decode_field(object, "label")?,
            note: decode_field(object, "note")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum DemoEnum {
    Known(String),
    Unknown { tag: String, payload: OpaquePayload },
}

fn unknown_variant(tag: &str) -> DecodeError {
    DecodeError::new(DecodeErrorKind::UnknownVariant(tag.into()))
        .under(PathSegment::Variant(tag.into()))
}

impl ContractType for DemoEnum {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        let (tag, payload) = match self {
            Self::Known(value) => (
                "known".into(),
                value
                    .encode()
                    .map_err(|error| error.under(PathSegment::Variant("known".into())))?,
            ),
            Self::Unknown { tag, payload } => (
                tag.clone(),
                SlotValue::Value(ContractValue::opaque(payload.forward())),
            ),
        };
        Ok(ContractValue::enum_value(tag, payload))
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        let ValueRef::Enum { tag, payload } = value.view() else {
            return if matches!(value.view(), ValueRef::Null) {
                Err(DecodeError::new(DecodeErrorKind::UnexpectedNull))
            } else {
                Err(DecodeError::new(DecodeErrorKind::KindMismatch))
            };
        };
        if tag == "known" {
            return String::decode(payload)
                .map(Self::Known)
                .map_err(|error| error.under(PathSegment::Variant(tag.into())));
        }
        match payload {
            SlotValue::Value(value) => match value.view() {
                ValueRef::Opaque(payload) => Ok(Self::Unknown {
                    tag: tag.into(),
                    payload: payload.forward(),
                }),
                _ => Err(unknown_variant(tag)),
            },
            _ => Err(unknown_variant(tag)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum DemoError {
    Known(String),
    Unknown { tag: String, payload: OpaquePayload },
}

impl ContractType for DemoError {
    fn encode_value(&self) -> Result<ContractValue, EncodeError> {
        match self {
            Self::Known(value) => DemoEnum::Known(value.clone()).encode_value(),
            Self::Unknown { tag, payload } => DemoEnum::Unknown {
                tag: tag.clone(),
                payload: payload.forward(),
            }
            .encode_value(),
        }
    }

    fn decode_value(value: &ContractValue) -> Result<Self, DecodeError> {
        DemoEnum::decode_value(value).map(|value| match value {
            DemoEnum::Known(value) => Self::Known(value),
            DemoEnum::Unknown { tag, payload } => Self::Unknown { tag, payload },
        })
    }
}

impl ContractError for DemoError {
    fn error_tag(&self) -> &str {
        match self {
            Self::Known(_) => "known",
            Self::Unknown { tag, .. } => tag,
        }
    }
}

fn struct_shape() -> Shape {
    Shape::structure([
        ("id".into(), Shape::u64()),
        ("label".into(), Shape::optional(Shape::string()).unwrap()),
        ("note".into(), Shape::tri_state(Shape::string()).unwrap()),
    ])
    .unwrap()
}

fn enum_shape() -> Shape {
    Shape::enumeration([("known".into(), VariantShape::Value(Shape::string()))]).unwrap()
}

fn object(entries: impl IntoIterator<Item = (&'static str, ContractValue)>) -> ContractValue {
    ContractValue::object(entries.into_iter().map(|(key, value)| (key.into(), value))).unwrap()
}

#[test]
fn struct_tolerance_drops_unknowns_before_strict_typed_decode() {
    const SENTINEL: &str = "unknown-struct-payload";
    let input = SlotValue::Value(object([
        ("id", ContractValue::u64(7)),
        ("label", ContractValue::string("visible")),
        ("note", ContractValue::null()),
        ("extra", ContractValue::string(SENTINEL)),
    ]));
    let direct = DemoStruct::decode(&input).unwrap_err();
    assert_eq!(
        direct.kind(),
        &DecodeErrorKind::UnknownField("extra".into())
    );
    assert_eq!(direct.path(), &[PathSegment::Field("extra".into())]);

    let provider =
        conform_slot(&struct_shape(), DecodeRole::ProviderInput, input.clone()).unwrap_err();
    assert_eq!(
        provider.kind,
        ConformanceErrorKind::UnknownField("extra".into())
    );
    assert_eq!(provider.path, vec![PathSegment::Field("extra".into())]);
    assert!(!format!("{provider:?} {provider}").contains(SENTINEL));
    let normalized = conform_slot(&struct_shape(), DecodeRole::ConsumerOutput, input).unwrap();
    let typed = DemoStruct::decode(&normalized).unwrap();
    assert_eq!(
        typed,
        DemoStruct {
            id: 7,
            label: Some("visible".into()),
            note: Field::Null,
        }
    );
    assert_eq!(typed.encode().unwrap(), normalized);
    let SlotValue::Value(value) = normalized else {
        panic!()
    };
    let ValueRef::Object(normalized_object) = value.view() else {
        panic!()
    };
    assert!(normalized_object.get("extra").is_none());
    let absent = DemoStruct {
        id: 8,
        label: None,
        note: Field::Missing,
    };
    assert_eq!(DemoStruct::decode(&absent.encode().unwrap()), Ok(absent));

    let missing =
        DemoStruct::decode_value(&object([("label", ContractValue::string("x"))])).unwrap_err();
    assert_eq!(missing.kind(), &DecodeErrorKind::MissingRequired);
    assert_eq!(missing.path(), &[PathSegment::Field("id".into())]);
    let nested = DemoStruct::decode_value(&object([
        ("id", ContractValue::string(SENTINEL)),
        ("label", ContractValue::string("x")),
    ]))
    .unwrap_err();
    assert_eq!(nested.kind(), &DecodeErrorKind::KindMismatch);
    assert_eq!(nested.path(), &[PathSegment::Field("id".into())]);
    assert!(!format!("{nested:?} {nested}").contains(SENTINEL));
}

#[test]
fn enum_tolerance_preserves_unknowns_for_strict_typed_decode() {
    let known = DemoEnum::Known("payload".into());
    assert_eq!(DemoEnum::decode(&known.encode().unwrap()), Ok(known));
    for (payload, kind) in [
        (SlotValue::Missing, DecodeErrorKind::MissingRequired),
        (SlotValue::Null, DecodeErrorKind::UnexpectedNull),
    ] {
        let error =
            DemoEnum::decode_value(&ContractValue::enum_value("known", payload)).unwrap_err();
        assert_eq!(error.kind(), &kind);
        assert_eq!(error.path(), &[PathSegment::Variant("known".into())]);
    }

    const SENTINEL: &str = "distinctive-unknown-enum-payload";
    let raw = SlotValue::Value(ContractValue::list([object([
        ("marker", ContractValue::string(SENTINEL)),
        ("bytes", ContractValue::bytes([0xfb])),
    ])]));
    let input = SlotValue::Value(ContractValue::enum_value("future", raw));
    let direct = DemoEnum::decode(&input).unwrap_err();
    assert_eq!(
        direct.kind(),
        &DecodeErrorKind::UnknownVariant("future".into())
    );

    let provider =
        conform_slot(&enum_shape(), DecodeRole::ProviderInput, input.clone()).unwrap_err();
    assert_eq!(
        provider.kind,
        ConformanceErrorKind::UnknownVariant("future".into())
    );
    assert_eq!(provider.path, vec![PathSegment::Variant("future".into())]);
    assert!(!format!("{provider:?} {provider}").contains(SENTINEL));
    let normalized = conform_slot(&enum_shape(), DecodeRole::ConsumerOutput, input).unwrap();
    let unknown = DemoEnum::decode(&normalized).unwrap();
    let DemoEnum::Unknown { tag, payload } = &unknown else {
        panic!()
    };
    assert_eq!(tag, "future");
    let expected = OpaqueTree::List(vec![OpaqueTree::Object(vec![
        ("marker".into(), OpaqueTree::String(SENTINEL.into())),
        (
            "bytes".into(),
            OpaqueTree::Object(vec![("base64".into(), OpaqueTree::String("+w==".into()))]),
        ),
    ])]);
    assert_eq!(payload.reveal(), &expected);
    assert_eq!(payload.forward().reveal(), &expected);
    assert_eq!(format!("{payload:?}"), "OpaquePayload(<redacted>)");
    assert!(!format!("{payload:?} {unknown:?}").contains(SENTINEL));
    let reencoded = unknown.encode().unwrap();
    assert_eq!(reencoded, normalized);
    assert_eq!(unknown.encode().unwrap(), reencoded);
    assert_eq!(
        conform_slot(&enum_shape(), DecodeRole::ConsumerOutput, reencoded.clone()).unwrap(),
        reencoded
    );

    let error = DemoError::decode(&reencoded).unwrap();
    assert_eq!(error.error_tag(), "future");
    assert_eq!(DemoError::Known("x".into()).error_tag(), "known");
    let DemoError::Unknown { payload, .. } = &error else {
        panic!()
    };
    assert_eq!(payload.forward().reveal(), &expected);
    assert_eq!(format!("{payload:?}"), "OpaquePayload(<redacted>)");
    assert!(!format!("{payload:?} {error:?}").contains(SENTINEL));
    assert_eq!(error.encode().unwrap(), reencoded);
}
