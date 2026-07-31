//! The coded rejection vocabulary and strict reader for format-1 schema documents.

use crate::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SCHEMA_FORMAT, SchemaCapability,
    SchemaDocument, SchemaField, SchemaPayload, SchemaType, SchemaVariant, Shape,
};
use boxology_contract::{
    BoxId, CapabilityName, ExposureLevel, Idempotency, canonicalize_ordinary_rust_identifier,
};
use serde_json::{Map, Value};
use std::{collections::BTreeSet, fmt};

/// A stable `BXC####` diagnostic code, or one of the frozen texts a code renders.
type Code = &'static str;

/// Where each rule is written down: the narrowing gates are the strict reader's own, so S4's; the
/// identity namespaces, the contract grammar, and the revision spelling are S2's.
const READER: Code = "specs/s4-contract-change-classification.md D1";
const CLASSIFICATION: Code = "specs/s4-contract-change-classification.md D2";
const FINGERPRINT: Code = "specs/s2-contract-generator.md D6";
const IDENTITY: Code = "specs/s2-contract-generator.md D4";
const GRAMMAR: Code = "specs/s2-contract-generator.md D3";
const INTEGRITY: Code = "specs/s4-contract-change-classification.md D6";

/// Where in a schema document a diagnostic points: a JSON-pointer-style path such as
/// `/capabilities/0/shape`, and the empty pointer for the document itself. `serde_json` records no
/// byte spans, so `boxology_manifest`'s line-and-column model has nothing to read from, and a
/// structural pointer also stays correct under any reformatting of the same document.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Location(String);

impl Location {
    /// The document itself.
    pub(crate) fn root() -> Self {
        Self(String::new())
    }

    /// Extends the pointer with one object key.
    ///
    /// This is the only path by which text that did not originate in this crate can reach a
    /// rendered diagnostic, and it admits a key only when the key is plain, bounded identifier
    /// text; every other key locates as `/?`, which no admitted key can spell. So JSON pointer's
    /// `~` and `/` escapes are never needed, and no document can place a quote, a line break, or a
    /// terminal escape sequence into a report.
    pub(crate) fn key(&self, name: &str) -> Self {
        let plain = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
        match !name.is_empty() && name.len() <= 64 && name.bytes().all(plain) {
            true => Self(format!("{}/{name}", self.0)),
            false => Self(format!("{}/?", self.0)),
        }
    }

    /// Extends the pointer with one array index.
    pub(crate) fn index(&self, at: usize) -> Self {
        Self(format!("{}/{at}", self.0))
    }
}

/// One stable coded rejection of a schema document.
///
/// Payload safety is structural, not reviewed. A diagnostic stores only its code and its location:
/// rule and attribution are *derived* from the code through one table, so they cannot drift from
/// it, and the location is built from static key names, array indices, and the single gated helper
/// above. There is no field a document's own text can reach.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Diagnostic {
    location: Location,
    code: Code,
}

impl Diagnostic {
    /// The rejection a code reports at a location.
    pub(crate) fn at(code: Code, location: Location) -> Self {
        Self { location, code }
    }

    /// Returns the stable `BXC####` code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the JSON-pointer-style location of the offending value.
    pub fn location(&self) -> &str {
        &self.location.0
    }

    /// Returns the violated rule.
    pub fn rule(&self) -> &'static str {
        rule_of(self.code)
    }

    /// Returns the normative source of the rule.
    pub fn rule_source(&self) -> &'static str {
        source_of(self.code)
    }

    /// Constructs the D2 error for a classification call with neither document present.
    pub fn classification_requires_document() -> Self {
        Self::at("BXC0024", Location::root())
    }

    /// Constructs the D2 error for a classification call whose documents have different box ids.
    pub fn box_id_mismatch() -> Self {
        Self::at("BXC0025", Location::root().key("box_id"))
    }

    /// Constructs the D6 integrity error for findings under equal document revisions.
    pub fn integrity_findings_under_equal_revisions() -> Self {
        Self::at("BXC0037", Location::root())
    }

    /// Constructs the D6 integrity error for silence under differing document revisions.
    pub fn integrity_silence_under_differing_revisions() -> Self {
        Self::at("BXC0038", Location::root())
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at={:?} rule={:?} source={:?}",
            self.code,
            self.location.0,
            self.rule(),
            self.rule_source()
        )
    }
}

/// A nonempty, deterministically sorted diagnostic collection.
///
/// S4's one rejection vocabulary: the strict reader returns it and so does `classify`. It is
/// neither `Clone` nor `std::error::Error` yet, so a consumer cannot `?` it into a boxed error;
/// the intent is that the classifier's slice adds `Error` once a caller needs it.
#[derive(Debug, Eq, PartialEq)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    /// Sorts accumulated diagnostics into report order; returns `None` when there are none.
    ///
    /// Report order is location then code, bytewise over the pointer: a stable total order and
    /// nothing more. It is **not** document order, since `/x/10` sorts before `/x/2`, reachable in
    /// any document with ten capabilities, variants, or doc lines. Zero-padding would order them
    /// and stop the pointers being pointers, so the suite pins the real order instead.
    pub fn new(mut diagnostics: Vec<Diagnostic>) -> Option<Self> {
        diagnostics.sort();
        (!diagnostics.is_empty()).then_some(Self(diagnostics))
    }

    /// Consumes the collection into its sorted diagnostics, which a consumer moves into a report of
    /// its own. `Diagnostic` is deliberately not `Clone`: one diagnostic has one owner.
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.0
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<String> = self.0.iter().map(Diagnostic::to_string).collect();
        formatter.write_str(&lines.join("\n"))
    }
}

const ROOT_KEYS: &[&str] = &[
    "schema_format",
    "box_id",
    "revision",
    "provenance",
    "capabilities",
    "types",
];
const CAPABILITY_KEYS: &[&str] = &[
    "name",
    "id",
    "docs",
    "deprecation",
    "error",
    "input",
    "output",
    "shape",
    "max_exposure",
    "idempotency",
];
const INPUT_KEYS: &[&str] = &["name", "type"];
const OUTPUT_KEYS: &[&str] = &["type"];
const TYPE_KEYS: &[&str] = &["kind", "name", "docs", "deprecation", "variants"];
const VARIANT_KEYS: &[&str] = &["name", "docs", "deprecation", "payload"];
const DEPRECATION_KEYS: &[&str] = &["note"];
const VALUE_PAYLOAD_KEYS: &[&str] = &["deprecation", "docs", "kind", "type"];
const NAMED_PAYLOAD_KEYS: &[&str] = &["fields", "kind"];
const PAYLOAD_KEYS: &[&str] = &["deprecation", "docs", "fields", "kind", "type"];
const FIELD_KEYS: &[&str] = &["deprecation", "docs", "name", "type"];

