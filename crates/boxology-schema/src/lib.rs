//! The format-1 Boxology schema document model and its one canonical serializer.
//!
//! Callers provide every value. This crate consults no filesystem, environment, network, locale,
//! or clock. Serialization is total over the model, so it has no failure path at all; a coded
//! failure vocabulary arrives with the strict reader. Format authority stays with S2
//! (`specs/s2-contract-generator.md` D3, D4): per `specs/s4-contract-change-classification.md` D1
//! this is the relocated model the emitter and the classifier share, never a second authority, so
//! it may only ever spell what S2 already spells.
//!
//! Rejections are payload-safe by construction, not by review: a [`Diagnostic`] stores a code and a
//! location, its rule and attribution are `&'static str` derived from the code, and the location is
//! built from static key names, array indices, and one gated helper admitting a document's own text
//! only when it is plain, bounded identifier bytes. No other path leads from a document to a report.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

// `BXC####` allocation, recorded so S4's slices cannot collide or strand gaps. The strict reader
// claims BXC0001–BXC0023 and BXC0029–BXC0030, the whole format-1 read inventory: BXC0001–BXC0006 the document gates,
// with unknown key, missing key, and wrong type generic across every level rather than repeated
// per level (`boxology-manifest`'s shape, keeping the initial read inventory compact);
// BXC0007 and BXC0008 the reader's two narrowings; BXC0009 the revision spelling; BXC0010–BXC0014
// the identity namespaces; BXC0015–BXC0023 the contract grammar's own rules. D2's BXC0024–BXC0025
// are registered here, but emitted and reachability-proven by the classifier. The classifier owns
// BXC0026–BXC0028 and BXC0031–BXC0038; BXC0029–BXC0030 remain named-payload-field rules.
//
// The two narrowings are fail-closed and deliberate. `boxology-contract-syntax` hardcodes
// `external` and `none` and rejects every other exposure and idempotency, so no document this
// codec wrote can carry one, and admitting one would mean classifying a document no emitter
// produced. The consequence: widening the emitted grammar must widen this reader in the same
// change, or documents valid under the widened grammar are rejected here.
//
// Recorded non-goals and divergences, so that freezing the inventory does not bury them.
// Duplicate JSON object keys are **not** detected: `serde_json` silently keeps the last, so two
// byte-different documents parse alike and no code covers it. The location grammar here is a JSON
// pointer, not S4 D6's identity path, because a rejection can precede identity — a document whose
// `box_id` is malformed has no identity path to be reported at. BXC0009's rule already exists in
// `boxology-generator-model` attributed to S2 D4 and pinned in that crate's golden; D6 is the
// better home and is what this crate freezes, leaving one rule with two attributions to reconcile.
//
// The lock lands with the inventory rather than after the reader: `ALL_CODES`, the byte-compared
// rule-text and attribution golden, and the compile-time exhaustiveness scan are all in this
// slice. Its reachability half — `corpus_covers_every_code`, one minimal document provoking each
// code — needs something that can parse a document, so it arrives with the reader, and **the
// reader may not merge without it**. The payload field validators and their two identity/uniqueness
// codes belong to this slice, while the classifier-reserved range remains outside this reader.
mod read;

pub use read::{Diagnostic, Diagnostics};

use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
            /// Every leaf in declaration order, so the vocabulary lock is exhaustive by
            /// construction: a leaf added to the macro invocation lands here too.
            #[cfg(test)]
            const ALL: &'static [Self] = &[$(Self::$variant,)*];

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

/// The ordered metadata of one named field in a named variant payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaField {
    /// The field's doc lines, in declaration order.
    pub docs: Vec<String>,
    /// The deprecation note, present exactly when the field is deprecated.
    pub deprecation: Option<String>,
    /// The field's declared identifier.
    pub name: String,
    /// The field's boundary leaf type.
    pub ty: BoundaryLeaf,
}

