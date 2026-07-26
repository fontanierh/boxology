//! The format-1 Boxology schema document model and its one canonical serializer.
//!
//! Callers provide every value. This crate consults no filesystem, environment, network, locale,
//! or clock. Serialization is total over the model, so it has no failure path at all; a coded
//! failure vocabulary arrives with the strict reader. Format authority stays with S2
//! (`specs/s2-contract-generator.md` D3, D4): per `specs/s4-contract-change-classification.md` D1
//! this is the relocated model the emitter and the classifier share, never a second authority, so
//! it may only ever spell what S2 already spells.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use serde_json::{Value, json};

/// The schema format this crate models and serializes.
pub const SCHEMA_FORMAT: u64 = 1;

macro_rules! boundary_leaves {
    ($($variant:ident => $spelling:literal,)*) => {
        /// One canonical boundary leaf of the format-1 type vocabulary (S2 D3). Deliberately
        /// crate-local: the identical enumeration in `boxology-contract-syntax` belongs to the
        /// contract parser, which no schema consumer should have to depend on.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum BoundaryLeaf {
            $(#[doc = concat!("The `", $spelling, "` boundary leaf.")] $variant,)*
        }

        impl BoundaryLeaf {
            /// Returns the leaf's canonical schema spelling.
            pub fn canonical_name(self) -> &'static str {
                match self { $(Self::$variant => $spelling,)* }
            }
        }
    };
}

#[rustfmt::skip]
boundary_leaves! {
    Bool => "bool", U8 => "u8", U16 => "u16", U32 => "u32", U64 => "u64",
    I8 => "i8", I16 => "i16", I32 => "i32", I64 => "i64",
    F32 => "f32", F64 => "f64", String => "String", Blob => "Blob",
}

/// A capability's declared interaction shape, as format 1 spells it. Format 1's entire shape
/// vocabulary is `unary`: the controlled grammar rejects every other shape (S2 D3), so no document
/// has ever carried one and this crate may not invent a spelling. The remaining
/// `boxology_contract::CapabilityShape` variants join this enumeration when S2 emits them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shape {
    /// One request produces one response.
    Unary,
}

impl Shape {
    /// Returns the shape's canonical schema spelling.
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Unary => "unary",
        }
    }
}

/// The opaque provenance value of a schema document. S4 D1 makes it the one value strictness never
/// looks inside: it sits outside the compatibility surface, and both a live generator object and S2
/// D11's `"@PROVENANCE@"` normalization token are ordinary JSON values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance(Value);

impl Provenance {
    /// Wraps any JSON value as a document's provenance.
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// Returns the wrapped JSON value.
    pub fn value(&self) -> &Value {
        &self.0
    }
}

/// One capability's input parameter slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSlot {
    /// The declared parameter name.
    pub name: String,
    /// The parameter's boundary leaf type.
    pub leaf: BoundaryLeaf,
}

/// One capability's output slot, which format 1 names but does not name a parameter for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSlot {
    /// The output's boundary leaf type.
    pub leaf: BoundaryLeaf,
}

/// One boundary capability of a format-1 schema document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaCapability {
    /// The box-local name; the qualified id is derived from it, never stored twice.
    pub name: CapabilityName,
    /// The doc lines, in declaration order.
    pub docs: Vec<String>,
    /// The deprecation note, present exactly when the capability is deprecated.
    pub deprecation: Option<String>,
    /// The declared identifier of the error type this capability returns.
    pub error: String,
    /// The single input parameter slot.
    pub input: InputSlot,
    /// The output slot.
    pub output: OutputSlot,
    /// The declared interaction shape.
    pub shape: Shape,
    /// The greatest exposure the capability permits.
    pub max_exposure: ExposureLevel,
    /// The declared idempotency property.
    pub idempotency: Idempotency,
}

/// One variant of a declared error type; every format-1 payload is `unit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaVariant {
    /// The variant's declared identifier.
    pub name: String,
    /// The doc lines, in declaration order.
    pub docs: Vec<String>,
    /// The deprecation note, present exactly when the variant is deprecated.
    pub deprecation: Option<String>,
}

