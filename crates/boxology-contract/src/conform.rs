use std::fmt;

use crate::{
    ContractValue, DecodeRole, ObjectRef, OpaquePayload, PathSegment, SlotValue, ValueRef,
};

#[derive(Clone)]
pub(crate) struct Shape(Repr);

#[derive(Clone)]
enum Repr {
    Bool,
    I64,
    U64,
    F32,
    F64,
    String,
    Bytes,
    List(Box<Shape>),
    Map(Box<Shape>),
    Struct(Vec<(String, Shape)>),
    Enum(Vec<(String, VariantShape)>),
    Optional(Box<Shape>),
    TriState(Box<Shape>),
}

#[derive(Clone)]
pub(crate) enum VariantShape {
    Unit,
    Value(Shape),
}

macro_rules! primitives {
    ($($name:ident => $variant:ident),+ $(,)?) => {$(
        pub(crate) fn $name() -> Self { Self(Repr::$variant) }
    )+};
}

impl Shape {
    primitives!(
        bool => Bool, i64 => I64, u64 => U64, f32 => F32,
        f64 => F64, string => String, bytes => Bytes,
    );

    pub(crate) fn list(element: Shape) -> Result<Self, ShapeError> {
        if matches!(element.0, Repr::TriState(_)) {
            Err(ShapeError::TriStateListElement)
        } else {
            Ok(Self(Repr::List(Box::new(element))))
        }
    }

    pub(crate) fn map(value: Shape) -> Result<Self, ShapeError> {
        if matches!(value.0, Repr::TriState(_)) {
            Err(ShapeError::TriStateMapValue)
        } else {
            Ok(Self(Repr::Map(Box::new(value))))
        }
    }

    pub(crate) fn structure(
        fields: impl IntoIterator<Item = (String, Shape)>,
    ) -> Result<Self, ShapeError> {
        let mut result = Vec::new();
        for (name, shape) in fields {
            if result
                .iter()
                .any(|(known, _): &(String, Shape)| known == &name)
            {
                return Err(ShapeError::DuplicateField(name));
            }
            result.push((name, shape));
        }
        Ok(Self(Repr::Struct(result)))
    }

    pub(crate) fn enumeration(
        variants: impl IntoIterator<Item = (String, VariantShape)>,
    ) -> Result<Self, ShapeError> {
        let mut result = Vec::new();
        for (tag, variant) in variants {
            if result
                .iter()
                .any(|(known, _): &(String, VariantShape)| known == &tag)
            {
                return Err(ShapeError::DuplicateVariant(tag));
            }
            if matches!(&variant, VariantShape::Value(Shape(Repr::TriState(_)))) {
                return Err(ShapeError::TriStateEnumPayload);
            }
            result.push((tag, variant));
        }
        Ok(Self(Repr::Enum(result)))
    }

    pub(crate) fn optional(inner: Shape) -> Result<Self, ShapeError> {
        Self::wrapper(inner, false)
    }

    pub(crate) fn tri_state(inner: Shape) -> Result<Self, ShapeError> {
        Self::wrapper(inner, true)
    }