/// A format-1 error-variant payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaPayload {
    /// A variant with no payload; its wire spelling remains the string `"unit"`.
    Unit,
    /// A variant with one unnamed boundary leaf and its metadata.
    Value {
        /// The payload's doc lines, in declaration order.
        docs: Vec<String>,
        /// The deprecation note, present exactly when the payload is deprecated.
        deprecation: Option<String>,
        /// The payload's boundary leaf type.
        ty: BoundaryLeaf,
    },
    /// A variant with ordered named fields; an empty vector remains distinct from [`Self::Unit`].
    Named(Vec<SchemaField>),
}

/// One variant of a declared error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaVariant {
    /// The variant's declared identifier.
    pub name: String,
    /// The doc lines, in declaration order.
    pub docs: Vec<String>,
    /// The deprecation note, present exactly when the variant is deprecated.
    pub deprecation: Option<String>,
    /// The variant's unit, value, or ordered named-field payload.
    pub payload: SchemaPayload,
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
    /// Key sorting is guaranteed by this code, not by `serde_json::Map`'s backing. Every object —
    /// the document's own, and every object nested anywhere inside the opaque provenance value —
    /// is rebuilt through an explicit `BTreeMap` before serialization, so the emitted bytes are
    /// identical whether or not something in the dependency graph enables
    /// `serde_json/preserve_order`. That claim is checked, not merely asserted: the crate's
    /// `preserve-order` feature builds that configuration and `cargo xtask ci` runs these tests in
    /// it.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(&sorted(self.value()))
            .expect("schema values are serializable");
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
            "payload": self.payload.value(),
        })
    }
}

impl SchemaPayload {
    fn value(&self) -> Value {
        match self {
            Self::Unit => json!("unit"),
            Self::Value {
                docs,
                deprecation: note,
                ty,
            } => json!({
                "deprecation": deprecation(note),
                "docs": docs,
                "kind": "value",
                "type": ty.canonical_name(),
            }),
            Self::Named(fields) => json!({
                "fields": fields.iter().map(SchemaField::value).collect::<Vec<_>>(),
                "kind": "named",
            }),
        }
    }
}

impl SchemaField {
    fn value(&self) -> Value {
        json!({
            "deprecation": deprecation(&self.deprecation),
            "docs": self.docs,
            "name": self.name,
            "type": self.ty.canonical_name(),
        })
    }
}

/// Returns an exposure level's S2 D3 grammar token. Spelling every level is not a claim that a
/// format-1 document may carry every level: the serializer is total over `ExposureLevel`, the
/// emitter builds only `External`, and the strict reader admits only that (BXC0007).
fn exposure_name(level: ExposureLevel) -> &'static str {
    match level {
        ExposureLevel::CodeOnly => "code_only",
        ExposureLevel::Internal => "internal",
        ExposureLevel::External => "external",
    }
}

/// Returns an idempotency property's S2 D3 grammar token, under the same distinction between
/// spelling a value and admitting one (BXC0008).
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