#[rustfmt::skip]
impl SchemaDocument {
    /// Parses one complete, strict format-1 schema document.
    pub fn parse(bytes: &[u8]) -> Result<Self, Diagnostics> {
        let text = std::str::from_utf8(bytes).map_err(|_| single("BXC0001", Location::root()))?;
        let value = serde_json::from_str::<Value>(text).map_err(|_| single("BXC0002", Location::root()))?;
        let Value::Object(root) = value else {
            return Err(single("BXC0005", Location::root()));
        };
        let Some(format) = root.get("schema_format") else {
            return Err(single("BXC0004", Location::root()));
        };
        let Some(format) = format.as_u64() else {
            return Err(single("BXC0005", Location::root().key("schema_format")));
        };
        if format != SCHEMA_FORMAT {
            return Err(single("BXC0006", Location::root().key("schema_format")));
        }
        let mut reader = Reader::default();
        let at = Location::root();
        reader.unknown(&root, ROOT_KEYS, &at);
        let box_id = reader.box_id(&root, &at);
        let revision = reader.revision(&root, &at);
        let provenance = reader.field(&root, "provenance", &at).cloned().map(Provenance::new);
        let types = reader.types(&root, &at);
        let type_names = types.as_ref().and_then(|parsed| parsed.names.as_deref());
        let capabilities = reader.capabilities(&root, &at, box_id.as_ref(), type_names);
        if let Some(diagnostics) = Diagnostics::new(reader.diagnostics) {
            return Err(diagnostics);
        }
        let (Some(box_id), Some(revision), Some(provenance), Some(capabilities), Some(types)) = (box_id, revision, provenance, capabilities, types) else {
            unreachable!("required schema fields report their own errors")
        };
        Ok(Self {
            box_id,
            capabilities,
            provenance,
            revision,
            types: types.values,
        })
    }
}

#[rustfmt::skip]
#[derive(Default)]
struct Reader { diagnostics: Vec<Diagnostic> }
#[rustfmt::skip]
struct ParsedTypes { values: Vec<SchemaType>, names: Option<Vec<String>> }