/// One declared type of a format-1 schema document; format 1 declares error types only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaType {
    /// The type's declared identifier.
    pub name: String,
    /// The doc lines, in declaration order.
    pub docs: Vec<String>,
    /// The deprecation note, present exactly when the type is deprecated.
    pub deprecation: Option<String>,
    /// The variants, in declaration order.
    pub variants: Vec<SchemaVariant>,
}

/// One complete format-1 schema document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDocument {
    /// The owning box identity.
    pub box_id: BoxId,
    /// The declared capabilities, in declaration order.
    pub capabilities: Vec<SchemaCapability>,
    /// The opaque provenance value.
    pub provenance: Provenance,
    /// The contract revision, spelled `sha256:` followed by 64 lowercase hexadecimal digits.
    pub revision: String,
    /// The declared types, in declaration order.
    pub types: Vec<SchemaType>,
}

impl SchemaDocument {
    /// Serializes the document into its canonical bytes.
    ///
    /// The encoding is the one format 1 has always had: object keys sorted at every level,
    /// two-space pretty printing, LF line endings, and a trailing newline.
    ///
    /// Key sorting is **not** guaranteed by this code. Every object below is written in model
    /// order, not sorted order, and the sort comes from `serde_json::Map` being a `BTreeMap` —
    /// which holds only while nothing in the dependency graph enables `serde_json/preserve_order`.
    /// The emitter it replaces did not depend on that: `boxology-generator`'s `schema.rs` collects
    /// through an explicit `BTreeMap` first, so its bytes survive the feature. S2 owes this codec
    /// the same explicit ordering; until then this crate's fixture-bytes and key-order tests are
    /// what catch a graph-wide `preserve_order`, loudly rather than silently.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes =
            serde_json::to_vec_pretty(&self.value()).expect("schema values are serializable");
        bytes.push(b'\n');
        bytes
    }

    fn value(&self) -> Value {
        let capabilities = self
            .capabilities
            .iter()
            .map(|capability| capability.value(&self.box_id))
            .collect::<Vec<_>>();
        json!({
            "schema_format": SCHEMA_FORMAT,
            "box_id": self.box_id.as_str(),
            "revision": self.revision,
            "provenance": self.provenance.0,
            "capabilities": capabilities,
            "types": self.types.iter().map(SchemaType::value).collect::<Vec<_>>(),
        })
    }
}

impl SchemaCapability {
    fn value(&self, box_id: &BoxId) -> Value {
        json!({
            "name": self.name.as_str(),
            "id": format!("{box_id}.{}", self.name),
            "docs": self.docs,
            "deprecation": deprecation(&self.deprecation),
            "error": self.error,
            "input": {"name": self.input.name, "type": self.input.leaf.canonical_name()},
            "output": {"type": self.output.leaf.canonical_name()},
            "shape": self.shape.canonical_name(),
            "max_exposure": exposure_name(self.max_exposure),
            "idempotency": idempotency_name(self.idempotency),
        })
    }
}

impl SchemaType {
    fn value(&self) -> Value {
        json!({
            "kind": "error",
            "name": self.name,
            "docs": self.docs,
            "deprecation": deprecation(&self.deprecation),
            "variants": self.variants.iter().map(SchemaVariant::value).collect::<Vec<_>>(),
        })
    }
}

impl SchemaVariant {
    fn value(&self) -> Value {
        json!({
            "name": self.name,
            "docs": self.docs,
            "deprecation": deprecation(&self.deprecation),
            "payload": "unit",
        })
    }
}

/// Returns the format-1 spelling of an exposure level: S2 D3's `exposure` grammar tokens.
fn exposure_name(level: ExposureLevel) -> &'static str {
    match level {
        ExposureLevel::CodeOnly => "code_only",
        ExposureLevel::Internal => "internal",
        ExposureLevel::External => "external",
    }
}

/// Returns the format-1 spelling of an idempotency property: S2 D3's `idempotency` tokens.
fn idempotency_name(value: Idempotency) -> &'static str {
    match value {
        Idempotency::None => "none",
        Idempotency::Inherent => "inherent",
    }
}