/// Rebuilds one JSON value with every object's entries collected through a `BTreeMap`, so key order
/// is decided here instead of inherited from whichever map `serde_json::Map` happens to wrap.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, nested)| (key, sorted(nested)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        scalar => scalar,
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
                    payload: SchemaPayload::Unit,
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

    fn mixed_payloads(provenance: Value) -> SchemaDocument {
        let mut document = hello(provenance);
        document.box_id = BoxId::new("payloads").unwrap();
        document.capabilities[0].name = CapabilityName::new("inspect").unwrap();
        document.capabilities[0].error = "PayloadError".to_owned();
        document.types[0].name = "PayloadError".to_owned();
        document.types[0].variants = vec![
            SchemaVariant {
                name: "Unit".to_owned(),
                docs: Vec::new(),
                deprecation: None,
                payload: SchemaPayload::Unit,
            },
            SchemaVariant {
                name: "Value".to_owned(),
                docs: vec!["value variant".to_owned()],
                deprecation: None,
                payload: SchemaPayload::Value {
                    docs: vec!["value payload".to_owned()],
                    deprecation: Some("use detail".to_owned()),
                    ty: BoundaryLeaf::U32,
                },
            },
            SchemaVariant {
                name: "Named".to_owned(),
                docs: Vec::new(),
                deprecation: Some("retired".to_owned()),
                payload: SchemaPayload::Named(vec![
                    SchemaField {
                        docs: vec!["message field".to_owned()],
                        deprecation: None,
                        name: "message".to_owned(),
                        ty: BoundaryLeaf::String,
                    },
                    SchemaField {
                        docs: Vec::new(),
                        deprecation: Some("use text".to_owned()),
                        name: "code".to_owned(),
                        ty: BoundaryLeaf::I64,
                    },
                ]),
            },
            SchemaVariant {
                name: "EmptyNamed".to_owned(),
                docs: Vec::new(),
                deprecation: None,
                payload: SchemaPayload::Named(Vec::new()),
            },
        ];
        document
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
            // Provenance is opaque to strictness but not to the encoding. Both levels are authored
            // out of alphabetical order, which is the order `serde_json` emits under
            // `preserve-order` and makes no difference at all under the default `BTreeMap`.
            provenance: Provenance::new(json!({
                "generator": "boxology-generator",
                "environment": {"toolchain": "pinned", "arch": "any"},
            })),
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

    /// The pinned Hello document, byte-identical to `boxology-generator`'s `SCHEMA` literal in
    /// `cold_schema_has_exact_projection_revision_and_document`. It is the cross-crate anchor: the
    /// generator asserts its emitted bytes against that literal, this crate asserts the same
    /// literal against the serializer, and the two together say the codec did not fork.
    const PINNED_HELLO: &[u8] = br#"{
  "box_id": "hello",
  "capabilities": [
    {
      "deprecation": null,
      "docs": [],
      "error": "GreetError",
      "id": "hello.greet",
      "idempotency": "none",
      "input": {
        "name": "name",
        "type": "String"
      },
      "max_exposure": "external",
      "name": "greet",
      "output": {
        "type": "String"
      },
      "shape": "unary"
    }
  ],
  "provenance": {
    "generator": "boxology-generator",
    "generator_version": "0.0.0",
    "semantic_digest": "sha256:545f142b0ced7670e3f9efc7bcaaf3b7a2a0b2b790e5b48acaa85e4901c89b18"
  },
  "revision": "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176",
  "schema_format": 1,
  "types": [
    {
      "deprecation": null,
      "docs": [],
      "kind": "error",
      "name": "GreetError",
      "variants": [
        {
          "deprecation": null,
          "docs": [],
          "name": "EmptyName",
          "payload": "unit"
        }
      ]
    }
  ]
}
"#;

    const PINNED_MIXED: &[u8] = br#"{
  "box_id": "payloads",
  "capabilities": [
    {
      "deprecation": null,
      "docs": [],
      "error": "PayloadError",
      "id": "payloads.inspect",
      "idempotency": "none",
      "input": {
        "name": "name",
        "type": "String"
      },
      "max_exposure": "external",
      "name": "inspect",
      "output": {
        "type": "String"
      },
      "shape": "unary"
    }
  ],
  "provenance": {
    "a": "mixed",
    "z": {
      "a": 2,
      "b": 1
    }
  },
  "revision": "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176",
  "schema_format": 1,
  "types": [
    {
      "deprecation": null,
      "docs": [],
      "kind": "error",
      "name": "PayloadError",
      "variants": [
        {
          "deprecation": null,
          "docs": [],
          "name": "Unit",
          "payload": "unit"
        },
        {
          "deprecation": null,
          "docs": [
            "value variant"
          ],
          "name": "Value",
          "payload": {
            "deprecation": {
              "note": "use detail"
            },
            "docs": [
              "value payload"
            ],
            "kind": "value",
            "type": "u32"
          }
        },
        {
          "deprecation": {
            "note": "retired"
          },
          "docs": [],
          "name": "Named",
          "payload": {
            "fields": [
              {
                "deprecation": null,
                "docs": [
                  "message field"
                ],
                "name": "message",
                "type": "String"
              },
              {
                "deprecation": {
                  "note": "use text"
                },
                "docs": [],
                "name": "code",
                "type": "i64"
              }
            ],
            "kind": "named"
          }
        },
        {
          "deprecation": null,
          "docs": [],
          "name": "EmptyNamed",
          "payload": {
            "fields": [],
            "kind": "named"
          }
        }
      ]
    }
  ]
}
"#;

    #[test]
    fn canonical_bytes_match_the_pinned_hello_document() {
        // Authored in reverse key order: under `preserve-order` that is what `serde_json` would
        // emit, so these sorted bytes are `canonical_bytes`'s doing wherever the orders can differ.
        let provenance = json!({
            "semantic_digest":
                "sha256:545f142b0ced7670e3f9efc7bcaaf3b7a2a0b2b790e5b48acaa85e4901c89b18",
            "generator_version": "0.0.0",
            "generator": "boxology-generator",
        });
        assert_eq!(hello(provenance).canonical_bytes(), PINNED_HELLO);
    }

    #[test]
    fn mixed_payload_bytes_are_pinned_and_round_trip() {
        let document = mixed_payloads(json!({"z": {"b": 1, "a": 2}, "a": "mixed"}));
        let bytes = document.canonical_bytes();
        assert_eq!(bytes, PINNED_MIXED);
        assert_eq!(SchemaDocument::parse(&bytes).unwrap(), document);
        assert_ne!(SchemaPayload::Unit, SchemaPayload::Named(Vec::new()));
    }

    /// Every spelling below is emitted verbatim into documents that other builds of this software
    /// must read, which makes `canonical_name` a wire authority in its own right. A typo in a leaf
    /// no fixture happens to use is invisible until it surfaces as a cross-version
    /// incompatibility, so each of the 13 leaves, the one shape, and both enumerations are locked
    /// exactly. Locking what `ExposureLevel::Internal` *spells* is not a claim that a format-1
    /// document may hold it: no emitter writes it and the reader rejects it (BXC0007, BXC0008).
    #[test]
    fn wire_vocabulary_spellings_are_locked() {
        #[rustfmt::skip]
        let leaves = [
            "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64",
            "f32", "f64", "String", "Blob",
        ];
        let spelled = BoundaryLeaf::ALL
            .iter()
            .map(|leaf| leaf.canonical_name())
            .collect::<Vec<_>>();
        assert_eq!(spelled, leaves);
        assert_eq!(Shape::Unary.canonical_name(), "unary");
        assert_eq!(exposure_name(ExposureLevel::CodeOnly), "code_only");
        assert_eq!(exposure_name(ExposureLevel::Internal), "internal");
        assert_eq!(exposure_name(ExposureLevel::External), "external");
        assert_eq!(idempotency_name(Idempotency::None), "none");
        assert_eq!(idempotency_name(Idempotency::Inherent), "inherent");
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
            "provenance", "environment", "arch", "toolchain", "generator",
            "revision", "schema_format", "types",
            "deprecation", "docs", "kind", "name", "variants",
            "deprecation", "docs", "name", "payload",
            "deprecation", "docs", "name", "payload",
        ];
        #[rustfmt::skip]
        let names = ["value", "put", "key", "get", "StoreError", "Missing", "Denied"];
        assert_eq!(keys(&text), expected_keys);
        assert_eq!(values(&text, "name"), names);
    }

    /// Guards the guard: the byte tests above only check sorting while the feature really reaches
    /// `serde_json`, and if that wiring broke the `preserve-order` CI step would pass vacuously.
    #[cfg(feature = "preserve-order")]
    #[test]
    fn the_preserve_order_build_really_holds_unsorted_objects() {
        let Value::Object(map) = json!({"b": 1, "a": 2}) else {
            panic!("object");
        };
        assert_eq!(map.keys().collect::<Vec<_>>(), ["b", "a"]);
    }

    #[test]
    fn public_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(BoundaryLeaf, Shape, Provenance)>();
        bounds::<(InputSlot, OutputSlot, SchemaCapability)>();
        bounds::<(SchemaVariant, SchemaType, SchemaDocument)>();
    }
}