#[rustfmt::skip]
impl Reader {
    fn push(&mut self, code: Code, location: Location) { self.diagnostics.push(Diagnostic::at(code, location)); }
    fn unknown(&mut self, object: &Map<String, Value>, known: &[&str], at: &Location) {
        for key in object.keys().filter(|key| !known.contains(&key.as_str())) {
            self.push("BXC0003", at.key(key));
        }
    }
    fn field<'a>(&mut self, object: &'a Map<String, Value>, key: &str, at: &Location) -> Option<&'a Value> {
        object.get(key).or_else(|| { self.push("BXC0004", at.clone()); None })
    }
    fn typed<'a, T, F: FnOnce(&'a Value) -> Option<T>>(&mut self, object: &'a Map<String, Value>, key: &str, at: &Location, parse: F) -> Option<T> {
        let value = self.field(object, key, at)?;
        parse(value).or_else(|| { self.push("BXC0005", at.key(key)); None })
    }
    fn string_field<'a>(&mut self, object: &'a Map<String, Value>, key: &str, at: &Location) -> Option<&'a str> { self.typed(object, key, at, Value::as_str) }
    fn array_field<'a>(&mut self, object: &'a Map<String, Value>, key: &str, at: &Location) -> Option<&'a Vec<Value>> { self.typed(object, key, at, Value::as_array) }
    fn object_field<'a>(&mut self, object: &'a Map<String, Value>, key: &str, at: &Location) -> Option<&'a Map<String, Value>> { self.typed(object, key, at, Value::as_object) }
    fn object_value<'a>(&mut self, value: &'a Value, at: Location) -> Option<&'a Map<String, Value>> { value.as_object().or_else(|| { self.push("BXC0005", at); None }) }
    fn check<T>(&mut self, value: Option<T>, code: Code, location: Location) -> Option<T> { if value.is_none() { self.push(code, location); } value }
    fn box_id(&mut self, root: &Map<String, Value>, at: &Location) -> Option<BoxId> {
        self.string_field(root, "box_id", at).and_then(|value| self.check(BoxId::new(value.to_owned()).ok(), "BXC0010", at.key("box_id")))
    }
    fn revision(&mut self, root: &Map<String, Value>, at: &Location) -> Option<String> {
        self.string_field(root, "revision", at).and_then(|value| self.check(valid_revision(value).then(|| value.to_owned()), "BXC0009", at.key("revision")))
    }
    fn capability_name(&mut self, object: &Map<String, Value>, at: &Location) -> Option<CapabilityName> {
        self.string_field(object, "name", at).and_then(|value| self.check(CapabilityName::new(value.to_owned()).ok(), "BXC0011", at.key("name")))
    }
    fn identifier(&mut self, object: &Map<String, Value>, key: &str, code: Code, at: &Location) -> Option<String> {
        self.string_field(object, key, at).and_then(|value| self.check(canonicalize_ordinary_rust_identifier(value), code, at.key(key)))
    }
    fn exact<T>(&mut self, object: &Map<String, Value>, key: &str, expected: &str, code: Code, at: &Location, valid: T) -> Option<T> {
        self.string_field(object, key, at).and_then(|value| self.check((value == expected).then_some(valid), code, at.key(key)))
    }
    fn docs(&mut self, object: &Map<String, Value>, key: &str, at: &Location) -> Option<Vec<String>> {
        let values = self.array_field(object, key, at)?;
        let mut valid = true;
        let docs = values.iter().enumerate().filter_map(|(index, value)| match value {
            Value::String(value) => Some(value.clone()),
            _ => { valid = false; self.push("BXC0005", at.key(key).index(index)); None }
        }).collect();
        valid.then_some(docs)
    }
    fn deprecation(&mut self, object: &Map<String, Value>, key: &str, at: &Location) -> Option<Option<String>> {
        let value = self.field(object, key, at)?;
        let deprecation_at = at.key(key);
        match value {
            Value::Null => Some(None),
            Value::Object(value) => {
                self.unknown(value, DEPRECATION_KEYS, &deprecation_at);
                self.string_field(value, "note", &deprecation_at).map(|note| Some(note.to_owned()))
            }
            _ => { self.push("BXC0005", deprecation_at); None }
        }
    }
    fn types(&mut self, root: &Map<String, Value>, at: &Location) -> Option<ParsedTypes> {
        let values = self.array_field(root, "types", at)?;
        let types_at = at.key("types");
        let mut types = Vec::new();
        let mut names = Vec::new();
        let mut seen = BTreeSet::new();
        let mut names_complete = true;
        for (index, value) in values.iter().enumerate() {
            let item_at = types_at.index(index);
            let Some(value) = self.object_value(value, item_at.clone()) else {
                names_complete = false;
                continue;
            };
            self.unknown(value, TYPE_KEYS, &item_at);
            let name = self.identifier(value, "name", "BXC0013", &item_at);
            if let Some(name) = name.as_ref() {
                if !seen.insert(name.clone()) {
                    self.push("BXC0017", item_at.key("name"));
                }
                names.push(name.clone());
            } else {
                names_complete = false;
            }
            let kind = self.exact(value, "kind", "error", "BXC0021", &item_at, ());
            let docs = self.docs(value, "docs", &item_at);
            let deprecation = self.deprecation(value, "deprecation", &item_at);
            let variants = self.variants(value, &item_at);
            if let (Some(name), Some(docs), Some(deprecation), Some(variants), Some(())) = (name, docs, deprecation, variants, kind) {
                types.push(SchemaType {
                    name,
                    docs,
                    deprecation,
                    variants,
                });
            }
        }
        Some(ParsedTypes { values: types, names: names_complete.then_some(names) })
    }
    fn variants(&mut self, object: &Map<String, Value>, at: &Location) -> Option<Vec<SchemaVariant>> {
        let values = self.array_field(object, "variants", at)?;
        let variants_at = at.key("variants");
        let mut variants = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, value) in values.iter().enumerate() {
            let item_at = variants_at.index(index);
            let Some(value) = self.object_value(value, item_at.clone()) else {
                continue;
            };
            self.unknown(value, VARIANT_KEYS, &item_at);
            let name = self.identifier(value, "name", "BXC0014", &item_at).and_then(|name| self.check((name != "Unknown").then_some(name), "BXC0014", item_at.key("name")));
            if let Some(name) = name.as_ref() && !seen.insert(name.clone()) {
                self.push("BXC0018", item_at.key("name"));
            }
            let docs = self.docs(value, "docs", &item_at);
            let deprecation = self.deprecation(value, "deprecation", &item_at);
            let payload = self.payload(value, &item_at);
            if let (Some(name), Some(docs), Some(deprecation), Some(payload)) =
                (name, docs, deprecation, payload)
            {
                variants.push(SchemaVariant {
                    name,
                    docs,
                    deprecation,
                    payload,
                });
            }
        }
        Some(variants)
    }
    fn payload(&mut self, object: &Map<String, Value>, at: &Location) -> Option<SchemaPayload> {
        let value = self.field(object, "payload", at)?;
        let payload_at = at.key("payload");
        match value {
            Value::String(value) if value == "unit" => Some(SchemaPayload::Unit),
            Value::Object(value) => self.payload_object(value, &payload_at),
            _ => {
                self.push("BXC0022", payload_at);
                None
            }
        }
    }
    fn payload_object(
        &mut self,
        object: &Map<String, Value>,
        at: &Location,
    ) -> Option<SchemaPayload> {
        match self.string_field(object, "kind", at) {
            Some("value") => {
                self.unknown(object, VALUE_PAYLOAD_KEYS, at);
                let docs = self.docs(object, "docs", at);
                let deprecation = self.deprecation(object, "deprecation", at);
                let ty = self.leaf(object, at);
                match (docs, deprecation, ty) {
                    (Some(docs), Some(deprecation), Some(ty)) => {
                        Some(SchemaPayload::Value { docs, deprecation, ty })
                    }
                    _ => None,
                }
            }
            Some("named") => {
                self.unknown(object, NAMED_PAYLOAD_KEYS, at);
                self.named_fields(object, at).map(SchemaPayload::Named)
            }
            Some(_) => {
                self.unknown(object, PAYLOAD_KEYS, at);
                self.push("BXC0022", at.key("kind"));
                None
            }
            None => None,
        }
    }
    fn named_fields(
        &mut self,
        object: &Map<String, Value>,
        at: &Location,
    ) -> Option<Vec<SchemaField>> {
        let values = self.array_field(object, "fields", at)?;
        let fields_at = at.key("fields");
        let mut fields = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, value) in values.iter().enumerate() {
            let item_at = fields_at.index(index);
            let Some(value) = self.object_value(value, item_at.clone()) else {
                continue;
            };
            self.unknown(value, FIELD_KEYS, &item_at);
            let name = self.identifier(value, "name", "BXC0029", &item_at);
            if let Some(name) = name.as_ref() && !seen.insert(name.clone()) {
                self.push("BXC0030", item_at.key("name"));
            }
            let docs = self.docs(value, "docs", &item_at);
            let deprecation = self.deprecation(value, "deprecation", &item_at);
            let ty = self.leaf(value, &item_at);
            if let (Some(name), Some(docs), Some(deprecation), Some(ty)) =
                (name, docs, deprecation, ty)
            {
                fields.push(SchemaField {
                    docs,
                    deprecation,
                    name,
                    ty,
                });
            }
        }
        Some(fields)
    }
    fn capabilities(&mut self, root: &Map<String, Value>, at: &Location, box_id: Option<&BoxId>, type_names: Option<&[String]>) -> Option<Vec<SchemaCapability>> {
        let values = self.array_field(root, "capabilities", at)?;
        let capabilities_at = at.key("capabilities");
        let mut capabilities = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, value) in values.iter().enumerate() {
            let item_at = capabilities_at.index(index);
            let Some(value) = self.object_value(value, item_at.clone()) else {
                continue;
            };
            self.unknown(value, CAPABILITY_KEYS, &item_at);
            let name = self.capability_name(value, &item_at);
            if name.as_ref().is_some_and(|name| !seen.insert(name.as_str().to_owned())) {
                self.push("BXC0016", item_at.key("name"));
            }
            let id = self.string_field(value, "id", &item_at).map(str::to_owned);
            if let (Some(box_id), Some(name), Some(id)) = (box_id, name.as_ref(), id.as_deref()) && id != format!("{box_id}.{}", name.as_str()) {
                self.push("BXC0012", item_at.key("id"));
            }
            let docs = self.docs(value, "docs", &item_at);
            let deprecation = self.deprecation(value, "deprecation", &item_at);
            let error = self.string_field(value, "error", &item_at).and_then(|error| {
                let error = canonicalize_ordinary_rust_identifier(error);
                match type_names {
                    Some(names) => self.check(error.filter(|error| names.contains(error)), "BXC0023", item_at.key("error")),
                    None => error,
                }
            });
            let input = self.input(value, &item_at);
            let output = self.output(value, &item_at);
            let shape = self.shape(value, &item_at);
            let max_exposure = self.max_exposure(value, &item_at);
            let idempotency = self.idempotency(value, &item_at);
            if let (Some(name), Some(docs), Some(deprecation), Some(error), Some(input), Some(output), Some(shape), Some(max_exposure), Some(idempotency)) = (name, docs, deprecation, error, input, output, shape, max_exposure, idempotency) {
                capabilities.push(SchemaCapability {
                    name,
                    docs,
                    deprecation,
                    error,
                    input,
                    output,
                    shape,
                    max_exposure,
                    idempotency,
                });
            }
        }
        Some(capabilities)
    }
    fn input(&mut self, object: &Map<String, Value>, at: &Location) -> Option<InputSlot> {
        let input_at = at.key("input");
        let value = self.object_field(object, "input", at)?;
        self.unknown(value, INPUT_KEYS, &input_at);
        let name = self.identifier(value, "name", "BXC0015", &input_at);
        let leaf = self.leaf(value, &input_at);
        name.zip(leaf).map(|(name, leaf)| InputSlot { name, leaf })
    }
    fn output(&mut self, object: &Map<String, Value>, at: &Location) -> Option<OutputSlot> {
        let output_at = at.key("output");
        let value = self.object_field(object, "output", at)?;
        self.unknown(value, OUTPUT_KEYS, &output_at);
        self.leaf(value, &output_at).map(|leaf| OutputSlot { leaf })
    }
    fn leaf(&mut self, object: &Map<String, Value>, at: &Location) -> Option<BoundaryLeaf> {
        let value = self.string_field(object, "type", at).map(str::to_owned)?;
        let leaf = match value.as_str() {
            "bool" => Some(BoundaryLeaf::Bool),
            "u8" => Some(BoundaryLeaf::U8),
            "u16" => Some(BoundaryLeaf::U16),
            "u32" => Some(BoundaryLeaf::U32),
            "u64" => Some(BoundaryLeaf::U64),
            "i8" => Some(BoundaryLeaf::I8),
            "i16" => Some(BoundaryLeaf::I16),
            "i32" => Some(BoundaryLeaf::I32),
            "i64" => Some(BoundaryLeaf::I64),
            "f32" => Some(BoundaryLeaf::F32),
            "f64" => Some(BoundaryLeaf::F64),
            "String" => Some(BoundaryLeaf::String),
            "Blob" => Some(BoundaryLeaf::Blob),
            _ => None,
        };
        self.check(leaf, "BXC0019", at.key("type"))
    }
    fn shape(&mut self, object: &Map<String, Value>, at: &Location) -> Option<Shape> {
        self.exact(object, "shape", "unary", "BXC0020", at, Shape::Unary)
    }
    fn max_exposure(&mut self, object: &Map<String, Value>, at: &Location) -> Option<ExposureLevel> {
        self.exact(object, "max_exposure", "external", "BXC0007", at, ExposureLevel::External)
    }
    fn idempotency(&mut self, object: &Map<String, Value>, at: &Location) -> Option<Idempotency> {
        self.exact(object, "idempotency", "none", "BXC0008", at, Idempotency::None)
    }
}

