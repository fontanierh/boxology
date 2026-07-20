use std::fmt;

use crate::{ContractValue, DecodeRole, ObjectRef, SlotValue, ValueRef};

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
    Struct(Vec<(String, Shape)>),
    Optional(Box<Shape>),
    TriState(Box<Shape>),
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
    DuplicateField(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathSegment {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConformanceErrorKind {
    MissingRequired,
    UnexpectedNull,
    UnexpectedMissing,
    KindMismatch,
    UnknownField(String),
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
    let path = &mut Vec::new();
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
            conform_value(inner, role, &value, path, Position::Element).map(SlotValue::Value)
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
        (Repr::Struct(fields), ValueRef::Object(object)) => {
            conform_struct(fields, role, object, path)
        }
        _ => fail(path, ConformanceErrorKind::KindMismatch),
    }
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
}
