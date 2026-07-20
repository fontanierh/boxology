//! Owned, transport-neutral contract type descriptors.
//!
//! Unknown-variant opaque capture belongs to conformance, not to the declared
//! variant payload vocabulary. Type names and type-level deprecation remain
//! schema-owned rather than metadata on `TypeDescriptor`.

use std::error::Error;
use std::fmt;

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
}