    fn wrapper(inner: Shape, tri_state: bool) -> Result<Self, ShapeError> {
        if matches!(inner.0, Repr::Optional(_) | Repr::TriState(_)) {
            return Err(ShapeError::NestedPresence);
        }
        Ok(if tri_state {
            Self(Repr::TriState(Box::new(inner)))
        } else {
            Self(Repr::Optional(Box::new(inner)))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShapeError {
    NestedPresence,
    TriStateListElement,
    TriStateMapValue,
    TriStateEnumPayload,
    DuplicateField(String),
    DuplicateVariant(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConformanceErrorKind {
    MissingRequired,
    UnexpectedNull,
    UnexpectedMissing,
    UnexpectedPayload,
    KindMismatch,
    UnknownField(String),
    UnknownVariant(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConformanceError {
    pub(crate) path: Vec<PathSegment>,
    pub(crate) kind: ConformanceErrorKind,
}

impl fmt::Display for ConformanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?} at {:?}", self.kind, self.path)
    }
}

pub(crate) fn conform_slot(
    shape: &Shape,
    role: DecodeRole,
    slot: SlotValue,
) -> Result<SlotValue, ConformanceError> {
    conform_payload(shape, role, &slot, &mut Vec::new())
}

fn conform_payload(
    shape: &Shape,
    role: DecodeRole,
    slot: &SlotValue,
    path: &mut Vec<PathSegment>,
) -> Result<SlotValue, ConformanceError> {
    match slot {
        SlotValue::Missing => match shape.0 {
            Repr::TriState(_) => Ok(SlotValue::Missing),
            Repr::Optional(_) => fail(path, ConformanceErrorKind::UnexpectedMissing),
            _ => fail(path, ConformanceErrorKind::MissingRequired),
        },
        SlotValue::Null => match shape.0 {
            Repr::Optional(_) | Repr::TriState(_) => Ok(SlotValue::Null),
            _ => fail(path, ConformanceErrorKind::UnexpectedNull),
        },
        SlotValue::Value(value) => {
            let inner = match &shape.0 {
                Repr::Optional(inner) | Repr::TriState(inner) => inner.as_ref(),
                _ => shape,
            };
            conform_value(inner, role, value, path, Position::Element).map(SlotValue::Value)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Field,
    Element,
}

fn conform_value(
    shape: &Shape,
    role: DecodeRole,
    value: &ContractValue,
    path: &mut Vec<PathSegment>,
    position: Position,
) -> Result<ContractValue, ConformanceError> {
    let (shape, nullable) = match &shape.0 {
        Repr::Optional(inner) => (inner.as_ref(), position == Position::Element),
        Repr::TriState(inner) => (inner.as_ref(), position == Position::Field),
        _ => (shape, false),
    };
    match value.view() {
        ValueRef::Null if nullable => return Ok(ContractValue::null()),
        ValueRef::Null => return fail(path, ConformanceErrorKind::UnexpectedNull),
        _ => {}
    }
    match (&shape.0, value.view()) {
        (Repr::Bool, ValueRef::Bool(value)) => Ok(ContractValue::bool(value)),
        (Repr::I64, ValueRef::I64(value)) => Ok(ContractValue::i64(value)),
        (Repr::U64, ValueRef::U64(value)) => Ok(ContractValue::u64(value)),
        (Repr::F32, ValueRef::F32(value)) => Ok(ContractValue::f32(value).unwrap()),
        (Repr::F64, ValueRef::F64(value)) => Ok(ContractValue::f64(value).unwrap()),
        (Repr::String, ValueRef::String(value)) => Ok(ContractValue::string(value)),
        (Repr::Bytes, ValueRef::Bytes(value)) => Ok(ContractValue::bytes(value)),
        (Repr::List(element), ValueRef::List(values)) => {
            let values = values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    descend(path, PathSegment::Index(index), |path| {
                        conform_value(element, role, value, path, Position::Element)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ContractValue::list(values))
        }
        (Repr::Map(element), ValueRef::Object(object)) => conform_map(element, role, object, path),
        (Repr::Struct(fields), ValueRef::Object(object)) => {
            conform_struct(fields, role, object, path)
        }
        (Repr::Enum(variants), ValueRef::Enum { tag, payload }) => {
            conform_enum(variants, role, tag, payload, path)
        }
        _ => fail(path, ConformanceErrorKind::KindMismatch),
    }
}

fn conform_enum(
    variants: &[(String, VariantShape)],
    role: DecodeRole,
    tag: &str,
    payload: &SlotValue,
    path: &mut Vec<PathSegment>,
) -> Result<ContractValue, ConformanceError> {
    descend(path, PathSegment::Variant(tag.into()), |path| {
        let Some((_, variant)) = variants.iter().find(|(known, _)| known == tag) else {
            return match role {
                DecodeRole::ProviderInput => {
                    fail(path, ConformanceErrorKind::UnknownVariant(tag.into()))
                }
                DecodeRole::ConsumerOutput => Ok(ContractValue::enum_value(
                    tag,
                    SlotValue::Value(ContractValue::opaque(OpaquePayload::capture(payload))),
                )),
            };
        };

        let payload = match variant {
            VariantShape::Unit => match payload {
                SlotValue::Null => Ok(SlotValue::Null),
                SlotValue::Missing => fail(path, ConformanceErrorKind::UnexpectedMissing),
                SlotValue::Value(_) => fail(path, ConformanceErrorKind::UnexpectedPayload),
            },
            VariantShape::Value(shape) => conform_payload(shape, role, payload, path),
        }?;
        Ok(ContractValue::enum_value(tag, payload))
    })
}

fn conform_map(
    element: &Shape,
    role: DecodeRole,
    object: ObjectRef<'_>,
    path: &mut Vec<PathSegment>,
) -> Result<ContractValue, ConformanceError> {
    let mut output = Vec::new();
    for (key, value) in object.entries() {
        output.push(descend(path, PathSegment::MapKey(key.into()), |path| {
            conform_value(element, role, value, path, Position::Element)
                .map(|value| (key.into(), value))
        })?);
    }
    ContractValue::object(output).map_err(|_| unreachable!())
}

fn conform_struct(
    fields: &[(String, Shape)],
    role: DecodeRole,
    object: ObjectRef<'_>,
    path: &mut Vec<PathSegment>,
) -> Result<ContractValue, ConformanceError> {
    let mut output = Vec::new();
    for (name, value) in object.entries() {
        let Some((_, shape)) = fields.iter().find(|(field, _)| field == name) else {
            if role == DecodeRole::ConsumerOutput {
                continue;
            }
            return descend(path, PathSegment::Field(name.into()), |path| {
                fail(path, ConformanceErrorKind::UnknownField(name.into()))
            });
        };
        output.push(descend(path, PathSegment::Field(name.into()), |path| {
            conform_value(shape, role, value, path, Position::Field)
                .map(|value| (name.into(), value))
        })?);
    }
    for (name, shape) in fields {
        if object.get(name).is_none() && !matches!(shape.0, Repr::Optional(_) | Repr::TriState(_)) {
            return descend(path, PathSegment::Field(name.clone()), |path| {
                fail(path, ConformanceErrorKind::MissingRequired)
            });
        }
    }
    ContractValue::object(output).map_err(|_| unreachable!())
}

fn descend<T>(
    path: &mut Vec<PathSegment>,
    segment: PathSegment,
    operation: impl FnOnce(&mut Vec<PathSegment>) -> Result<T, ConformanceError>,
) -> Result<T, ConformanceError> {
    path.push(segment);
    let result = operation(path);
    path.pop();
    result
}

fn fail<T>(path: &[PathSegment], kind: ConformanceErrorKind) -> Result<T, ConformanceError> {
    Err(ConformanceError {
        path: path.into(),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OpaquePayload, OpaqueTree};

    const ROLES: [DecodeRole; 2] = [DecodeRole::ProviderInput, DecodeRole::ConsumerOutput];

    fn slot(value: ContractValue) -> SlotValue {
        SlotValue::Value(value)
    }

    fn object(entries: Vec<(&str, ContractValue)>) -> ContractValue {
        ContractValue::object(entries.into_iter().map(|(key, value)| (key.into(), value))).unwrap()
    }

    fn field(shape: Shape) -> Shape {
        Shape::structure([("x".into(), shape)]).unwrap()
    }

    fn enumeration(tag: &str, variant: VariantShape) -> Shape {
        Shape::enumeration([(tag.into(), variant)]).unwrap()
    }

    fn enum_slot(tag: &str, payload: SlotValue) -> SlotValue {
        slot(ContractValue::enum_value(tag, payload))
    }

    fn unknown_capture(output: &SlotValue) -> (&str, &OpaqueTree) {
        let SlotValue::Value(value) = output else {
            panic!()
        };
        let ValueRef::Enum { tag, payload } = value.view() else {
            panic!()
        };
        let SlotValue::Value(value) = payload else {
            panic!()
        };
        let ValueRef::Opaque(payload) = value.view() else {
            panic!()
        };
        (tag, payload.reveal())
    }

    fn assert_accepts(shape: Shape, input: SlotValue) {
        for role in ROLES {
            let output = conform_slot(&shape, role, input.clone()).unwrap();
            assert_eq!(output, input);
            assert_eq!(conform_slot(&shape, role, input.clone()).unwrap(), output);
            assert_eq!(conform_slot(&shape, role, output.clone()).unwrap(), output);
        }
    }

    fn assert_rejects(
        shape: Shape,
        input: SlotValue,
        kind: ConformanceErrorKind,
        path: Vec<PathSegment>,
    ) {
        for role in ROLES {
            assert_eq!(
                conform_slot(&shape, role, input.clone()).unwrap_err(),
                ConformanceError {
                    path: path.clone(),
                    kind: kind.clone(),
                }
            );
        }
    }

    #[test]
    fn legal_primitive_and_composite_shapes_construct() {
        let fields = [
            ("bool".into(), Shape::bool()),
            ("i64".into(), Shape::i64()),
            ("u64".into(), Shape::u64()),
            ("f32".into(), Shape::f32()),
            ("f64".into(), Shape::f64()),
            ("string".into(), Shape::string()),
            ("bytes".into(), Shape::bytes()),
            (
                "optional-list".into(),
                Shape::optional(Shape::list(Shape::i64()).unwrap()).unwrap(),
            ),
            ("tri-state".into(), Shape::tri_state(Shape::i64()).unwrap()),
        ];
        let shape = Shape::structure(fields).unwrap();
        let Repr::Struct(fields) = shape.0 else {
            panic!()
        };
        assert_eq!(fields.len(), 9);
        let Repr::Optional(optional) = &fields[7].1.0 else {
            panic!()
        };
        let Repr::List(element) = &optional.0 else {
            panic!()
        };
        assert!(matches!(element.0, Repr::I64));
        let Repr::TriState(tri_state) = &fields[8].1.0 else {
            panic!()
        };
        assert!(matches!(tri_state.0, Repr::I64));
    }

    #[test]
    fn every_direct_wrapper_on_wrapper_is_rejected() {
        for outer_tri_state in [false, true] {
            for inner_tri_state in [false, true] {
                let inner = if inner_tri_state {
                    Shape::tri_state(Shape::i64())
                } else {
                    Shape::optional(Shape::i64())
                }
                .unwrap();
                let result = if outer_tri_state {
                    Shape::tri_state(inner)
                } else {
                    Shape::optional(inner)
                };
                assert!(matches!(result, Err(ShapeError::NestedPresence)));
            }
        }
    }

    #[test]
    fn tri_state_list_elements_and_duplicate_struct_fields_are_rejected() {
        let tri_state = Shape::tri_state(Shape::i64()).unwrap();
        assert!(matches!(
            Shape::list(tri_state),
            Err(ShapeError::TriStateListElement)
        ));

        let duplicate = Shape::structure([
            ("same".into(), Shape::i64()),
            ("same".into(), Shape::bool()),
        ]);
        assert!(matches!(
            duplicate,
            Err(ShapeError::DuplicateField(name)) if name == "same"
        ));
    }

    #[test]
    fn map_construction_accepts_optional_and_rejects_tri_state_values() {
        let optional = Shape::optional(Shape::i64()).unwrap();
        let map = Shape::map(optional).unwrap();
        assert!(matches!(
            map.0,
            Repr::Map(value) if matches!(value.0, Repr::Optional(_))
        ));

        let tri_state = Shape::tri_state(Shape::i64()).unwrap();
        assert!(matches!(
            Shape::map(tri_state),
            Err(ShapeError::TriStateMapValue)
        ));
    }

    #[test]
    fn enum_construction_distinguishes_variants_and_rejects_invalid_shapes() {
        let optional = Shape::optional(Shape::string()).unwrap();
        let shape = Shape::enumeration([
            ("unit".into(), VariantShape::Unit),
            ("value".into(), VariantShape::Value(optional)),
        ])
        .unwrap();
        let Repr::Enum(variants) = shape.0 else {
            panic!()
        };
        assert!(matches!(variants[0].1, VariantShape::Unit));
        assert!(matches!(
            variants[1].1,
            VariantShape::Value(Shape(Repr::Optional(_)))
        ));

        let duplicate = Shape::enumeration([
            ("same".into(), VariantShape::Unit),
            ("same".into(), VariantShape::Value(Shape::i64())),
        ]);
        assert!(matches!(
            duplicate,
            Err(ShapeError::DuplicateVariant(tag)) if tag == "same"
        ));

        let tri_state = Shape::tri_state(Shape::i64()).unwrap();
        assert!(matches!(
            Shape::enumeration([("invalid".into(), VariantShape::Value(tri_state))]),
            Err(ShapeError::TriStateEnumPayload)
        ));
    }

    #[test]
    fn known_unit_variants_require_the_canonical_null_payload() {
        let shape = enumeration("ready", VariantShape::Unit);
        assert_accepts(shape.clone(), enum_slot("ready", SlotValue::Null));
        assert_rejects(
            shape.clone(),
            enum_slot("ready", SlotValue::Missing),
            ConformanceErrorKind::UnexpectedMissing,
            vec![PathSegment::Variant("ready".into())],
        );
        assert_rejects(
            shape,
            enum_slot("ready", slot(ContractValue::string("payload"))),
            ConformanceErrorKind::UnexpectedPayload,
            vec![PathSegment::Variant("ready".into())],
        );
    }

    #[test]
    fn known_value_variants_follow_top_slot_presence_and_kind_rules() {
        let path = || vec![PathSegment::Variant("count".into())];
        let scalar = || enumeration("count", VariantShape::Value(Shape::i64()));
        assert_rejects(
            scalar(),
            enum_slot("count", SlotValue::Missing),
            ConformanceErrorKind::MissingRequired,
            path(),
        );
        assert_rejects(
            scalar(),
            enum_slot("count", SlotValue::Null),
            ConformanceErrorKind::UnexpectedNull,
            path(),
        );
        assert_accepts(scalar(), enum_slot("count", slot(ContractValue::i64(7))));
        assert_rejects(
            scalar(),
            enum_slot("count", slot(ContractValue::string("seven"))),
            ConformanceErrorKind::KindMismatch,
            path(),
        );

        let optional = || {
            enumeration(
                "count",
                VariantShape::Value(Shape::optional(Shape::i64()).unwrap()),
            )
        };
        assert_rejects(
            optional(),
            enum_slot("count", SlotValue::Missing),
            ConformanceErrorKind::UnexpectedMissing,
            path(),
        );
        assert_accepts(optional(), enum_slot("count", SlotValue::Null));
        assert_accepts(optional(), enum_slot("count", slot(ContractValue::i64(7))));
        assert_rejects(
            optional(),
            enum_slot("count", slot(ContractValue::bool(true))),
            ConformanceErrorKind::KindMismatch,
            path(),
        );
    }

    #[test]
    fn complete_presence_grid_is_enforced_in_both_roles() {
        let integer = || ContractValue::i64(7);

        assert_rejects(
            Shape::i64(),
            SlotValue::Missing,
            ConformanceErrorKind::MissingRequired,
            vec![],
        );
        assert_rejects(
            Shape::i64(),
            SlotValue::Null,
            ConformanceErrorKind::UnexpectedNull,
            vec![],
        );
        assert_accepts(Shape::i64(), slot(integer()));

        assert_rejects(
            Shape::optional(Shape::i64()).unwrap(),
            SlotValue::Missing,
            ConformanceErrorKind::UnexpectedMissing,
            vec![],
        );
        assert_accepts(Shape::optional(Shape::i64()).unwrap(), SlotValue::Null);
        assert_accepts(Shape::optional(Shape::i64()).unwrap(), slot(integer()));

        assert_accepts(Shape::tri_state(Shape::i64()).unwrap(), SlotValue::Missing);
        assert_accepts(Shape::tri_state(Shape::i64()).unwrap(), SlotValue::Null);
        assert_accepts(Shape::tri_state(Shape::i64()).unwrap(), slot(integer()));

        let field_path = || vec![PathSegment::Field("x".into())];
        assert_rejects(
            field(Shape::i64()),
            slot(object(vec![])),
            ConformanceErrorKind::MissingRequired,
            field_path(),
        );
        assert_rejects(
            field(Shape::i64()),
            slot(object(vec![("x", ContractValue::null())])),
            ConformanceErrorKind::UnexpectedNull,
            field_path(),
        );
        assert_accepts(field(Shape::i64()), slot(object(vec![("x", integer())])));

        let optional = || Shape::optional(Shape::i64()).unwrap();
        assert_accepts(field(optional()), slot(object(vec![])));
        assert_rejects(
            field(optional()),
            slot(object(vec![("x", ContractValue::null())])),
            ConformanceErrorKind::UnexpectedNull,
            field_path(),
        );
        assert_accepts(field(optional()), slot(object(vec![("x", integer())])));

        let tri_state = || Shape::tri_state(Shape::i64()).unwrap();
        assert_accepts(field(tri_state()), slot(object(vec![])));
        assert_accepts(
            field(tri_state()),
            slot(object(vec![("x", ContractValue::null())])),
        );
        assert_accepts(field(tri_state()), slot(object(vec![("x", integer())])));

        assert_accepts(
            Shape::list(Shape::i64()).unwrap(),
            slot(ContractValue::list([integer()])),
        );
        assert_rejects(
            Shape::list(Shape::i64()).unwrap(),
            slot(ContractValue::list([ContractValue::null()])),
            ConformanceErrorKind::UnexpectedNull,
            vec![PathSegment::Index(0)],
        );
        assert_accepts(
            Shape::list(optional()).unwrap(),
            slot(ContractValue::list([ContractValue::null(), integer()])),
        );

        let nested = field(Shape::list(optional()).unwrap());
        assert_accepts(
            nested,
            slot(object(vec![(
                "x",
                ContractValue::list([ContractValue::null(), integer()]),
            )])),
        );
    }

    #[test]
    fn scalar_kinds_match_exactly_without_coercion() {
        let scalars = [
            (Shape::bool(), ContractValue::bool(true)),
            (Shape::i64(), ContractValue::i64(1)),
            (Shape::u64(), ContractValue::u64(1)),
            (Shape::f32(), ContractValue::f32(1.0).unwrap()),
            (Shape::f64(), ContractValue::f64(1.0).unwrap()),
            (Shape::string(), ContractValue::string("one")),
            (Shape::bytes(), ContractValue::bytes([1])),
        ];
        for (expected, (shape, value)) in scalars.iter().enumerate() {
            assert_accepts(shape.clone(), slot(value.clone()));
            for (actual, (_, value)) in scalars.iter().enumerate() {
                if actual != expected {
                    assert_rejects(
                        shape.clone(),
                        slot(value.clone()),
                        ConformanceErrorKind::KindMismatch,
                        vec![],
                    );
                }
            }
        }
    }

    #[test]
    fn strict_unknown_fields_reject_and_tolerant_fields_drop_in_input_order() {
        const SENTINEL: &str = "unknown-runtime-value";
        let shape =
            Shape::structure([("a".into(), Shape::i64()), ("b".into(), Shape::i64())]).unwrap();
        let input = slot(object(vec![
            ("b", ContractValue::i64(2)),
            ("extra", ContractValue::string(SENTINEL)),
            ("a", ContractValue::i64(1)),
        ]));
        let error = conform_slot(&shape, DecodeRole::ProviderInput, input.clone()).unwrap_err();
        assert_eq!(
            error,
            ConformanceError {
                path: vec![PathSegment::Field("extra".into())],
                kind: ConformanceErrorKind::UnknownField("extra".into()),
            }
        );
        assert!(!format!("{error:?} {error}").contains(SENTINEL));

        let expected = slot(object(vec![
            ("b", ContractValue::i64(2)),
            ("a", ContractValue::i64(1)),
        ]));
        let output = conform_slot(&shape, DecodeRole::ConsumerOutput, input.clone()).unwrap();
        assert_eq!(output, expected);
        assert_eq!(
            conform_slot(&shape, DecodeRole::ConsumerOutput, input).unwrap(),
            output
        );
        assert_eq!(
            conform_slot(&shape, DecodeRole::ConsumerOutput, output.clone()).unwrap(),
            output
        );
    }

    #[test]
    fn rejected_runtime_values_never_appear_in_diagnostics() {
        const SENTINEL: &str = "rejected-runtime-value";
        let inputs = [
            (Shape::bool(), slot(ContractValue::string(SENTINEL)), vec![]),
            (
                Shape::list(Shape::bool()).unwrap(),
                slot(ContractValue::list([ContractValue::string(SENTINEL)])),
                vec![PathSegment::Index(0)],
            ),
            (
                field(Shape::bool()),
                slot(object(vec![("x", ContractValue::string(SENTINEL))])),
                vec![PathSegment::Field("x".into())],
            ),
            (
                Shape::bool(),
                slot(ContractValue::sensitive(ContractValue::string(SENTINEL))),
                vec![],
            ),
            (
                Shape::bool(),
                slot(ContractValue::opaque(OpaquePayload::new(
                    OpaqueTree::String(SENTINEL.into()),
                ))),
                vec![],
            ),
        ];
        for (shape, input, path) in inputs {
            for role in ROLES {
                let error = conform_slot(&shape, role, input.clone()).unwrap_err();
                assert_eq!(error.kind, ConformanceErrorKind::KindMismatch);
                assert_eq!(error.path, path);
                assert!(!format!("{error:?} {error}").contains(SENTINEL));
            }
        }
    }

    #[test]
    fn map_values_preserve_arbitrary_keys_order_and_presence_in_both_roles() {
        let input = slot(object(vec![
            ("second/key", ContractValue::i64(2)),
            ("", ContractValue::i64(1)),
            ("not-a-schema-field", ContractValue::i64(3)),
        ]));
        assert_accepts(Shape::map(Shape::i64()).unwrap(), input);

        assert_rejects(
            Shape::map(Shape::i64()).unwrap(),
            slot(object(vec![("null-key", ContractValue::null())])),
            ConformanceErrorKind::UnexpectedNull,
            vec![PathSegment::MapKey("null-key".into())],
        );
        assert_accepts(
            Shape::map(Shape::optional(Shape::i64()).unwrap()).unwrap(),
            slot(object(vec![
                ("null", ContractValue::null()),
                ("value", ContractValue::i64(4)),
            ])),
        );
        assert_rejects(
            Shape::map(Shape::i64()).unwrap(),
            slot(ContractValue::list([])),
            ConformanceErrorKind::KindMismatch,
            vec![],
        );
    }

    #[test]
    fn struct_values_inside_maps_remain_strict_or_tolerant_by_role() {
        const SENTINEL: &str = "nested-map-runtime-value";
        let entry =
            Shape::structure([("a".into(), Shape::i64()), ("b".into(), Shape::i64())]).unwrap();
        let shape = Shape::map(entry).unwrap();
        let input = slot(object(vec![
            (
                "customer/α",
                object(vec![
                    ("b", ContractValue::i64(2)),
                    ("extra", ContractValue::string(SENTINEL)),
                    ("a", ContractValue::i64(1)),
                ]),
            ),
            (
                "second",
                object(vec![
                    ("a", ContractValue::i64(3)),
                    ("b", ContractValue::i64(4)),
                ]),
            ),
        ]));

        let error = conform_slot(&shape, DecodeRole::ProviderInput, input.clone()).unwrap_err();
        assert_eq!(
            error,
            ConformanceError {
                path: vec![
                    PathSegment::MapKey("customer/α".into()),
                    PathSegment::Field("extra".into()),
                ],
                kind: ConformanceErrorKind::UnknownField("extra".into()),
            }
        );
        assert!(!format!("{error:?} {error}").contains(SENTINEL));

        let expected = slot(object(vec![
            (
                "customer/α",
                object(vec![
                    ("b", ContractValue::i64(2)),
                    ("a", ContractValue::i64(1)),
                ]),
            ),
            (
                "second",
                object(vec![
                    ("a", ContractValue::i64(3)),
                    ("b", ContractValue::i64(4)),
                ]),
            ),
        ]));
        let output = conform_slot(&shape, DecodeRole::ConsumerOutput, input.clone()).unwrap();
        assert_eq!(output, expected);
        assert_eq!(
            conform_slot(&shape, DecodeRole::ConsumerOutput, input).unwrap(),
            output
        );
        assert_eq!(
            conform_slot(&shape, DecodeRole::ConsumerOutput, output.clone()).unwrap(),
            output
        );
    }

    #[test]
    fn known_enum_payloads_recurse_through_structs_lists_and_maps() {
        const SENTINEL: &str = "nested-enum-runtime-value";
        let entry = Shape::structure([("required".into(), Shape::i64())]).unwrap();
        let payload = Shape::structure([(
            "items".into(),
            Shape::list(Shape::map(entry).unwrap()).unwrap(),
        )])
        .unwrap();
        let shape = enumeration("batch", VariantShape::Value(payload));
        let input = enum_slot(
            "batch",
            slot(object(vec![(
                "items",
                ContractValue::list([object(vec![(
                    "entry",
                    object(vec![
                        ("extra", ContractValue::string(SENTINEL)),
                        ("required", ContractValue::i64(1)),
                    ]),
                )])]),
            )])),
        );

        let error = conform_slot(&shape, DecodeRole::ProviderInput, input.clone()).unwrap_err();
        assert_eq!(
            error,
            ConformanceError {
                path: vec![
                    PathSegment::Variant("batch".into()),
                    PathSegment::Field("items".into()),
                    PathSegment::Index(0),
                    PathSegment::MapKey("entry".into()),
                    PathSegment::Field("extra".into()),
                ],
                kind: ConformanceErrorKind::UnknownField("extra".into()),
            }
        );
        assert!(!format!("{error:?} {error}").contains(SENTINEL));

        let expected = enum_slot(
            "batch",
            slot(object(vec![(
                "items",
                ContractValue::list([object(vec![(
                    "entry",
                    object(vec![("required", ContractValue::i64(1))]),
                )])]),
            )])),
        );
        let output = conform_slot(&shape, DecodeRole::ConsumerOutput, input.clone()).unwrap();
        assert_eq!(output, expected);
        assert_eq!(
            conform_slot(&shape, DecodeRole::ConsumerOutput, input).unwrap(),
            output
        );
        assert_eq!(
            conform_slot(&shape, DecodeRole::ConsumerOutput, output.clone()).unwrap(),
            output
        );
        assert_accepts(shape, expected);
    }

    #[test]
    fn unknown_variants_are_strict_or_captured_opaquely_by_role() {
        const SENTINEL: &str = "unknown-enum-runtime-value";
        let duplicate_tree = OpaqueTree::Object(vec![
            ("same".into(), OpaqueTree::String(SENTINEL.into())),
            ("same".into(), OpaqueTree::Bool(true)),
        ]);
        let cases = vec![
            (SlotValue::Missing, OpaqueTree::Null),
            (SlotValue::Null, OpaqueTree::Null),
            (
                slot(ContractValue::bytes([0xfb])),
                OpaqueTree::Object(vec![("base64".into(), OpaqueTree::String("+w==".into()))]),
            ),
            (
                slot(ContractValue::list([object(vec![(
                    "secret",
                    ContractValue::string(SENTINEL),
                )])])),
                OpaqueTree::List(vec![OpaqueTree::Object(vec![(
                    "secret".into(),
                    OpaqueTree::String(SENTINEL.into()),
                )])]),
            ),
            (
                slot(ContractValue::sensitive(ContractValue::string(SENTINEL))),
                OpaqueTree::String(SENTINEL.into()),
            ),
            (
                slot(ContractValue::opaque(OpaquePayload::new(
                    duplicate_tree.clone(),
                ))),
                duplicate_tree,
            ),
        ];
        let shape = enumeration("known", VariantShape::Unit);

        for (payload, expected_capture) in cases {
            let input = enum_slot("future", payload);
            let error = conform_slot(&shape, DecodeRole::ProviderInput, input.clone()).unwrap_err();
            assert_eq!(
                error,
                ConformanceError {
                    path: vec![PathSegment::Variant("future".into())],
                    kind: ConformanceErrorKind::UnknownVariant("future".into()),
                }
            );
            assert!(!format!("{error:?} {error}").contains(SENTINEL));

            let output = conform_slot(&shape, DecodeRole::ConsumerOutput, input.clone()).unwrap();
            let (tag, captured) = unknown_capture(&output);
            assert_eq!(tag, "future");
            assert_eq!(captured, &expected_capture);
            assert!(!format!("{output:?}").contains(SENTINEL));
            assert_eq!(
                conform_slot(&shape, DecodeRole::ConsumerOutput, input).unwrap(),
                output
            );
            assert_eq!(
                conform_slot(&shape, DecodeRole::ConsumerOutput, output.clone()).unwrap(),
                output
            );
        }
    }

    #[test]
    fn rejected_map_values_do_not_leak_through_diagnostics() {
        const SENTINEL: &str = "rejected-map-runtime-value";
        let values = [
            ContractValue::string(SENTINEL),
            ContractValue::sensitive(ContractValue::string(SENTINEL)),
            ContractValue::opaque(OpaquePayload::new(OpaqueTree::String(SENTINEL.into()))),
        ];
        for value in values {
            let input = slot(object(vec![("safe-key", value)]));
            for role in ROLES {
                let error = conform_slot(&Shape::map(Shape::bool()).unwrap(), role, input.clone())
                    .unwrap_err();
                assert_eq!(error.kind, ConformanceErrorKind::KindMismatch);
                assert_eq!(error.path, vec![PathSegment::MapKey("safe-key".into())]);
                assert!(!format!("{error:?} {error}").contains(SENTINEL));
            }
        }
    }
}
