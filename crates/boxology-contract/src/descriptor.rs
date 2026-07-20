//! Owned, transport-neutral contract type descriptors.
//!
//! Unknown-variant opaque capture belongs to conformance, not to the declared
//! variant payload vocabulary. Type names and type-level deprecation remain
//! schema-owned rather than metadata on `TypeDescriptor`.

use std::error::Error;
use std::fmt;

use crate::conform::{ConformanceError, Shape, VariantShape, conform_slot};
use crate::{DecodeRole, SlotValue};

/// An owned contract type descriptor with an invariant-preserving private representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDescriptor(Repr);

macro_rules! descriptor_model {
    ($($constructor:ident => $variant:ident : $doc:literal),+ $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        enum Repr {
            $($variant,)+
            Secret(Box<TypeDescriptor>),
            Optional(Box<TypeDescriptor>),
            TriState(Box<TypeDescriptor>),
            List(Box<TypeDescriptor>),
            Map(Box<TypeDescriptor>),
            Struct(Vec<FieldDescriptor>),
            Enum(Vec<VariantDescriptor>),
        }

        impl TypeDescriptor {
            $(
                #[doc = concat!("Constructs the ", $doc, " descriptor.")]
                pub fn $constructor() -> Self { Self(Repr::$variant) }
            )+

            /// Borrows this descriptor through its complete read-only view.
            pub fn view(&self) -> DescriptorRef<'_> {
                match &self.0 {
                    $(Repr::$variant => DescriptorRef::$variant,)+
                    Repr::Secret(inner) => DescriptorRef::Secret(inner),
                    Repr::Optional(inner) => DescriptorRef::Optional(inner),
                    Repr::TriState(inner) => DescriptorRef::TriState(inner),
                    Repr::List(inner) => DescriptorRef::List(inner),
                    Repr::Map(inner) => DescriptorRef::Map(inner),
                    Repr::Struct(fields) => DescriptorRef::Struct(fields),
                    Repr::Enum(variants) => DescriptorRef::Enum(variants),
                }
            }
        }

        /// A complete borrowed view of a type descriptor.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum DescriptorRef<'a> {
            $(#[doc = concat!("The ", $doc, " descriptor.")] $variant,)+
            /// Sensitive inner descriptor.
            Secret(&'a TypeDescriptor),
            /// Nullable inner descriptor.
            Optional(&'a TypeDescriptor),
            /// Missing-or-null inner descriptor.
            TriState(&'a TypeDescriptor),
            /// List element descriptor.
            List(&'a TypeDescriptor),
            /// String-keyed map value descriptor.
            Map(&'a TypeDescriptor),
            /// Ordered struct fields.
            Struct(&'a [FieldDescriptor]),
            /// Ordered enum variants.
            Enum(&'a [VariantDescriptor]),
        }
    };
}

descriptor_model!(
    bool => Bool: "boolean", i8 => I8: "signed 8-bit integer",
    i16 => I16: "signed 16-bit integer", i32 => I32: "signed 32-bit integer",
    i64 => I64: "signed 64-bit integer", u8 => U8: "unsigned 8-bit integer",
    u16 => U16: "unsigned 16-bit integer", u32 => U32: "unsigned 32-bit integer",
    u64 => U64: "unsigned 64-bit integer", f32 => F32: "32-bit float",
    f64 => F64: "64-bit float", string => String: "UTF-8 string",
    blob => Blob: "owned byte string",
);

macro_rules! unary_descriptors {
    ($($name:ident => $variant:ident, $check:ident, $error:ident, $doc:literal);+ $(;)?) => {$(
        #[doc = $doc]
        pub fn $name(inner: Self) -> Result<Self, DescriptorError> {
            if $check(&inner.0) { Err(DescriptorError::$error) }
            else { Ok(Self(Repr::$variant(Box::new(inner)))) }
        }
    )+};
}

impl TypeDescriptor {
    unary_descriptors!(
        secret => Secret, is_tri_state, TriStateSecretInner, "Constructs a sensitive value descriptor.";
        optional => Optional, is_presence, NestedPresence, "Constructs an optional descriptor.";
        tri_state => TriState, is_presence, NestedPresence, "Constructs a tri-state descriptor.";
        list => List, is_tri_state, TriStateListElement, "Constructs a list descriptor.";
        map => Map, is_tri_state, TriStateMapValue, "Constructs a string-keyed map descriptor.";
    );

    /// Constructs an ordered struct descriptor with unique field names.
    pub fn structure(
        fields: impl IntoIterator<Item = FieldDescriptor>,
    ) -> Result<Self, DescriptorError> {
        unique(
            fields,
            FieldDescriptor::name,
            DescriptorError::DuplicateField,
        )
        .map(|fields| Self(Repr::Struct(fields)))
    }

    /// Constructs an ordered enum descriptor with unique variant tags.
    pub fn enumeration(
        variants: impl IntoIterator<Item = VariantDescriptor>,
    ) -> Result<Self, DescriptorError> {
        let variants = unique(
            variants,
            VariantDescriptor::tag,
            DescriptorError::DuplicateVariant,
        )?;
        if variants.iter().any(|variant| {
            matches!(variant.payload(), VariantPayload::Value(inner) if matches!(inner.0, Repr::TriState(_)))
        }) {
            Err(DescriptorError::TriStateEnumPayload)
        } else {
            Ok(Self(Repr::Enum(variants)))
        }
    }

    /// Conforms a call slot to this descriptor for the selected decoding role.
    pub fn conform(
        &self,
        role: DecodeRole,
        slot: SlotValue,
    ) -> Result<SlotValue, ConformanceError> {
        conform_slot(&self.to_shape(), role, slot)
    }

    fn to_shape(&self) -> Shape {
        // Public descriptor construction enforces the same or stricter
        // recursive invariants than each fallible private shape constructor.
        match &self.0 {
            Repr::Bool => Shape::bool(),
            Repr::I8 | Repr::I16 | Repr::I32 | Repr::I64 => Shape::i64(),
            Repr::U8 | Repr::U16 | Repr::U32 | Repr::U64 => Shape::u64(),
            Repr::F32 => Shape::f32(),
            Repr::F64 => Shape::f64(),
            Repr::String => Shape::string(),
            Repr::Blob => Shape::bytes(),
            Repr::Secret(inner) => Shape::sensitive(inner.to_shape())
                .expect("validated secret descriptor lowers to a sensitive shape"),
            Repr::Optional(inner) => Shape::optional(inner.to_shape())
                .expect("validated optional descriptor lowers to an optional shape"),
            Repr::TriState(inner) => Shape::tri_state(inner.to_shape())
                .expect("validated tri-state descriptor lowers to a tri-state shape"),
            Repr::List(inner) => Shape::list(inner.to_shape())
                .expect("validated list descriptor lowers to a list shape"),
            Repr::Map(inner) => Shape::map(inner.to_shape())
                .expect("validated map descriptor lowers to a map shape"),
            Repr::Struct(fields) => Shape::structure(
                fields
                    .iter()
                    .map(|field| (field.name().into(), field.descriptor().to_shape())),
            )
            .expect("validated struct descriptor lowers to a struct shape"),
            Repr::Enum(variants) => Shape::enumeration(variants.iter().map(|variant| {
                let payload = match variant.payload() {
                    VariantPayload::Unit => VariantShape::Unit,
                    VariantPayload::Value(inner) => VariantShape::Value(inner.to_shape()),
                };
                (variant.tag().into(), payload)
            }))
            .expect("validated enum descriptor lowers to an enum shape"),
        }
    }
}

fn is_presence(repr: &Repr) -> bool {
    matches!(repr, Repr::Optional(_) | Repr::TriState(_))
}

fn is_tri_state(repr: &Repr) -> bool {
    matches!(repr, Repr::TriState(_))
}

/// Deprecation metadata attached to a declared contract element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deprecation {
    note: Option<String>,
}

impl Deprecation {
    /// Constructs deprecation metadata with an optional note.
    pub fn new(note: Option<String>) -> Self {
        Self { note }
    }

    /// Returns the optional note.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

/// A declared enum variant payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VariantPayload {
    /// No payload.
    Unit,
    /// One typed payload.
    Value(TypeDescriptor),
}

macro_rules! named_descriptor {
    ($type:ident, $key:ident, $value:ident : $value_type:ty, $noun:literal) => {
        #[doc = concat!("One named, ordered ", $noun, ".")]
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $type {
            $key: String,
            $value: $value_type,
            deprecation: Option<Deprecation>,
        }

        impl $type {
            #[doc = concat!("Constructs a ", $noun, " without imposing a name grammar.")]
            pub fn new(
                $key: impl Into<String>,
                $value: $value_type,
                deprecation: Option<Deprecation>,
            ) -> Self {
                Self {
                    $key: $key.into(),
                    $value,
                    deprecation,
                }
            }

            #[doc = concat!("Returns the schema-owned ", $noun, " name.")]
            pub fn $key(&self) -> &str {
                &self.$key
            }

            #[doc = concat!("Returns the ", $noun, " value.")]
            pub fn $value(&self) -> &$value_type {
                &self.$value
            }

            #[doc = concat!("Returns the ", $noun, " deprecation metadata.")]
            pub fn deprecation(&self) -> Option<&Deprecation> {
                self.deprecation.as_ref()
            }
        }
    };
}

named_descriptor!(FieldDescriptor, name, descriptor: TypeDescriptor, "struct field");
named_descriptor!(VariantDescriptor, tag, payload: VariantPayload, "enum variant");

/// A failure to construct a legal type descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DescriptorError {
    /// Presence wrappers were nested directly.
    NestedPresence,
    /// A list element was tri-state.
    TriStateListElement,
    /// A map value was tri-state.
    TriStateMapValue,
    /// An enum payload was tri-state.
    TriStateEnumPayload,
    /// A secret inner value was tri-state.
    TriStateSecretInner,
    /// A struct repeated this field name.
    DuplicateField(String),
    /// An enum repeated this variant tag.
    DuplicateVariant(String),
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NestedPresence => formatter.write_str("presence wrappers cannot be nested"),
            Self::TriStateListElement => formatter.write_str("list elements cannot be tri-state"),
            Self::TriStateMapValue => formatter.write_str("map values cannot be tri-state"),
            Self::TriStateEnumPayload => formatter.write_str("enum payloads cannot be tri-state"),
            Self::TriStateSecretInner => formatter.write_str("secret values cannot be tri-state"),
            Self::DuplicateField(name) => write!(formatter, "duplicate struct field: {name:?}"),
            Self::DuplicateVariant(tag) => write!(formatter, "duplicate enum variant: {tag:?}"),
        }
    }
}

impl Error for DescriptorError {}

fn unique<T>(
    items: impl IntoIterator<Item = T>,
    name: impl Fn(&T) -> &str,
    duplicate: impl Fn(String) -> DescriptorError,
) -> Result<Vec<T>, DescriptorError> {
    let mut result = Vec::new();
    for item in items {
        if result.iter().any(|known| name(known) == name(&item)) {
            return Err(duplicate(name(&item).into()));
        }
        result.push(item);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Blob, ConformanceErrorKind, ContractType, ContractValue, DecodeErrorKind, PathSegment,
        ValueRef,
    };

    fn field(name: &str, descriptor: TypeDescriptor) -> FieldDescriptor {
        FieldDescriptor::new(name, descriptor, None)
    }

    fn variant(tag: &str, payload: VariantPayload) -> VariantDescriptor {
        VariantDescriptor::new(tag, payload, None)
    }

    macro_rules! rejects {
        ($($value:expr => $error:expr),+ $(,)?) => {$(
            assert_eq!($value.unwrap_err(), $error);
        )+};
    }

    fn view_path(descriptor: &TypeDescriptor) -> String {
        let (name, inner) = match descriptor.view() {
            DescriptorRef::List(inner) => ("list", Some(inner)),
            DescriptorRef::Map(inner) => ("map", Some(inner)),
            DescriptorRef::Optional(inner) => ("optional", Some(inner)),
            DescriptorRef::Secret(inner) => ("secret", Some(inner)),
            DescriptorRef::Blob => ("blob", None),
            _ => panic!(),
        };
        inner.map_or_else(
            || name.into(),
            |inner| format!("{name}/{}", view_path(inner)),
        )
    }

    #[test]
    fn recursive_public_view_exposes_owned_children() {
        let descriptor = TypeDescriptor::list(
            TypeDescriptor::map(
                TypeDescriptor::optional(TypeDescriptor::secret(TypeDescriptor::blob()).unwrap())
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(view_path(&descriptor), "list/map/optional/secret/blob");
    }

    #[test]
    fn every_illegal_position_returns_its_exact_error() {
        let optional = || TypeDescriptor::optional(TypeDescriptor::bool()).unwrap();
        let tri_state = || TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap();
        rejects!(
            TypeDescriptor::optional(optional()) => DescriptorError::NestedPresence,
            TypeDescriptor::optional(tri_state()) => DescriptorError::NestedPresence,
            TypeDescriptor::tri_state(optional()) => DescriptorError::NestedPresence,
            TypeDescriptor::tri_state(tri_state()) => DescriptorError::NestedPresence,
            TypeDescriptor::list(tri_state()) => DescriptorError::TriStateListElement,
            TypeDescriptor::map(tri_state()) => DescriptorError::TriStateMapValue,
            TypeDescriptor::secret(tri_state()) => DescriptorError::TriStateSecretInner,
            TypeDescriptor::enumeration([variant("bad", VariantPayload::Value(tri_state()))])
                => DescriptorError::TriStateEnumPayload,
        );
    }

    #[test]
    fn duplicate_names_return_exact_errors() {
        rejects!(
            TypeDescriptor::structure([
                field("x", TypeDescriptor::i8()), field("x", TypeDescriptor::i16())
            ]) => DescriptorError::DuplicateField("x".into()),
            TypeDescriptor::enumeration([
                variant("x", VariantPayload::Unit), variant("x", VariantPayload::Unit)
            ]) => DescriptorError::DuplicateVariant("x".into()),
        );
        assert_eq!(
            DescriptorError::DuplicateField("field".into()).to_string(),
            "duplicate struct field: \"field\""
        );
        assert_eq!(
            DescriptorError::DuplicateVariant("variant".into()).to_string(),
            "duplicate enum variant: \"variant\""
        );
    }

    #[test]
    fn legal_edges_metadata_and_public_bounds_are_preserved() {
        macro_rules! views { ($($name:ident => $variant:ident),+) => {$(
            assert!(matches!(TypeDescriptor::$name().view(), DescriptorRef::$variant));
        )+} }
        views!(
            bool => Bool, i8 => I8, i16 => I16, i32 => I32, i64 => I64,
            u8 => U8, u16 => U16, u32 => U32, u64 => U64,
            f32 => F32, f64 => F64, string => String, blob => Blob
        );
        let optional = TypeDescriptor::optional(TypeDescriptor::string()).unwrap();
        assert!(TypeDescriptor::secret(optional).is_ok());
        let secret = TypeDescriptor::secret(TypeDescriptor::string()).unwrap();
        assert!(TypeDescriptor::optional(secret.clone()).is_ok());
        assert!(TypeDescriptor::secret(secret).is_ok());
        assert!(TypeDescriptor::structure([]).is_ok());
        assert!(TypeDescriptor::enumeration([]).is_ok());
        assert!(
            TypeDescriptor::structure([field(
                "any name!",
                TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
            )])
            .is_ok()
        );
        let deprecation = Deprecation::new(Some("use next".into()));
        let field =
            FieldDescriptor::new("field", TypeDescriptor::bool(), Some(deprecation.clone()));
        let variant = VariantDescriptor::new("variant", VariantPayload::Unit, Some(deprecation));
        assert_eq!(field.name(), "field");
        assert_eq!(field.deprecation().unwrap().note(), Some("use next"));
        assert_eq!(variant.tag(), "variant");
        assert_eq!(variant.deprecation().unwrap().note(), Some("use next"));
        assert!(matches!(field.descriptor().view(), DescriptorRef::Bool));
        assert!(matches!(variant.payload(), VariantPayload::Unit));

        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<TypeDescriptor>();
        bounds::<Deprecation>();
        bounds::<FieldDescriptor>();
        bounds::<VariantDescriptor>();
        bounds::<VariantPayload>();
        bounds::<DescriptorError>();
        assert_eq!(
            DescriptorError::NestedPresence.to_string(),
            "presence wrappers cannot be nested"
        );
    }

    fn slot(value: ContractValue) -> SlotValue {
        SlotValue::Value(value)
    }

    #[test]
    fn lowering_preserves_carriers_narrow_widths_and_blob_bytes() {
        let role = DecodeRole::ProviderInput;
        let signed = slot(ContractValue::i64(128));
        for descriptor in [
            TypeDescriptor::i8(),
            TypeDescriptor::i16(),
            TypeDescriptor::i32(),
            TypeDescriptor::i64(),
        ] {
            assert_eq!(descriptor.conform(role, signed.clone()), Ok(signed.clone()));
        }
        assert_eq!(
            i8::decode(&signed).unwrap_err().kind(),
            &DecodeErrorKind::OutOfRange
        );
        let unsigned = slot(ContractValue::u64(u64::MAX));
        for descriptor in [
            TypeDescriptor::u8(),
            TypeDescriptor::u16(),
            TypeDescriptor::u32(),
            TypeDescriptor::u64(),
        ] {
            assert_eq!(
                descriptor.conform(role, unsigned.clone()),
                Ok(unsigned.clone())
            );
        }
        for (descriptor, input) in [
            (TypeDescriptor::bool(), slot(ContractValue::bool(true))),
            (
                TypeDescriptor::f32(),
                slot(ContractValue::f32(1.5).unwrap()),
            ),
            (
                TypeDescriptor::f64(),
                slot(ContractValue::f64(2.5).unwrap()),
            ),
            (
                TypeDescriptor::string(),
                slot(ContractValue::string("text")),
            ),
        ] {
            assert_eq!(descriptor.conform(role, input.clone()), Ok(input));
        }
        assert_eq!(
            TypeDescriptor::f32()
                .conform(role, slot(ContractValue::f64(1.5).unwrap()))
                .unwrap_err()
                .kind(),
            &ConformanceErrorKind::KindMismatch
        );
        let bytes = slot(ContractValue::bytes([0, 1, 255]));
        assert_eq!(
            TypeDescriptor::blob().conform(role, bytes.clone()),
            Ok(bytes.clone())
        );
        assert_eq!(Blob::decode(&bytes).unwrap().as_bytes(), &[0, 1, 255]);
    }

    #[test]
    fn secret_lowering_is_exact_nullable_nested_and_redacted() {
        const SENTINEL: &str = "descriptor-secret-sentinel";
        let role = DecodeRole::ProviderInput;
        let secret = TypeDescriptor::secret(TypeDescriptor::string()).unwrap();
        let input = slot(ContractValue::sensitive(ContractValue::string(SENTINEL)));
        let output = secret.conform(role, input.clone()).unwrap();
        assert_eq!(output, input);
        let SlotValue::Value(output) = output else {
            panic!()
        };
        let ValueRef::Sensitive(inner) = output.view() else {
            panic!()
        };
        assert!(matches!(inner.view(), ValueRef::String(SENTINEL)));

        let bare = slot(ContractValue::string(SENTINEL));
        assert_eq!(
            secret.conform(role, bare).unwrap_err().kind(),
            &ConformanceErrorKind::KindMismatch
        );
        assert_eq!(
            TypeDescriptor::string()
                .conform(role, input.clone())
                .unwrap_err()
                .kind(),
            &ConformanceErrorKind::KindMismatch
        );

        let nullable = TypeDescriptor::optional(TypeDescriptor::string()).unwrap();
        let transitive =
            TypeDescriptor::optional(TypeDescriptor::secret(nullable).unwrap()).unwrap();
        let sensitive_null = slot(ContractValue::sensitive(ContractValue::null()));
        assert_eq!(
            transitive.conform(role, sensitive_null.clone()),
            Ok(sensitive_null)
        );
        let nested = TypeDescriptor::secret(secret).unwrap();
        let nested_input = slot(ContractValue::sensitive(ContractValue::sensitive(
            ContractValue::string(SENTINEL),
        )));
        assert_eq!(nested.conform(role, nested_input.clone()), Ok(nested_input));

        let error = TypeDescriptor::secret(TypeDescriptor::bool())
            .unwrap()
            .conform(role, input)
            .unwrap_err();
        assert_eq!(error.kind(), &ConformanceErrorKind::KindMismatch);
        assert!(!format!("{error:?} {error}").contains(SENTINEL));
    }

    #[test]
    fn aggregate_lowering_and_public_errors_are_structural() {
        let payload = TypeDescriptor::map(
            TypeDescriptor::list(TypeDescriptor::secret(TypeDescriptor::blob()).unwrap()).unwrap(),
        )
        .unwrap();
        let event = TypeDescriptor::enumeration([
            variant("idle", VariantPayload::Unit),
            variant("data", VariantPayload::Value(payload)),
        ])
        .unwrap();
        let descriptor = TypeDescriptor::structure([
            FieldDescriptor::new(
                "event",
                event,
                Some(Deprecation::new(Some("legacy".into()))),
            ),
            field(
                "state",
                TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
            ),
        ])
        .unwrap();
        let map = ContractValue::object([(
            "key".into(),
            ContractValue::list([ContractValue::sensitive(ContractValue::bytes([7]))]),
        )])
        .unwrap();
        let input = slot(
            ContractValue::object([(
                "event".into(),
                ContractValue::enum_value("data", SlotValue::Value(map)),
            )])
            .unwrap(),
        );
        assert_eq!(
            descriptor.conform(DecodeRole::ProviderInput, input.clone()),
            Ok(input)
        );

        let error = TypeDescriptor::list(TypeDescriptor::bool())
            .unwrap()
            .conform(
                DecodeRole::ProviderInput,
                slot(ContractValue::list([ContractValue::string("wrong")])),
            )
            .unwrap_err();
        assert_eq!(error.kind(), &ConformanceErrorKind::KindMismatch);
        assert_eq!(error.path(), &[PathSegment::Index(0)]);

        fn error_bounds<T: std::error::Error + Send + Sync + 'static>() {}
        fn bounds<T: Send + Sync + 'static>() {}
        error_bounds::<ConformanceError>();
        bounds::<ConformanceErrorKind>();
    }

    fn object(entries: impl IntoIterator<Item = (&'static str, ContractValue)>) -> ContractValue {
        ContractValue::object(
            entries
                .into_iter()
                .map(|(name, value)| (name.into(), value)),
        )
        .unwrap()
    }

    fn agrees_accepts(descriptor: TypeDescriptor, input: SlotValue) {
        for role in [DecodeRole::ProviderInput, DecodeRole::ConsumerOutput] {
            let private = conform_slot(&descriptor.to_shape(), role, input.clone());
            let public = descriptor.conform(role, input.clone());
            assert_eq!(public, private);
            assert_eq!(public, Ok(input.clone()));
        }
    }

    fn agrees_rejects(
        descriptor: TypeDescriptor,
        input: SlotValue,
        kind: ConformanceErrorKind,
        path: &[PathSegment],
    ) {
        for role in [DecodeRole::ProviderInput, DecodeRole::ConsumerOutput] {
            let private = conform_slot(&descriptor.to_shape(), role, input.clone());
            let public = descriptor.conform(role, input.clone());
            assert_eq!(public, private);
            let error = public.unwrap_err();
            assert_eq!(error.kind(), &kind);
            assert_eq!(error.path(), path);
        }
    }

    fn structure_field(descriptor: TypeDescriptor) -> TypeDescriptor {
        TypeDescriptor::structure([field("x", descriptor)]).unwrap()
    }

    #[test]
    fn public_and_private_presence_grids_agree_in_both_roles() {
        let integer = || ContractValue::i64(7);
        let value = || slot(integer());
        let optional = || TypeDescriptor::optional(TypeDescriptor::i64()).unwrap();
        let tri_state = || TypeDescriptor::tri_state(TypeDescriptor::i64()).unwrap();

        agrees_rejects(
            TypeDescriptor::i64(),
            SlotValue::Missing,
            ConformanceErrorKind::MissingRequired,
            &[],
        );
        agrees_rejects(
            TypeDescriptor::i64(),
            SlotValue::Null,
            ConformanceErrorKind::UnexpectedNull,
            &[],
        );
        agrees_accepts(TypeDescriptor::i64(), value());
        agrees_rejects(
            optional(),
            SlotValue::Missing,
            ConformanceErrorKind::UnexpectedMissing,
            &[],
        );
        agrees_accepts(optional(), SlotValue::Null);
        agrees_accepts(optional(), value());
        agrees_accepts(tri_state(), SlotValue::Missing);
        agrees_accepts(tri_state(), SlotValue::Null);
        agrees_accepts(tri_state(), value());

        let field_path = [PathSegment::Field("x".into())];
        agrees_rejects(
            structure_field(TypeDescriptor::i64()),
            slot(object([])),
            ConformanceErrorKind::MissingRequired,
            &field_path,
        );
        agrees_rejects(
            structure_field(TypeDescriptor::i64()),
            slot(object([("x", ContractValue::null())])),
            ConformanceErrorKind::UnexpectedNull,
            &field_path,
        );
        agrees_accepts(
            structure_field(TypeDescriptor::i64()),
            slot(object([("x", integer())])),
        );
        agrees_accepts(structure_field(optional()), slot(object([])));
        agrees_rejects(
            structure_field(optional()),
            slot(object([("x", ContractValue::null())])),
            ConformanceErrorKind::UnexpectedNull,
            &field_path,
        );
        agrees_accepts(
            structure_field(optional()),
            slot(object([("x", integer())])),
        );
        agrees_accepts(structure_field(tri_state()), slot(object([])));
        agrees_accepts(
            structure_field(tri_state()),
            slot(object([("x", ContractValue::null())])),
        );
        agrees_accepts(
            structure_field(tri_state()),
            slot(object([("x", integer())])),
        );

        let index_path = [PathSegment::Index(0)];
        agrees_accepts(
            TypeDescriptor::list(TypeDescriptor::i64()).unwrap(),
            slot(ContractValue::list([integer()])),
        );
        agrees_rejects(
            TypeDescriptor::list(TypeDescriptor::i64()).unwrap(),
            slot(ContractValue::list([ContractValue::null()])),
            ConformanceErrorKind::UnexpectedNull,
            &index_path,
        );
        agrees_accepts(
            TypeDescriptor::list(optional()).unwrap(),
            slot(ContractValue::list([ContractValue::null(), integer()])),
        );
        let key_path = [PathSegment::MapKey("x".into())];
        agrees_rejects(
            TypeDescriptor::map(TypeDescriptor::i64()).unwrap(),
            slot(object([("x", ContractValue::null())])),
            ConformanceErrorKind::UnexpectedNull,
            &key_path,
        );
        agrees_accepts(
            TypeDescriptor::map(optional()).unwrap(),
            slot(object([("x", ContractValue::null())])),
        );
        agrees_accepts(
            structure_field(TypeDescriptor::list(optional()).unwrap()),
            slot(object([(
                "x",
                ContractValue::list([ContractValue::null(), integer()]),
            )])),
        );

        let enum_slot = |payload| slot(ContractValue::enum_value("event", payload));
        let variant_path = [PathSegment::Variant("event".into())];
        let unit =
            || TypeDescriptor::enumeration([variant("event", VariantPayload::Unit)]).unwrap();
        agrees_accepts(unit(), enum_slot(SlotValue::Null));
        agrees_rejects(
            unit(),
            enum_slot(SlotValue::Missing),
            ConformanceErrorKind::UnexpectedMissing,
            &variant_path,
        );
        agrees_rejects(
            unit(),
            enum_slot(value()),
            ConformanceErrorKind::UnexpectedPayload,
            &variant_path,
        );
        let required = || {
            TypeDescriptor::enumeration([variant(
                "event",
                VariantPayload::Value(TypeDescriptor::i64()),
            )])
            .unwrap()
        };
        agrees_rejects(
            required(),
            enum_slot(SlotValue::Missing),
            ConformanceErrorKind::MissingRequired,
            &variant_path,
        );
        agrees_rejects(
            required(),
            enum_slot(SlotValue::Null),
            ConformanceErrorKind::UnexpectedNull,
            &variant_path,
        );
        agrees_accepts(required(), enum_slot(value()));
        let optional_variant = || {
            TypeDescriptor::enumeration([variant("event", VariantPayload::Value(optional()))])
                .unwrap()
        };
        agrees_rejects(
            optional_variant(),
            enum_slot(SlotValue::Missing),
            ConformanceErrorKind::UnexpectedMissing,
            &variant_path,
        );
        agrees_accepts(optional_variant(), enum_slot(SlotValue::Null));
        agrees_accepts(optional_variant(), enum_slot(value()));
    }

    #[test]
    fn public_struct_conformance_is_strict_or_ordered_and_tolerant() {
        const SENTINEL: &str = "public-unknown-field-sentinel";
        let descriptor = TypeDescriptor::structure([
            field("a", TypeDescriptor::i64()),
            field("b", TypeDescriptor::i64()),
        ])
        .unwrap();
        let input = slot(object([
            ("b", ContractValue::i64(2)),
            ("extra", ContractValue::string(SENTINEL)),
            ("a", ContractValue::i64(1)),
        ]));
        let error = descriptor
            .conform(DecodeRole::ProviderInput, input.clone())
            .unwrap_err();
        assert_eq!(
            error.kind(),
            &ConformanceErrorKind::UnknownField("extra".into())
        );
        assert_eq!(error.path(), &[PathSegment::Field("extra".into())]);
        assert!(!format!("{error:?} {error}").contains(SENTINEL));
        let output = descriptor
            .conform(DecodeRole::ConsumerOutput, input)
            .unwrap();
        assert_eq!(
            output,
            slot(object([
                ("b", ContractValue::i64(2)),
                ("a", ContractValue::i64(1)),
            ]))
        );
    }

    #[test]
    fn aggregate_views_expose_ordered_children_and_every_wrapper() {
        let deprecated = Deprecation::new(Some("legacy".into()));
        let enumeration = TypeDescriptor::enumeration([
            VariantDescriptor::new("unit", VariantPayload::Unit, None),
            VariantDescriptor::new(
                "value",
                VariantPayload::Value(TypeDescriptor::list(TypeDescriptor::i64()).unwrap()),
                Some(deprecated.clone()),
            ),
        ])
        .unwrap();
        let descriptor = TypeDescriptor::structure([
            FieldDescriptor::new(
                "state",
                TypeDescriptor::tri_state(TypeDescriptor::bool()).unwrap(),
                Some(deprecated),
            ),
            field("event", enumeration),
        ])
        .unwrap();
        let DescriptorRef::Struct(fields) = descriptor.view() else {
            panic!()
        };
        assert_eq!(
            fields.iter().map(FieldDescriptor::name).collect::<Vec<_>>(),
            ["state", "event"]
        );
        assert!(matches!(
            fields[0].descriptor().view(),
            DescriptorRef::TriState(_)
        ));
        assert_eq!(fields[0].deprecation().unwrap().note(), Some("legacy"));
        let DescriptorRef::Enum(variants) = fields[1].descriptor().view() else {
            panic!()
        };
        assert_eq!(
            variants
                .iter()
                .map(VariantDescriptor::tag)
                .collect::<Vec<_>>(),
            ["unit", "value"]
        );
        assert!(matches!(variants[0].payload(), VariantPayload::Unit));
        let VariantPayload::Value(value) = variants[1].payload() else {
            panic!()
        };
        assert!(matches!(value.view(), DescriptorRef::List(_)));
        assert_eq!(variants[1].deprecation().unwrap().note(), Some("legacy"));
    }

    #[test]
    fn descriptor_structural_equality_covers_every_semantic_dimension() {
        let a = || field("a", TypeDescriptor::i64());
        let b = || field("b", TypeDescriptor::string());
        let c = || field("c", TypeDescriptor::string());
        let ordered = || TypeDescriptor::structure([a(), b()]).unwrap();
        assert_ne!(TypeDescriptor::bool(), TypeDescriptor::string());
        assert_ne!(TypeDescriptor::i8(), TypeDescriptor::i16());
        assert_ne!(
            TypeDescriptor::list(TypeDescriptor::i8()).unwrap(),
            TypeDescriptor::list(TypeDescriptor::i16()).unwrap()
        );
        assert_ne!(ordered(), TypeDescriptor::structure([b(), a()]).unwrap());
        assert_ne!(
            TypeDescriptor::structure([field("a", TypeDescriptor::i64())]).unwrap(),
            TypeDescriptor::structure([field("renamed", TypeDescriptor::i64())]).unwrap()
        );
        let deprecated = |note: &str| Some(Deprecation::new(Some(note.into())));
        assert_ne!(
            TypeDescriptor::structure([FieldDescriptor::new(
                "a",
                TypeDescriptor::i64(),
                deprecated("one"),
            )])
            .unwrap(),
            TypeDescriptor::structure([FieldDescriptor::new(
                "a",
                TypeDescriptor::i64(),
                deprecated("two"),
            )])
            .unwrap()
        );
        let enumeration = |tag: &str, payload, deprecation| {
            TypeDescriptor::enumeration([VariantDescriptor::new(tag, payload, deprecation)])
                .unwrap()
        };
        assert_ne!(
            enumeration("a", VariantPayload::Unit, None),
            enumeration("b", VariantPayload::Unit, None)
        );
        assert_ne!(
            enumeration("a", VariantPayload::Unit, None),
            enumeration("a", VariantPayload::Value(TypeDescriptor::bool()), None)
        );
        assert_ne!(
            enumeration("a", VariantPayload::Unit, deprecated("one")),
            enumeration("a", VariantPayload::Unit, deprecated("two"))
        );
        let enum_pair = |first, second| {
            TypeDescriptor::enumeration([
                variant(first, VariantPayload::Unit),
                variant(second, VariantPayload::Unit),
            ])
            .unwrap()
        };
        assert_ne!(enum_pair("a", "b"), enum_pair("b", "a"));
        let nested = TypeDescriptor::optional(
            TypeDescriptor::secret(TypeDescriptor::optional(ordered()).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(nested, nested.clone());
        assert_ne!(
            nested,
            TypeDescriptor::optional(
                TypeDescriptor::secret(
                    TypeDescriptor::optional(TypeDescriptor::structure([a(), c()]).unwrap())
                        .unwrap()
                )
                .unwrap()
            )
            .unwrap()
        );
    }
}