#[rustfmt::skip]
fn valid_revision(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else { return false };
    hex.len() == 64 && hex.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn single(code: Code, location: Location) -> Diagnostics {
    Diagnostics::new(vec![Diagnostic::at(code, location)]).expect("one diagnostic is nonempty")
}

/// The frozen rule text of every code. The unreachable fallback is coded text, not a panic.
fn rule_of(code: Code) -> Code {
    match code {
        "BXC0001" => "a schema document must be valid UTF-8",
        "BXC0002" => "a schema document must be well-formed JSON",
        "BXC0003" => "format 1 rejects unknown schema keys",
        "BXC0004" => "a required schema key must be present",
        "BXC0005" => "a schema value must hold its declared JSON type",
        "BXC0006" => "this reader supports schema format 1 and rejects unknown formats",
        "BXC0007" => "format 1's only exposure is external",
        "BXC0008" => "format 1's only idempotency is none",
        "BXC0009" => "a revision must be sha256: and 64 lowercase hexadecimal digits",
        "BXC0010" => "the box id must match [a-z][a-z0-9-]*",
        "BXC0011" => "a capability name must match [a-z][a-z0-9_]*",
        "BXC0012" => "a capability id must be its box id and its name joined by a period",
        "BXC0013" => "a type name must be an ordinary non-raw Rust identifier",
        "BXC0014" => "a variant name must be an ordinary non-raw Rust identifier",
        "BXC0015" => "an input parameter name must be an ordinary non-raw Rust identifier",
        "BXC0016" => "capability names must be unique",
        "BXC0017" => "type names must be unique",
        "BXC0018" => "variant names must be unique within their type",
        "BXC0019" => "a boundary type must be one of format 1's canonical leaves",
        "BXC0020" => "format 1's only capability shape is unary",
        "BXC0021" => "format 1 declares error types only",
        "BXC0022" => "format 1's variant payload must be unit, value, or named",
        "BXC0023" => "a capability error must name a declared type",
        "BXC0029" => "a named payload field name must be an ordinary non-raw Rust identifier",
        "BXC0030" => "named payload field names must be unique",
        "BXC0024" => "classification requires a base or a submitted document",
        "BXC0025" => "base and submitted must declare the same box id",
        "BXC0037" => {
            "findings under equal revisions mean the projection and the classifier disagree"
        }
        "BXC0038" => {
            "differing revisions with no finding mean the projection and the classifier disagree"
        }
        _ => "a schema document must satisfy format 1",
    }
}

/// BXC0001-BXC0008 are the reader's own, including the two narrowings that reject values S2 D3's
/// *contract* grammar lists as legal: the one emitter provably cannot write them, so citing D3
/// there would point a reader at text saying the opposite. BXC0009 is D6's fingerprint spelling,
/// BXC0010-BXC0014 D4's identity namespaces, BXC0015-BXC0023 D3's grammar — where the uniqueness
/// rules are actually written (D4 states none) and the only text reaching an input parameter name.
/// BXC0024-BXC0025 are D2's classifier pairing errors; the classifier owns their reachability.
/// BXC0029 is S2 D4's named-field identity rule and BXC0030 is S2 D3's named-field uniqueness
/// rule. BXC0037-BXC0038 are D6's integrity cross-checks; the classifier owns their reachability.
/// BXC0026-BXC0028 and BXC0031-BXC0036 are reserved for classifier findings and have no
/// rule-text arms here.
fn source_of(code: Code) -> Code {
    match code {
        "BXC0024" | "BXC0025" => CLASSIFICATION,
        "BXC0037" | "BXC0038" => INTEGRITY,
        "BXC0009" => FINGERPRINT,
        "BXC0029" => IDENTITY,
        "BXC0030" => GRAMMAR,
        _ if ("BXC0010"..="BXC0014").contains(&code) => IDENTITY,
        _ if ("BXC0015"..="BXC0023").contains(&code) => GRAMMAR,
        _ => READER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BASE_REVISION: &str =
        "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";

    fn baseline() -> Value {
        json!({
            "schema_format": 1,
            "box_id": "hello",
            "revision": BASE_REVISION,
            "provenance": {"generator": "test", "opaque": [null, true, 7]},
            "capabilities": [{
                "name": "greet",
                "id": "hello.greet",
                "docs": ["greet docs"],
                "deprecation": null,
                "error": "GreetError",
                "input": {"name": "name", "type": "String"},
                "output": {"type": "String"},
                "shape": "unary",
                "max_exposure": "external",
                "idempotency": "none"
            }],
            "types": [{
                "kind": "error",
                "name": "GreetError",
                "docs": ["error docs"],
                "deprecation": null,
                "variants": [{
                    "name": "EmptyName",
                    "docs": ["empty docs"],
                    "deprecation": null,
                    "payload": "unit"
                }]
            }]
        })
    }

    fn bytes(value: &Value) -> Vec<u8> {
        serde_json::to_vec(value).expect("test JSON is serializable")
    }

    fn assert_one(value: Value, code: Code, location: &str) {
        let diagnostics = SchemaDocument::parse(&bytes(&value)).expect_err("invalid test document");
        assert_eq!(
            diagnostics.to_string(),
            Diagnostic::at(code, Location(location.to_owned())).to_string()
        );
        assert_eq!(diagnostics.into_vec().len(), 1);
    }

    fn value_payload(value: &mut Value) -> &mut Value {
        let payload = &mut value["types"][0]["variants"][0]["payload"];
        *payload = json!({"deprecation": null, "docs": [], "kind": "value", "type": "String"});
        payload
    }

    fn named_field(value: &mut Value) -> &mut Value {
        let payload = &mut value["types"][0]["variants"][0]["payload"];
        *payload = json!({"fields": [{"deprecation": null, "docs": [], "name": "field", "type": "String"}], "kind": "named"});
        &mut payload["fields"][0]
    }

    const PAYLOAD: &str = "/types/0/variants/0/payload";
    const FIELD: &str = "/types/0/variants/0/payload/fields/0";

    fn assert_one_bytes(value: &[u8], code: Code, location: &str) {
        let diagnostics = SchemaDocument::parse(value).expect_err("invalid test bytes");
        assert_eq!(
            diagnostics.to_string(),
            Diagnostic::at(code, Location(location.to_owned())).to_string()
        );
        assert_eq!(diagnostics.into_vec().len(), 1);
    }

    /// Every code this crate emits, ascending; the golden below is driven from it. Its reachability
    /// half — `corpus_covers_every_code`, one minimal document provoking each code — cannot exist
    /// until something can parse a document, so it lands with the reader, and **that slice may not
    /// merge without it**.
    #[rustfmt::skip]
    const ALL_CODES: &[Code] = &[
        "BXC0001", "BXC0002", "BXC0003", "BXC0004", "BXC0005", "BXC0006", "BXC0007", "BXC0008",
        "BXC0009", "BXC0010", "BXC0011", "BXC0012", "BXC0013", "BXC0014", "BXC0015", "BXC0016",
        "BXC0017", "BXC0018", "BXC0019", "BXC0020", "BXC0021", "BXC0022", "BXC0023", "BXC0024",
        "BXC0025", "BXC0029", "BXC0030", "BXC0037", "BXC0038",
    ];

    /// Codes this reader can emit from document bytes. The pairing constructors BXC0024–BXC0025
    /// and the integrity constructors BXC0037–BXC0038 are classifier inputs, so their reachability
    /// is proved by the classifier surface lock.
    const READER_CODES: &[Code] = &[
        "BXC0001", "BXC0002", "BXC0003", "BXC0004", "BXC0005", "BXC0006", "BXC0007", "BXC0008",
        "BXC0009", "BXC0010", "BXC0011", "BXC0012", "BXC0013", "BXC0014", "BXC0015", "BXC0016",
        "BXC0017", "BXC0018", "BXC0019", "BXC0020", "BXC0021", "BXC0022", "BXC0023", "BXC0029",
        "BXC0030",
    ];

    /// Codes emitted by the classifier rather than this reader. They occupy the allocated range
    /// without duplicating the classifier's finding text or pretending a reader corpus can reach
    /// them.
    const CLASSIFIER_RESERVED_CODES: &[Code] = &[
        "BXC0026", "BXC0027", "BXC0028", "BXC0031", "BXC0032", "BXC0033", "BXC0034", "BXC0035",
        "BXC0036",
    ];

    /// The unregistered code that probes the table's fallback, and the only quoted `BXC` literal in
    /// this crate that is not a registered code. The scan below subtracts exactly it.
    const FALLBACK_PROBE: Code = "BXC9999";

    /// The rendered wording of every code, byte for byte, as `<code> <rule> <source>` per line in
    /// `ALL_CODES` order. Nothing else pins either, so a rewording or a repointing is a diff here.
    const EXPECTED: &str = "\
BXC0001 a schema document must be valid UTF-8 specs/s4-contract-change-classification.md D1
BXC0002 a schema document must be well-formed JSON specs/s4-contract-change-classification.md D1
BXC0003 format 1 rejects unknown schema keys specs/s4-contract-change-classification.md D1
BXC0004 a required schema key must be present specs/s4-contract-change-classification.md D1
BXC0005 a schema value must hold its declared JSON type specs/s4-contract-change-classification.md D1
BXC0006 this reader supports schema format 1 and rejects unknown formats specs/s4-contract-change-classification.md D1
BXC0007 format 1's only exposure is external specs/s4-contract-change-classification.md D1
BXC0008 format 1's only idempotency is none specs/s4-contract-change-classification.md D1
BXC0009 a revision must be sha256: and 64 lowercase hexadecimal digits specs/s2-contract-generator.md D6
BXC0010 the box id must match [a-z][a-z0-9-]* specs/s2-contract-generator.md D4
BXC0011 a capability name must match [a-z][a-z0-9_]* specs/s2-contract-generator.md D4
BXC0012 a capability id must be its box id and its name joined by a period specs/s2-contract-generator.md D4
BXC0013 a type name must be an ordinary non-raw Rust identifier specs/s2-contract-generator.md D4
BXC0014 a variant name must be an ordinary non-raw Rust identifier specs/s2-contract-generator.md D4
BXC0015 an input parameter name must be an ordinary non-raw Rust identifier specs/s2-contract-generator.md D3
BXC0016 capability names must be unique specs/s2-contract-generator.md D3
BXC0017 type names must be unique specs/s2-contract-generator.md D3
BXC0018 variant names must be unique within their type specs/s2-contract-generator.md D3
BXC0019 a boundary type must be one of format 1's canonical leaves specs/s2-contract-generator.md D3
BXC0020 format 1's only capability shape is unary specs/s2-contract-generator.md D3
BXC0021 format 1 declares error types only specs/s2-contract-generator.md D3
BXC0022 format 1's variant payload must be unit, value, or named specs/s2-contract-generator.md D3
BXC0023 a capability error must name a declared type specs/s2-contract-generator.md D3
BXC0024 classification requires a base or a submitted document specs/s4-contract-change-classification.md D2
BXC0025 base and submitted must declare the same box id specs/s4-contract-change-classification.md D2
BXC0029 a named payload field name must be an ordinary non-raw Rust identifier specs/s2-contract-generator.md D4
BXC0030 named payload field names must be unique specs/s2-contract-generator.md D3
BXC0037 findings under equal revisions mean the projection and the classifier disagree specs/s4-contract-change-classification.md D6
BXC0038 differing revisions with no finding mean the projection and the classifier disagree specs/s4-contract-change-classification.md D6
";

    #[test]
    fn all_codes_is_exhaustive() {
        // Every source file's whole text, at compile time. There is deliberately no cut at the test
        // module: a cut is where such a scan narrows *silently*, and three variants of that defect
        // have now been found in this project — an unguarded cut, a second source file, and
        // production code appended *below* the test module, which every before-the-marker cut
        // misses by construction. Whole files minus one named literal has no such place. It does
        // let a registered code spelled only in a test pass; `rule_text_and_sources_are_locked`
        // closes that direction, since a code with no `rule_of` arm renders the fallback there.
        //
        // The module list is compared, not assumed, and every `mod ` counts, so `pub mod` and an
        // indented declaration cannot slip past — nor can a comment or a literal that spells
        // `mod `, which trips this loudly. That false positive is the accepted price. The needle is
        // assembled rather than written, so this scan does not match its own source.
        let root = include_str!("lib.rs");
        let named = |(at, _): (usize, &str)| root[at + 4..].split([';', ' ']).next().unwrap_or("");
        let declared: Vec<&str> = root.match_indices("mod ").map(named).collect();
        assert_eq!(
            declared,
            ["read", "tests"],
            "the root's `mod ` text changed"
        );
        let needle = format!("{}BXC", '"');
        let mut seen: Vec<&str> = Vec::new();
        for source in [root, include_str!("read.rs")] {
            for (at, _) in source.match_indices(needle.as_str()) {
                let code = source.get(at + 1..at + 8).unwrap_or_default();
                if code != FALLBACK_PROBE && !seen.contains(&code) {
                    seen.push(code);
                }
            }
        }
        seen.sort_unstable();
        let mut allocated = ALL_CODES
            .iter()
            .chain(CLASSIFIER_RESERVED_CODES)
            .copied()
            .collect::<Vec<_>>();
        allocated.sort_unstable();
        assert_eq!(seen, allocated);
        let read = include_str!("read.rs");
        let source_of = read.split_once("fn source_of").unwrap().1;
        let source_of = source_of.split_once("#[cfg(test)]").unwrap().0;
        let anchors = source_of
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|token| token.starts_with(&needle[1..]));
        let (all, unique) = (
            anchors.clone().count(),
            anchors.collect::<BTreeSet<_>>().len(),
        );
        assert_eq!((all, unique), (11, 11), "source_of anchors must be unique");
        // Dense from BXC0001, with the classifier's reserved range represented explicitly rather
        // than assigned reader rule text. An ascending reader-only list would miss this gap.
        let spell = |n| format!("BX{}{n:04}", 'C');
        let dense: Vec<String> = (1..=38).map(spell).collect();
        assert_eq!(
            allocated
                .iter()
                .map(|code| (*code).to_owned())
                .collect::<Vec<_>>(),
            dense
        );
    }

    #[test]
    fn rule_text_and_sources_are_locked() {
        // Read off what a diagnostic actually renders, and reject the generic fallback: a code with
        // no arm of its own would otherwise lock quietly under a wording that says nothing about it.
        let generic = rule_of(FALLBACK_PROBE);
        let mut rendered = String::new();
        for code in ALL_CODES {
            let found = Diagnostic::at(code, Location::root());
            assert_ne!(found.rule(), generic, "{code} renders the generic fallback");
            let line = format!("{code} {} {}\n", found.rule(), found.rule_source());
            rendered.push_str(&line);
        }
        assert_eq!(rendered, EXPECTED);
    }

    #[test]
    fn diagnostics_sort_and_render_deterministically() {
        // Deliberately neither sorted nor reversed, and every key component decides one adjacent
        // pair: the two `/capabilities/0/docs` entries can only come out in code order, the rest in
        // pointer order. Index 10 against index 2 pins that pointer order is not document order.
        let capability = Location::root().key("capabilities");
        let docs = capability.index(0).key("docs");
        let shuffled = vec![
            Diagnostic::at("BXC0005", docs.clone()),
            Diagnostic::at("BXC0009", Location::root().key("revision")),
            Diagnostic::at("BXC0020", capability.index(2).key("shape")),
            Diagnostic::at("BXC0003", docs),
            Diagnostic::at("BXC0001", Location::root()),
            Diagnostic::at("BXC0020", capability.index(10).key("shape")),
        ];
        let found = Diagnostics::new(shuffled).expect("a nonempty collection");
        assert_eq!(found.to_string(), SORTED);
        assert_eq!(found.into_vec().len(), 6);
        assert!(Diagnostics::new(Vec::new()).is_none());
    }

    const SORTED: &str = "\
BXC0001 at=\"\" rule=\"a schema document must be valid UTF-8\" source=\"specs/s4-contract-change-classification.md D1\"
BXC0003 at=\"/capabilities/0/docs\" rule=\"format 1 rejects unknown schema keys\" source=\"specs/s4-contract-change-classification.md D1\"
BXC0005 at=\"/capabilities/0/docs\" rule=\"a schema value must hold its declared JSON type\" source=\"specs/s4-contract-change-classification.md D1\"
BXC0020 at=\"/capabilities/10/shape\" rule=\"format 1's only capability shape is unary\" source=\"specs/s2-contract-generator.md D3\"
BXC0020 at=\"/capabilities/2/shape\" rule=\"format 1's only capability shape is unary\" source=\"specs/s2-contract-generator.md D3\"
BXC0009 at=\"/revision\" rule=\"a revision must be sha256: and 64 lowercase hexadecimal digits\" source=\"specs/s2-contract-generator.md D6\"";

    #[test]
    fn locations_never_echo_a_document_key() {
        // The one dynamic path into a rendered diagnostic, and all of the payload-safety claim that
        // is not already `&'static str`. A plain bounded key becomes the segment; of anything else
        // nothing survives — including a key that would close the rendering's quoting or its line.
        let at = Location::root().key("capabilities").index(7);
        assert_eq!(
            Diagnostic::at("BXC0003", at.key("max_exposure")).location(),
            "/capabilities/7/max_exposure"
        );
        let long = "x".repeat(65);
        for hostile in ["", "a\" rule=\"owned\n", "a/b", "a~b", "-", &long] {
            let found = Diagnostic::at("BXC0003", at.key(hostile));
            assert_eq!(
                found.to_string(),
                "BXC0003 at=\"/capabilities/7/?\" rule=\"format 1 rejects unknown schema keys\" \
                 source=\"specs/s4-contract-change-classification.md D1\""
            );
        }
    }

    #[test]
    fn reader_gates_are_solitary() {
        assert_one_bytes(&[0xff], "BXC0001", "");
        assert_one_bytes(b"{", "BXC0002", "");
        assert_one_bytes(b"[]", "BXC0005", "");

        let mut absent = baseline();
        absent.as_object_mut().unwrap().remove("schema_format");
        assert_one(absent, "BXC0004", "");

        let mut wrong_type = baseline();
        wrong_type["schema_format"] = json!("1");
        assert_one(wrong_type, "BXC0005", "/schema_format");

        let mut future = baseline();
        future["schema_format"] = json!(2);
        assert_one(future, "BXC0006", "/schema_format");
    }

    const CORPUS: &[(&str, &str)] = &[
        ("BXC0001", ""),
        ("BXC0002", ""),
        ("BXC0003", "/future"),
        ("BXC0004", ""),
        ("BXC0005", "/box_id"),
        ("BXC0006", "/schema_format"),
        ("BXC0007", "/capabilities/0/max_exposure"),
        ("BXC0008", "/capabilities/0/idempotency"),
        ("BXC0009", "/revision"),
        ("BXC0010", "/box_id"),
        ("BXC0011", "/capabilities/0/name"),
        ("BXC0012", "/capabilities/0/id"),
        ("BXC0013", "/types/0/name"),
        ("BXC0014", "/types/0/variants/0/name"),
        ("BXC0015", "/capabilities/0/input/name"),
        ("BXC0016", "/capabilities/1/name"),
        ("BXC0017", "/types/1/name"),
        ("BXC0018", "/types/0/variants/1/name"),
        ("BXC0019", "/capabilities/0/input/type"),
        ("BXC0020", "/capabilities/0/shape"),
        ("BXC0021", "/types/0/kind"),
        ("BXC0022", "/types/0/variants/0/payload"),
        ("BXC0023", "/capabilities/0/error"),
        ("BXC0029", "/types/0/variants/0/payload/fields/0/name"),
        ("BXC0030", "/types/0/variants/0/payload/fields/1/name"),
    ];

    fn corpus_bytes(code: Code) -> Vec<u8> {
        match code {
            "BXC0001" => vec![0xff],
            "BXC0002" => b"{".to_vec(),
            _ => {
                let mut value = baseline();
                match code {
                    "BXC0003" => {
                        value
                            .as_object_mut()
                            .unwrap()
                            .insert("future".into(), json!(true));
                    }
                    "BXC0004" => {
                        value.as_object_mut().unwrap().remove("box_id");
                    }
                    "BXC0005" => value["box_id"] = json!(1),
                    "BXC0006" => value["schema_format"] = json!(2),
                    "BXC0007" => value["capabilities"][0]["max_exposure"] = json!("code_only"),
                    "BXC0008" => value["capabilities"][0]["idempotency"] = json!("inherent"),
                    "BXC0009" => value["revision"] = json!("bad"),
                    "BXC0010" => value["box_id"] = json!("Bad"),
                    "BXC0011" => value["capabilities"][0]["name"] = json!("Bad"),
                    "BXC0012" => value["capabilities"][0]["id"] = json!("hello.other"),
                    "BXC0013" => value["types"][0]["name"] = json!("type"),
                    "BXC0014" => value["types"][0]["variants"][0]["name"] = json!("Unknown"),
                    "BXC0015" => value["capabilities"][0]["input"]["name"] = json!("async"),
                    "BXC0016" => {
                        let duplicate = value["capabilities"][0].clone();
                        value["capabilities"]
                            .as_array_mut()
                            .unwrap()
                            .push(duplicate);
                    }
                    "BXC0017" => {
                        let duplicate = value["types"][0].clone();
                        value["types"].as_array_mut().unwrap().push(duplicate);
                    }
                    "BXC0018" => {
                        let duplicate = value["types"][0]["variants"][0].clone();
                        value["types"][0]["variants"]
                            .as_array_mut()
                            .unwrap()
                            .push(duplicate);
                    }
                    "BXC0019" => value["capabilities"][0]["input"]["type"] = json!("Never"),
                    "BXC0020" => value["capabilities"][0]["shape"] = json!("streaming"),
                    "BXC0021" => value["types"][0]["kind"] = json!("struct"),
                    "BXC0022" => value["types"][0]["variants"][0]["payload"] = json!("value"),
                    "BXC0023" => value["capabilities"][0]["error"] = json!("MissingError"),
                    "BXC0029" => {
                        value["types"][0]["variants"][0]["payload"] = json!({
                            "fields": [{
                                "deprecation": null,
                                "docs": [],
                                "name": "type",
                                "type": "String"
                            }],
                            "kind": "named"
                        });
                    }
                    "BXC0030" => {
                        value["types"][0]["variants"][0]["payload"] = json!({
                            "fields": [
                                {"deprecation": null, "docs": [], "name": "field", "type": "String"},
                                {"deprecation": null, "docs": [], "name": "field", "type": "String"}
                            ],
                            "kind": "named"
                        });
                    }
                    _ => unreachable!("the reader corpus has no unregistered code"),
                }
                bytes(&value)
            }
        }
    }

    #[test]
    fn corpus_covers_every_code() {
        let covered: Vec<&str> = CORPUS.iter().map(|(code, _)| *code).collect();
        assert_eq!(covered.as_slice(), READER_CODES);
        let valid = SchemaDocument::parse(&bytes(&baseline())).expect("corpus baseline");
        assert_eq!((valid.capabilities.len(), valid.types.len()), (1, 1));
        assert_eq!(valid.box_id.as_str(), "hello");
        assert_eq!(valid.capabilities[0].input.name, "name");
        assert_eq!(valid.capabilities[0].docs[0], "greet docs");
        assert_eq!(valid.types[0].docs[0], "error docs");
        assert_eq!(valid.types[0].variants[0].docs[0], "empty docs");
        assert_eq!(valid.types[0].variants[0].name, "EmptyName");
        assert_eq!(valid.provenance.value()["opaque"][2], 7);
        for &(code, location) in CORPUS {
            assert_one_bytes(&corpus_bytes(code), code, location);
        }
    }

    #[test]
    fn payload_vocabulary_parses_to_the_exact_ordered_models() {
        let mut value = baseline();
        value["types"][0]["variants"][0]["payload"] = json!({
            "deprecation": {"note": "use detail"},
            "docs": ["payload docs"],
            "kind": "value",
            "type": "u32"
        });
        let parsed = SchemaDocument::parse(&bytes(&value)).expect("value payload");
        assert_eq!(
            parsed.types[0].variants[0].payload,
            SchemaPayload::Value {
                docs: vec!["payload docs".to_owned()],
                deprecation: Some("use detail".to_owned()),
                ty: BoundaryLeaf::U32,
            }
        );

        value["types"][0]["variants"][0]["payload"] = json!({
            "fields": [
                {"deprecation": null, "docs": ["first"], "name": "first", "type": "String"},
                {"deprecation": {"note": "retired"}, "docs": [], "name": "second", "type": "i64"}
            ],
            "kind": "named"
        });
        let parsed = SchemaDocument::parse(&bytes(&value)).expect("named payload");
        assert_eq!(
            parsed.types[0].variants[0].payload,
            SchemaPayload::Named(vec![
                SchemaField {
                    docs: vec!["first".to_owned()],
                    deprecation: None,
                    name: "first".to_owned(),
                    ty: BoundaryLeaf::String,
                },
                SchemaField {
                    docs: Vec::new(),
                    deprecation: Some("retired".to_owned()),
                    name: "second".to_owned(),
                    ty: BoundaryLeaf::I64,
                },
            ])
        );

        value["types"][0]["variants"][0]["payload"] = json!({
            "fields": [],
            "kind": "named"
        });
        let parsed = SchemaDocument::parse(&bytes(&value)).expect("empty named payload");
        assert_eq!(
            parsed.types[0].variants[0].payload,
            SchemaPayload::Named(Vec::new())
        );
    }

    #[test]
    fn payload_fields_are_strict_one_at_a_time() {
        // (named field, key, missing, invalid leaf, expected code)
        let cases = [
            (false, "type", true, false, "BXC0004"),
            (false, "type", false, false, "BXC0005"),
            (false, "type", false, true, "BXC0019"),
            (false, "docs", false, false, "BXC0005"),
            (false, "deprecation", false, false, "BXC0005"),
            (false, "extra", false, false, "BXC0003"),
            (true, "type", true, false, "BXC0004"),
            (true, "type", false, false, "BXC0005"),
            (true, "type", false, true, "BXC0019"),
            (true, "docs", false, false, "BXC0005"),
            (true, "deprecation", false, false, "BXC0005"),
            (true, "extra", false, false, "BXC0003"),
        ];
        for &(named, key, missing, invalid, code) in &cases {
            let mut value = baseline();
            let target = if named {
                named_field(&mut value)
            } else {
                value_payload(&mut value)
            };
            if missing {
                target.as_object_mut().unwrap().remove(key);
            } else {
                target[key] = if invalid { json!("Never") } else { json!(true) };
            }
            let base = if named { FIELD } else { PAYLOAD };
            let location = if missing {
                base.to_owned()
            } else {
                format!("{base}/{key}")
            };
            assert_one(value, code, &location);
        }
    }

    #[test]
    fn named_field_names_are_nfc_unique() {
        let mut value = baseline();
        value["types"][0]["variants"][0]["payload"] = json!({
            "fields": [
                {"deprecation": null, "docs": [], "name": "e\u{301}", "type": "String"},
                {"deprecation": null, "docs": [], "name": "é", "type": "String"}
            ],
            "kind": "named"
        });
        assert_one(
            value,
            "BXC0030",
            "/types/0/variants/0/payload/fields/1/name",
        );
    }

    #[test]
    fn every_unemitted_exposure_and_idempotency_spelling_is_rejected() {
        for spelling in ["code_only", "internal"] {
            let mut value = baseline();
            value["capabilities"][0]["max_exposure"] = json!(spelling);
            assert_one(value, "BXC0007", "/capabilities/0/max_exposure");
        }
        let mut value = baseline();
        value["capabilities"][0]["idempotency"] = json!("inherent");
        assert_one(value, "BXC0008", "/capabilities/0/idempotency");
    }

    #[test]
    fn reserved_unknown_and_keyword_variant_names_are_rejected() {
        for name in ["Unknown", "gen"] {
            let mut value = baseline();
            value["types"][0]["variants"][0]["name"] = json!(name);
            assert_one(value, "BXC0014", "/types/0/variants/0/name");
        }
    }

    #[test]
    fn decomposed_identifiers_and_error_references_are_stored_in_nfc_form() {
        let mut value = baseline();
        value["types"][0]["name"] = json!("GreetE\u{301}rror");
        value["capabilities"][0]["error"] = json!("GreetÉrror");
        value["types"][0]["variants"][0]["name"] = json!("EmptyNa\u{301}me");
        value["capabilities"][0]["input"]["name"] = json!("na\u{301}me");
        let parsed = SchemaDocument::parse(&bytes(&value)).expect("canonicalizable identifiers");
        assert_eq!(parsed.types[0].name, "GreetÉrror");
        assert_eq!(parsed.types[0].variants[0].name, "EmptyNáme");
        assert_eq!(parsed.capabilities[0].input.name, "náme");

        let mut reference = baseline();
        reference["types"][0]["name"] = json!("GreetE\u{301}rror");
        reference["capabilities"][0]["error"] = json!("GreetE\u{301}rror");
        let parsed = SchemaDocument::parse(&bytes(&reference)).expect("canonicalizable reference");
        assert_eq!(parsed.capabilities[0].error, "GreetÉrror");

        let mut invalid = baseline();
        invalid["capabilities"][0]["error"] = json!("type");
        assert_one(invalid, "BXC0023", "/capabilities/0/error");
    }

    #[test]
    fn diagnostics_accumulate_and_sort_after_the_gate() {
        let mut value = baseline();
        value["future"] = json!(true);
        value["revision"] = json!("bad");
        value["capabilities"][0]["docs"] = json!(true);
        value["capabilities"][0]["shape"] = json!("streaming");
        value["types"][0]["variants"][0]["payload"] = json!("value");
        let diagnostics = SchemaDocument::parse(&bytes(&value)).expect_err("accumulated defects");
        let found = diagnostics.into_vec();
        let actual: Vec<(&str, &str)> = found
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.location()))
            .collect();
        assert_eq!(
            actual,
            vec![
                ("BXC0005", "/capabilities/0/docs"),
                ("BXC0020", "/capabilities/0/shape"),
                ("BXC0003", "/future"),
                ("BXC0009", "/revision"),
                ("BXC0022", "/types/0/variants/0/payload"),
            ]
        );
    }

    #[test]
    fn multiple_capabilities_and_types_and_empty_collections_are_valid() {
        let mut value = baseline();
        let mut capability = value["capabilities"][0].clone();
        capability["name"] = json!("store");
        capability["id"] = json!("hello.store");
        capability["error"] = json!("StoreError");
        capability["input"]["name"] = json!("value");
        capability["output"]["type"] = json!("bool");
        value["capabilities"]
            .as_array_mut()
            .unwrap()
            .push(capability);
        let mut schema_type = value["types"][0].clone();
        schema_type["name"] = json!("StoreError");
        schema_type["variants"][0]["name"] = json!("Denied");
        value["types"].as_array_mut().unwrap().push(schema_type);
        let parsed = SchemaDocument::parse(&bytes(&value)).expect("multi-document");
        assert_eq!(parsed.capabilities.len(), 2);
        assert_eq!(parsed.types.len(), 2);

        value["capabilities"] = json!([]);
        value["types"] = json!([]);
        let empty = SchemaDocument::parse(&bytes(&value)).expect("empty collections");
        assert!(empty.capabilities.is_empty());
        assert!(empty.types.is_empty());
    }

    #[test]
    fn public_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(Diagnostic, Diagnostics)>();
    }
}