fn deprecation(note: &Option<String>) -> Value {
    match note {
        None => Value::Null,
        Some(note) => json!({"note": note}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REVISION: &str =
        "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";

    fn capability(name: &str, input: &str, output: BoundaryLeaf, error: &str) -> SchemaCapability {
        SchemaCapability {
            name: CapabilityName::new(name).unwrap(),
            docs: Vec::new(),
            deprecation: None,
            error: error.to_owned(),
            input: InputSlot {
                name: input.to_owned(),
                leaf: BoundaryLeaf::String,
            },
            output: OutputSlot { leaf: output },
            shape: Shape::Unary,
            max_exposure: ExposureLevel::External,
            idempotency: Idempotency::None,
        }
    }

    fn error_type(name: &str, variants: &[&str]) -> SchemaType {
        SchemaType {
            name: name.to_owned(),
            docs: Vec::new(),
            deprecation: None,
            variants: variants
                .iter()
                .map(|name| SchemaVariant {
                    name: (*name).to_owned(),
                    docs: Vec::new(),
                    deprecation: None,
                })
                .collect(),
        }
    }

    fn hello(provenance: Value) -> SchemaDocument {
        let greet = capability("greet", "name", BoundaryLeaf::String, "GreetError");
        SchemaDocument {
            box_id: BoxId::new("hello").unwrap(),
            capabilities: vec![greet],
            provenance: Provenance::new(provenance),
            revision: REVISION.to_owned(),
            types: vec![error_type("GreetError", &["EmptyName"])],
        }
    }

    /// Two capabilities and two variants, each declared out of alphabetical order, so that any
    /// reordering of an array is visible and no key can be sorted merely by how it was authored.
    fn store() -> SchemaDocument {
        let mut put = capability("put", "value", BoundaryLeaf::Bool, "StoreError");
        put.docs = vec!["Stores a value.".to_owned()];
        put.deprecation = Some("use insert".to_owned());
        let get = capability("get", "key", BoundaryLeaf::String, "StoreError");
        SchemaDocument {
            box_id: BoxId::new("store").unwrap(),
            capabilities: vec![put, get],
            provenance: Provenance::new(json!({"generator": "boxology-generator"})),
            revision: REVISION.to_owned(),
            types: vec![error_type("StoreError", &["Missing", "Denied"])],
        }
    }

    /// Returns every object key of a pretty-printed document, in the order the bytes present it.
    fn keys(text: &str) -> Vec<&str> {
        text.lines()
            .filter_map(|line| line.trim_start().strip_prefix('"'))
            .filter_map(|rest| rest.split_once("\": "))
            .map(|(key, _)| key)
            .collect()
    }

    /// Returns every string value spelled under `key`, in the order the bytes present them.
    fn values(text: &str, key: &str) -> Vec<String> {
        let prefix = format!("\"{key}\": \"");
        text.lines()
            .filter_map(|line| line.trim_start().strip_prefix(&prefix))
            .map(|rest| rest.trim_end_matches(',').trim_end_matches('"').to_owned())
            .collect()
    }

    #[test]
    fn provenance_token_document_serializes_to_the_checked_in_fixture_bytes() {
        const FIXTURE: &[u8] = include_bytes!("../../fixtures/hello/generated/schema.json");
        assert_eq!(hello(json!("@PROVENANCE@")).canonical_bytes(), FIXTURE);
    }

    #[test]
    fn two_capability_document_orders_keys_and_preserves_array_order() {
        let text = String::from_utf8(store().canonical_bytes()).unwrap();
        #[rustfmt::skip]
        let expected_keys = [
            "box_id", "capabilities",
            "deprecation", "note", "docs", "error", "id", "idempotency", "input", "name", "type",
            "max_exposure", "name", "output", "type", "shape",
            "deprecation", "docs", "error", "id", "idempotency", "input", "name", "type",
            "max_exposure", "name", "output", "type", "shape",
            "provenance", "generator", "revision", "schema_format", "types",
            "deprecation", "docs", "kind", "name", "variants",
            "deprecation", "docs", "name", "payload",
            "deprecation", "docs", "name", "payload",
        ];
        #[rustfmt::skip]
        let names = ["value", "put", "key", "get", "StoreError", "Missing", "Denied"];
        assert_eq!(keys(&text), expected_keys);
        assert_eq!(values(&text, "name"), names);
    }

    #[test]
    fn public_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(BoundaryLeaf, Shape, Provenance)>();
        bounds::<(InputSlot, OutputSlot, SchemaCapability)>();
        bounds::<(SchemaVariant, SchemaType, SchemaDocument)>();
    }
}
