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

#[cfg(test)]
mod tests {
    use super::*;

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
}
