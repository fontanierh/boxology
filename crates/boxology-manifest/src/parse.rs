use crate::{D2_SOURCE, Diagnostic, Diagnostics, GlobPattern, LineColumn, RelativePath, Span};
use boxology_contract::{BoxId, CapabilityId, CapabilityName};
use std::ops::Range;
use toml_edit::{Document, Item, TableLike, Value};

type Code = &'static str;
/// A borrowed manifest table, and one array-of-tables element with its own span.
type Fields<'t> = &'t dyn TableLike;
type Element<'i> = (Span, Fields<'i>);
const PACKAGES: Code = "boxology-details/02-packages.md";
const ROLES: Code =
    "a crate role must be box-implementation, box-contract, composition, or platform";
const ORIGIN: LineColumn = LineColumn { line: 1, column: 1 };
const POINT: Span = Span {
    start: ORIGIN,
    end: ORIGIN,
};
/// The complete schema-1 key inventory. Every key outside it rejects, at every nesting level.
const TOP_KEYS: &str = "schema id kind owned display_name fixtures protected quality crates derived imports composition";
/// The only key `[quality]` models; nesting inherits the same fail-closed inventory rule.
const QUALITY_KEYS: &str = "commands";
/// `[composition]`'s own keys: an array of tables nested in a table is still that table's key.
const COMPOSITION_KEYS: &str = "boxes bindings";
/// The key inventory of one element of each array-of-tables section, applied per element.
const CRATE_KEYS: &str = "cargo_package path role";
const DERIVED_KEYS: &str = "id generator inputs outputs";
const IMPORT_KEYS: &str = "package contract";
const BINDING_KEYS: &str = "box capability transport exposure";

/// The declared package kind. `provider` parses as TOML and is rejected by rule (BXW0008), so it
/// is deliberately absent here: the model can only hold a kind v0 supports.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    /// A box package: one implementation and its generated contract.
    Box,
    /// A composition package: a wiring of boxes.
    Composition,
    /// A platform package: repository-wide infrastructure.
    Platform,
}

/// The role a Cargo crate plays in its package. Declared, never inferred from a path or a suffix.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CrateRole {
    /// The package's hand-authored box implementation.
    BoxImplementation,
    /// The package's generated contract crate.
    BoxContract,
    /// A crate wiring selected boxes into a composition.
    Composition,
    /// Repository-wide platform infrastructure.
    Platform,
}
impl CrateRole {
    /// Reads the closed, case-sensitive foundation crate-role vocabulary.
    fn parse(text: &str) -> Option<Self> {
        match text {
            "box-implementation" => Some(Self::BoxImplementation),
            "box-contract" => Some(Self::BoxContract),
            "composition" => Some(Self::Composition),
            "platform" => Some(Self::Platform),
            _ => None,
        }
    }
}

/// A Cargo crate this package declares, with the role it plays and the directory holding it.
#[derive(Debug, Eq, PartialEq)]
pub struct CrateEntry {
    cargo_package: String,
    path: RelativePath,
    role: CrateRole,
}
impl CrateEntry {
    ref_getters! {
        #[doc = "Returns the declared Cargo package name."] cargo_package: &str = cargo_package;
        #[doc = "Returns the crate's literal package-relative directory."] path: &RelativePath = path;
    }
    copy_getters! {
        #[doc = "Returns the declared crate role."] role: CrateRole = role;
    }
}

/// One declared derived output: a generator identity, its complete semantic inputs, and the
/// paths it owns. Inputs are declared and fail closed, so regeneration proves provenance.
#[derive(Debug, Eq, PartialEq)]
pub struct DerivedOutput {
    id: BoxId,
    generator: String,
    inputs: Vec<GlobPattern>,
    outputs: Vec<GlobPattern>,
}
impl DerivedOutput {
    ref_getters! {
        #[doc = "Returns the package-local output id, carrying its grammar in its type."]
        id: &BoxId = id;
        #[doc = "Returns the logical generator identity."] generator: &str = generator;
        #[doc = "Returns the declared semantic inputs, in declaration order."] inputs: &[GlobPattern] = inputs;
        #[doc = "Returns the declared output patterns, in declaration order."] outputs: &[GlobPattern] = outputs;
    }
}

/// One declared dependency: the package whose canonical contract this package imports. The
/// declared `contract` is validated and deliberately not stored — v1 requires it to equal
/// `package`, so a field for it could only ever hold this same id a second time.
#[derive(Debug, Eq, PartialEq)]
pub struct Import {
    package: BoxId,
}
impl Import {
    ref_getters! {
        #[doc = "Returns the imported package's id."] package: &BoxId = package;
    }
}

/// How a binding reaches the capability it wires. The vocabulary is closed and case-sensitive.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Transport {
    /// Linked into the same process as its caller.
    InProcess,
    /// Reached over HTTP.
    Http,
}
impl Transport {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "in-process" => Some(Self::InProcess),
            "http" => Some(Self::Http),
            _ => None,
        }
    }
}

/// How widely a binding is exposed. `Ord` is the S4 D5 total order, because T5 compares a binding's
/// exposure against the box's maximum and a declaration-accident order would misclassify that.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Exposure {
    /// Visible only to code compiled with the composition.
    CodeOnly,
    /// Reachable inside the deployment.
    Internal,
    /// Reachable from outside the deployment.
    External,
}
impl Exposure {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "code_only" => Some(Self::CodeOnly),
            "internal" => Some(Self::Internal),
            "external" => Some(Self::External),
            _ => None,
        }
    }
}

/// One wiring of one box-qualified capability, held as a `CapabilityId` rather than as its two
/// segments: it already carries both validated halves, and BXW0041 needs the qualifier back. The
/// declared `box` is validated and deliberately not stored, as `[[imports]]` does with `contract`:
/// BXW0041 requires it to equal the capability's own qualifier, so a field could only hold that
/// same id a second time.
#[derive(Debug, Eq, PartialEq)]
pub struct Binding {
    capability: CapabilityId,
    transport: Transport,
    exposure: Option<Exposure>,
}
impl Binding {
    /// Returns the declared exposure, which is optional and defaulted by no one here.
    pub fn exposure(&self) -> Option<Exposure> {
        self.exposure
    }
    ref_getters! {
        #[doc = "Returns the box-qualified capability."] capability: &CapabilityId = capability;
    }
    copy_getters! {
        #[doc = "Returns the declared transport."] transport: Transport = transport;
    }
}

/// A composition package's selected boxes and their bindings. Whether a selected identity exists,
/// and whether a binding suits the generated contract, are cross-document questions T5 owns.
#[derive(Debug, Eq, PartialEq)]
pub struct Composition {
    boxes: Vec<BoxId>,
    bindings: Vec<Binding>,
}
impl Composition {
    ref_getters! {
        #[doc = "Returns the selected boxes, in declaration order."] boxes: &[BoxId] = boxes;
        #[doc = "Returns the declared bindings, in declaration order."] bindings: &[Binding] = bindings;
    }
}

/// A validated schema-1 `boxology.toml`. Construction is the validation: a `Manifest` exists only
/// for a document that satisfied every rule this crate knows, so consumers never re-check it.
#[derive(Debug, Eq, PartialEq)]
pub struct Manifest {
    id: BoxId,
    kind: Kind,
    owned: Vec<GlobPattern>,
    display_name: Option<String>,
    fixtures: Vec<GlobPattern>,
    protected: Vec<GlobPattern>,
    quality_commands: Vec<String>,
    crates: Vec<CrateEntry>,
    derived: Vec<DerivedOutput>,
    imports: Vec<Import>,
    composition: Option<Composition>,
}
impl Manifest {
    /// Parses `bytes` as the manifest logically located at `manifest_path`.
    ///
    /// The schema gate runs first and alone: unreadable bytes, malformed TOML, and an absent or
    /// unknown schema version each yield one diagnostic and never a field error, so a future schema
    /// is rejected on its version, not mis-described against schema 1's key inventory. Every later
    /// rejection accumulates, so one call reports the whole document.
    pub fn parse(manifest_path: RelativePath, bytes: &[u8]) -> Result<Manifest, Diagnostics> {
        let path = manifest_path;
        let Ok(source) = std::str::from_utf8(bytes) else {
            return Err(one(&path, POINT, "BXW0001", "manifest bytes"));
        };
        let document = match Document::parse(source) {
            Ok(document) => document,
            Err(error) => {
                let span = locate(source, error.span());
                return Err(one(&path, span, "BXW0002", "manifest document"));
            }
        };
        let root = document.as_table();
        let span = item_span(source, root.get("schema"));
        let Some(version) = root.get("schema").and_then(Item::as_integer) else {
            return Err(one(&path, span, "BXW0003", "manifest key schema"));
        };
        if version != 1 {
            return Err(one(&path, span, "BXW0004", "manifest key schema"));
        }
        let mut parser = Parser {
            source,
            path,
            errors: Vec::new(),
        };
        parser.unknown(root, TOP_KEYS);
        let id_span = item_span(source, root.get("id"));
        let id = match parser.text(root, "id", "BXW0005") {
            Some(raw) => BoxId::new(raw).ok().or_else(|| {
                parser.key("BXW0006", id_span, "id");
                None
            }),
            None => None,
        };
        let kind_span = item_span(source, root.get("kind"));
        let kind = match parser.text(root, "kind", "BXW0007") {
            Some("box") => Some(Kind::Box),
            Some("composition") => Some(Kind::Composition),
            Some("platform") => Some(Kind::Platform),
            Some("provider") => {
                parser.key("BXW0008", kind_span, "kind");
                None
            }
            Some(_) => {
                parser.key("BXW0009", kind_span, "kind");
                None
            }
            None => None,
        };
        let owned = match root.get("owned") {
            Some(item) => parser.patterns("owned", item),
            None => {
                parser.key("BXW0012", POINT, "owned");
                Vec::new()
            }
        };
        let display_name = parser.optional_text(root, "display_name");
        // Fixture opacity is a platform-package privilege, so the key is judged against the
        // declared kind; an already-rejected kind adds no second complaint about this key.
        let fixtures = match root.get("fixtures") {
            None => Vec::new(),
            Some(item) => {
                // Both rules below reject the key's presence, not the value, so both span the key.
                let span = key_span(source, root, "fixtures");
                if matches!(kind, Some(Kind::Box | Kind::Composition)) {
                    parser.key("BXW0021", span, "fixtures");
                }
                if item.as_array().is_some_and(|array| array.is_empty()) {
                    parser.key("BXW0034", span, "fixtures");
                }
                parser.patterns("fixtures", item)
            }
        };
        // Protected control-plane paths are a platform-package privilege, so the key is judged
        // against the declared kind; an already-rejected kind adds no second complaint here.
        let protected = match root.get("protected") {
            None => Vec::new(),
            Some(item) => {
                let span = key_span(source, root, "protected");
                if matches!(kind, Some(Kind::Box | Kind::Composition)) {
                    parser.key("BXW0074", span, "protected");
                }
                if item.as_array().is_some_and(|array| array.is_empty()) {
                    parser.key("BXW0034", span, "protected");
                }
                parser.patterns("protected", item)
            }
        };
        let quality_commands = match root.get("quality") {
            Some(item) => parser.quality(item),
            None => Vec::new(),
        };
        let crates = root.get("crates").map_or(Vec::new(), |i| parser.crates(i));
        let outputs = root.get("derived");
        let derived = outputs.map_or(Vec::new(), |i| parser.derived(i));
        let declared = root.get("imports");
        let imports = declared.map_or(Vec::new(), |i| parser.imports(i));
        let composition = parser.composition(root, kind);
        match (Diagnostics::new(parser.errors), id.zip(kind)) {
            (None, Some((id, kind))) => Ok(Manifest {
                id,
                kind,
                owned,
                display_name,
                fixtures,
                protected,
                quality_commands,
                crates,
                derived,
                imports,
                composition,
            }),
            (Some(diagnostics), _) => Err(diagnostics),
            // Unreachable: a missing or rejected id or kind always records a diagnostic above. It
            // is coded rather than panicked so the crate keeps no uncoded failure path even if the
            // checks above are ever reordered.
            (None, None) => Err(one(&parser.path, POINT, "BXW0005", "manifest key id")),
        }
    }
    /// Returns the optional display name, which never carries identity.
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
    /// Returns the composition section, which exists on exactly the composition packages.
    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }
    ref_getters! {
        #[doc = "Returns the validated package id."] id: &BoxId = id;
        #[doc = "Returns the declared owned patterns, in declaration order."] owned: &[GlobPattern] = owned;
        #[doc = "Returns the declared fixture patterns; always empty off a platform package."] fixtures: &[GlobPattern] = fixtures;
        #[doc = "Returns the declared protected control-plane patterns, in declaration order."] protected: &[GlobPattern] = protected;
        #[doc = "Returns the declared quality commands, in declaration order."] quality_commands: &[String] = quality_commands;
        #[doc = "Returns the declared Cargo crates, in declaration order."] crates: &[CrateEntry] = crates;
        #[doc = "Returns the declared derived outputs, in declaration order."] derived: &[DerivedOutput] = derived;
        #[doc = "Returns the declared imports, in declaration order."] imports: &[Import] = imports;
    }
    copy_getters! {
        #[doc = "Returns the declared package kind."] kind: Kind = kind;
    }
}

/// The accumulating half of a parse: everything past the schema gate records instead of returning.
struct Parser<'a> {
    source: &'a str,
    path: RelativePath,
    errors: Vec<Diagnostic>,
}
impl Parser<'_> {
    fn push(&mut self, code: Code, span: Span, what: Code) {
        self.errors.push(diagnose(&self.path, span, code, what));
    }
    /// Records a key-scoped rejection, echoing the key name only when it is plain identifier text,
    /// so a hostile key can never place arbitrary bytes into a report.
    fn key(&mut self, code: Code, span: Span, name: &str) {
        let what = match !name.is_empty() && name.bytes().all(is_plain) {
            true => format!("manifest key {name}"),
            false => String::from("manifest key"),
        };
        self.errors.push(diagnose(&self.path, span, code, what));
    }
    /// Records BXW0010 for every key of `table` outside the space-separated `known` inventory, per
    /// table, so it reaches each nesting level as that level gains a modelled shape.
    fn unknown(&mut self, table: &dyn TableLike, known: &str) {
        for (name, item) in table.iter() {
            if !known.split(' ').any(|allowed| allowed == name) {
                let key = table.get_key_value(name).and_then(|(key, _)| key.span());
                let span = locate(self.source, key.or_else(|| item.span()));
                self.key("BXW0010", span, name);
            }
        }
    }
    /// Reads a string-valued key, coding absence and wrong type alike as `code`.
    fn text<'t>(&mut self, table: &'t dyn TableLike, key: Code, code: Code) -> Option<&'t str> {
        let item = table.get(key);
        match item.and_then(Item::as_str) {
            Some(value) => Some(value),
            None => {
                let span = item_span(self.source, item);
                self.key(code, span, key);
                None
            }
        }
    }
    /// Reads an optional string-valued key: absence is legal, a present non-string is BXW0011.
    fn optional_text(&mut self, table: &dyn TableLike, key: Code) -> Option<String> {
        let item = table.get(key)?;
        match item.as_str() {
            Some(value) => Some(value.to_owned()),
            None => {
                self.key("BXW0011", item_span(self.source, Some(item)), key);
                None
            }
        }
    }
    /// Reads `[quality]`: the nested key inventory first, then its required list of commands, each
    /// of which must be text that is not blank. Command text is never echoed into a diagnostic.
    fn quality(&mut self, item: &Item) -> Vec<String> {
        let table_span = item_span(self.source, Some(item));
        let Some(table) = item.as_table_like() else {
            self.key("BXW0011", table_span, "quality");
            return Vec::new();
        };
        self.unknown(table, QUALITY_KEYS);
        let Some(item) = table.get("commands") else {
            self.key("BXW0012", table_span, "commands");
            return Vec::new();
        };
        let span = item_span(self.source, Some(item));
        let Some(array) = item.as_array() else {
            self.key("BXW0011", span, "commands");
            return Vec::new();
        };
        if array.is_empty() {
            self.key("BXW0026", span, "commands");
        }
        let mut commands = Vec::new();
        for value in array.iter() {
            let span = locate(self.source, value.span());
            match value.as_str() {
                None => self.key("BXW0011", span, "commands"),
                Some(text) if text.trim().is_empty() => self.key("BXW0026", span, "commands"),
                Some(text) => commands.push(text.to_owned()),
            }
        }
        commands
    }
    /// Reads an array of glob patterns, coding non-arrays, non-string entries, and duplicates.
    fn patterns(&mut self, key: Code, item: &Item) -> Vec<GlobPattern> {
        let Some(array) = item.as_array() else {
            let span = item_span(self.source, Some(item));
            self.key("BXW0011", span, key);
            return Vec::new();
        };
        let mut patterns: Vec<GlobPattern> = Vec::new();
        for value in array.iter() {
            let span = locate(self.source, value.span());
            let Some(text) = value.as_str() else {
                self.key("BXW0011", span, key);
                continue;
            };
            let parsed = GlobPattern::parse(text, &self.path, span);
            match parsed {
                Err(diagnostic) => self.errors.push(diagnostic),
                Ok(seen) if patterns.contains(&seen) => self.push("BXW0020", span, "glob pattern"),
                Ok(pattern) => patterns.push(pattern),
            }
        }
        patterns
    }
    /// Reads an array-of-tables section into its elements and their spans, and runs each element's
    /// key inventory here: `ArrayOfTables` is not `TableLike`, so nothing in `unknown`'s signature
    /// would demand the per-element call, and a forgotten one would silently accept any key. Every
    /// element therefore reaches `unknown` through this one gate. A section spelled as a plain
    /// table, and an array entry that is not a table, cannot be read as elements and are typed
    /// defects, as `[[quality]]` is; the inline-table spelling of an element is equivalent TOML.
    fn section<'i>(&mut self, item: &'i Item, key: Code, known: &str) -> Vec<Element<'i>> {
        let mut tables: Vec<Element<'i>> = Vec::new();
        match item {
            Item::ArrayOfTables(array) => {
                let spanned = array
                    .iter()
                    .map(|t| (locate(self.source, t.span()), t as _));
                tables.extend(spanned);
            }
            Item::Value(Value::Array(array)) => {
                for value in array.iter() {
                    let span = locate(self.source, value.span());
                    match value.as_inline_table() {
                        Some(table) => tables.push((span, table)),
                        None => self.key("BXW0011", span, key),
                    }
                }
            }
            _ => self.key("BXW0011", item_span(self.source, Some(item)), key),
        }
        for (_, table) in &tables {
            self.unknown(*table, known);
        }
        tables
    }
    /// Reads a required string-valued key of a section element: an absent key is BXW0012 located
    /// at the element it is missing from, and a present non-string is BXW0011 at its own value.
    fn field<'t>(&mut self, at: Fields<'t>, key: Code, whole: Span) -> Option<&'t str> {
        let Some(item) = at.get(key) else {
            self.key("BXW0012", whole, key);
            return None;
        };
        let text = item.as_str();
        if text.is_none() {
            self.key("BXW0011", item_span(self.source, Some(item)), key);
        }
        text
    }
    /// Passes a validated value through, recording `code` at `key`'s own key span when validation
    /// rejected it. The rejected value never reaches the report; only the key name does.
    fn check<T>(&mut self, at: Fields<'_>, key: Code, code: Code, ok: Option<T>) -> Option<T> {
        if ok.is_none() {
            let span = key_span(self.source, at, key);
            self.key(code, span, key);
        }
        ok
    }
    /// Reads a required pattern list: an absent key is BXW0012, an empty list BXW0034 at the key
    /// whose presence is the defect, and every entry keeps the glob dialect's own codes.
    fn list(&mut self, at: Fields<'_>, key: Code, whole: Span) -> Vec<GlobPattern> {
        let Some(item) = at.get(key) else {
            self.key("BXW0012", whole, key);
            return Vec::new();
        };
        if item.as_array().is_some_and(|array| array.is_empty()) {
            let span = key_span(self.source, at, key);
            self.key("BXW0034", span, key);
        }
        self.patterns(key, item)
    }
    /// Reads `[[crates]]`. A role is declared, never inferred, and a crate path is a literal
    /// directory: dialect metacharacters and escaping segments are rejected, not expanded.
    fn crates(&mut self, item: &Item) -> Vec<CrateEntry> {
        let mut crates: Vec<CrateEntry> = Vec::new();
        for (whole, table) in self.section(item, "crates", CRATE_KEYS) {
            let name = self.field(table, "cargo_package", whole).and_then(|text| {
                let plain = !text.is_empty() && text.bytes().all(is_plain);
                let name = plain.then(|| text.to_owned());
                self.check(table, "cargo_package", "BXW0030", name)
            });
            let path = self.field(table, "path", whole).and_then(|text| {
                let literal = !text.starts_with('!') && !text.contains(['*', '?', '[']);
                let path = RelativePath::new(text).ok().filter(|_| literal);
                self.check(table, "path", "BXW0028", path)
            });
            let role = self
                .field(table, "role", whole)
                .and_then(|text| self.check(table, "role", "BXW0027", CrateRole::parse(text)));
            if let (Some(name), Some(path), Some(role)) = (name, path, role) {
                // Each identity is checked against the ones already accepted, at its own key, so a
                // clash reports where it is written and one entry never rejects itself.
                let fresh = !crates.iter().any(|other| other.cargo_package == name);
                let free = !crates.iter().any(|other| other.path == path);
                let fresh = self.check(table, "cargo_package", "BXW0029", fresh.then_some(()));
                let free = self.check(table, "path", "BXW0029", free.then_some(()));
                if fresh.and(free).is_some() {
                    crates.push(CrateEntry {
                        cargo_package: name,
                        path,
                        role,
                    });
                }
            }
        }
        crates
    }
    /// Reads `[[derived]]`. An output id and a generator identity share the package-id grammar,
    /// and both pattern lists are required: declared inputs are complete or regeneration proves
    /// nothing about provenance.
    fn derived(&mut self, item: &Item) -> Vec<DerivedOutput> {
        let mut derived: Vec<DerivedOutput> = Vec::new();
        for (whole, table) in self.section(item, "derived", DERIVED_KEYS) {
            let raw = self.field(table, "id", whole);
            let id = raw.and_then(|t| self.check(table, "id", "BXW0031", BoxId::new(t).ok()));
            let id = id.and_then(|id| {
                let fresh = !derived.iter().any(|other| other.id == id);
                self.check(table, "id", "BXW0032", fresh.then_some(id))
            });
            let generator = self
                .field(table, "generator", whole)
                .and_then(|text| self.check(table, "generator", "BXW0033", identity(text)));
            let inputs = self.list(table, "inputs", whole);
            let outputs = self.list(table, "outputs", whole);
            if let (Some(id), Some(generator)) = (id, generator) {
                derived.push(DerivedOutput {
                    id,
                    generator,
                    inputs,
                    outputs,
                });
            }
        }
        derived
    }
    /// Reads `[[imports]]`. `contract` is required, not defaulted to `package`: 02-packages writes
    /// it explicitly, and a default would let a document that names no contract be accepted as if
    /// it had named one. V1 imports the package's canonical contract, so the two must be equal.
    fn imports(&mut self, item: &Item) -> Vec<Import> {
        let mut imports: Vec<Import> = Vec::new();
        for (whole, table) in self.section(item, "imports", IMPORT_KEYS) {
            let raw = self.field(table, "package", whole);
            let id = raw.and_then(|t| self.check(table, "package", "BXW0006", BoxId::new(t).ok()));
            let package = id.and_then(|id| {
                let fresh = !imports.iter().any(|other| other.package == id);
                self.check(table, "package", "BXW0025", fresh.then_some(id))
            });
            // `contract` holds the wrong value, so the defect spans that key. An element missing
            // either key is already coded as absent and is not also called unequal.
            let contract = self.field(table, "contract", whole);
            if raw.zip(contract).is_some_and(|(p, c)| p != c) {
                let span = key_span(self.source, table, "contract");
                self.key("BXW0024", span, "contract");
            }
            if let Some(package) = package {
                imports.push(Import { package });
            }
        }
        imports
    }
    /// Reads `[composition]`, whose presence is itself a kind claim: only a composition package may
    /// declare one (BXW0022), and every composition package must (BXW0023). It is read whatever the
    /// kind, so a misplaced one still reports what is inside it, as a misplaced `fixtures` does; an
    /// already-rejected kind adds no second complaint. `bindings` is optional, being a container
    /// section: a composition may link boxes without wiring a capability across them.
    fn composition(&mut self, root: &dyn TableLike, kind: Option<Kind>) -> Option<Composition> {
        let Some(item) = root.get("composition") else {
            if kind == Some(Kind::Composition) {
                self.key("BXW0023", POINT, "composition");
            }
            return None;
        };
        if matches!(kind, Some(Kind::Box | Kind::Platform)) {
            let span = key_span(self.source, root, "composition");
            self.key("BXW0022", span, "composition");
        }
        let whole = item_span(self.source, Some(item));
        let Some(table) = item.as_table_like() else {
            self.key("BXW0011", whole, "composition");
            return None;
        };
        self.unknown(table, COMPOSITION_KEYS);
        let boxes = self.boxes(table, whole);
        let bindings = table
            .get("bindings")
            .map_or(Vec::new(), |i| self.bindings(i, &boxes));
        Some(Composition { boxes, bindings })
    }
    /// Reads `boxes`: required, unique, box-id grammar, and non-empty. Emptiness is BXW0034 because
    /// this list's presence is itself a claim, unlike a container section's.
    fn boxes(&mut self, at: Fields<'_>, whole: Span) -> Vec<BoxId> {
        let Some(item) = at.get("boxes") else {
            self.key("BXW0012", whole, "boxes");
            return Vec::new();
        };
        let Some(array) = item.as_array() else {
            self.key("BXW0011", item_span(self.source, Some(item)), "boxes");
            return Vec::new();
        };
        if array.is_empty() {
            self.key("BXW0034", key_span(self.source, at, "boxes"), "boxes");
        }
        let mut boxes: Vec<BoxId> = Vec::new();
        for value in array.iter() {
            let span = locate(self.source, value.span());
            match value.as_str().map(BoxId::new) {
                None => self.key("BXW0011", span, "boxes"),
                Some(Err(_)) => self.key("BXW0035", span, "boxes"),
                Some(Ok(id)) if boxes.contains(&id) => self.key("BXW0036", span, "boxes"),
                Some(Ok(id)) => boxes.push(id),
            }
        }
        boxes
    }
    /// Reads `[[composition.bindings]]` through the one section funnel, so each element's key
    /// inventory is applied. Both cross-checks are in-document: a binding names a box this document
    /// selected (BXW0040), by a capability that same box qualifies (BXW0041).
    fn bindings(&mut self, item: &Item, boxes: &[BoxId]) -> Vec<Binding> {
        let mut bindings: Vec<Binding> = Vec::new();
        for (whole, table) in self.section(item, "bindings", BINDING_KEYS) {
            let raw = self.field(table, "box", whole);
            let id = raw.and_then(|t| self.check(table, "box", "BXW0035", BoxId::new(t).ok()));
            let box_id = id.and_then(|id| {
                let selected = boxes.contains(&id);
                self.check(table, "box", "BXW0040", selected.then_some(id))
            });
            let cap = self.field(table, "capability", whole);
            let named = cap.and_then(|t| self.check(table, "capability", "BXW0037", qualified(t)));
            // A `box` absent is already coded as absent, and one rejected qualifies nothing.
            let capability = named.and_then(|id| {
                let own = raw.is_none_or(|text| text == id.box_id().as_str());
                self.check(table, "capability", "BXW0041", own.then_some(id))
            });
            let transport = self
                .field(table, "transport", whole)
                .and_then(|t| self.check(table, "transport", "BXW0038", Transport::parse(t)));
            // Absent is legal for `exposure` alone: 02-packages writes a binding both ways.
            let exposure = table.get("exposure").and_then(|item| match item.as_str() {
                Some(text) => self.check(table, "exposure", "BXW0039", Exposure::parse(text)),
                None => {
                    self.key("BXW0011", item_span(self.source, Some(item)), "exposure");
                    None
                }
            });
            let wired = box_id.zip(capability).zip(transport);
            bindings.extend(wired.map(|((_, capability), transport)| Binding {
                capability,
                transport,
                exposure,
            }));
        }
        bindings
    }
}
/// The `<box>.<name>` binding-capability grammar, composed of the contract crate's own identities
/// so it cannot drift: a box reference, the first dot, and a box-local `[a-z][a-z0-9_]*` name.
fn qualified(text: &str) -> Option<CapabilityId> {
    let (box_id, name) = text.split_once('.')?;
    Some(CapabilityId::new(
        BoxId::new(box_id).ok()?,
        CapabilityName::new(name).ok()?,
    ))
}
/// The `[a-z][a-z0-9-]*` identity grammar, taken from `BoxId` so the two cannot drift apart.
fn identity(text: &str) -> Option<String> {
    BoxId::new(text).ok().map(|id| id.as_str().to_owned())
}
/// Whether `byte` is safe to echo into a report and legal in a Cargo package name.
fn is_plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}
/// Locates a key's own name, whose presence — not its value — is what some rules reject.
fn key_span(source: &str, at: Fields<'_>, key: Code) -> Span {
    locate(
        source,
        at.get_key_value(key).and_then(|(key, _)| key.span()),
    )
}
/// The frozen rule text of every code reported here, in one table so a code and its wording cannot
/// drift apart. The fallback is unreachable, and is coded text rather than a panic.
fn rule_of(code: Code) -> Code {
    match code {
        "BXW0001" => "boxology.toml must be valid UTF-8",
        "BXW0002" => "boxology.toml must be well-formed TOML",
        "BXW0003" => "the manifest must declare an integer schema version",
        "BXW0004" => "this reader supports manifest schema 1 and rejects unknown versions",
        "BXW0005" => "the manifest must declare a string package id",
        "BXW0006" => "the package id must match [a-z][a-z0-9-]*",
        "BXW0007" => "the manifest must declare a string package kind",
        "BXW0008" => "provider packages are not supported in v0",
        "BXW0009" => "the package kind must be box, composition, or platform",
        "BXW0010" => "schema 1 rejects unknown manifest keys",
        "BXW0011" => "a known manifest key must hold its declared TOML type",
        "BXW0012" => "a required manifest key must be present",
        "BXW0020" => "patterns within one list must be unique",
        "BXW0021" => "only a platform package may declare fixtures",
        "BXW0022" => "only composition packages may declare a composition section",
        "BXW0023" => "a composition package must declare its composition section",
        "BXW0024" => "v1 imports the package's canonical contract, so contract must equal package",
        "BXW0025" => "declared import packages must be unique",
        "BXW0026" => "a quality command must be non-blank text",
        "BXW0027" => ROLES,
        "BXW0028" => "crate paths must be literal relative paths",
        "BXW0029" => "crate paths and cargo package names must be unique",
        "BXW0030" => "cargo package names must be non-empty identifiers",
        "BXW0031" => "derived output ids must match [a-z][a-z0-9-]*",
        "BXW0032" => "derived output ids must be unique",
        "BXW0033" => "generator identities must match [a-z][a-z0-9-]*",
        "BXW0034" => "this list must contain at least one entry",
        "BXW0035" => "box references must match [a-z][a-z0-9-]*",
        "BXW0036" => "selected boxes must be unique",
        "BXW0037" => "binding capabilities must be box-qualified names",
        "BXW0038" => "binding transport must be in-process or http",
        "BXW0039" => "binding exposure must be code_only, internal, or external",
        "BXW0040" => "every binding must reference a selected box",
        "BXW0041" => "a binding capability must be qualified by its own box",
        "BXW0074" => "only a platform package may declare protected control-plane paths",
        _ => "the manifest must satisfy schema 1",
    }
}
/// The shape, id grammar, kind vocabulary, crate-role vocabulary, and canonical-import rule are
/// 02-packages'; the rest is the S5 spec's D2.
fn source_of(code: Code) -> Code {
    match ("BXW0003"..="BXW0009").contains(&code) || matches!(code, "BXW0024" | "BXW0027") {
        true => PACKAGES,
        false => D2_SOURCE,
    }
}
fn diagnose(path: &RelativePath, span: Span, code: Code, what: impl Into<String>) -> Diagnostic {
    Diagnostic {
        path: path.clone(),
        span,
        code,
        offending: what.into(),
        rule: rule_of(code),
        rule_source: source_of(code),
    }
}
fn one(path: &RelativePath, span: Span, code: Code, what: Code) -> Diagnostics {
    Diagnostics(vec![diagnose(path, span, code, what)])
}
fn item_span(source: &str, item: Option<&Item>) -> Span {
    locate(source, item.and_then(Item::span))
}
/// Converts a `toml_edit` byte range into `Span`'s one-based line and character columns. An absent
/// range degrades to the document origin rather than to a panic.
fn locate(source: &str, range: Option<Range<usize>>) -> Span {
    match range {
        Some(range) => Span {
            start: coordinate(source, range.start),
            end: coordinate(source, range.end),
        },
        None => POINT,
    }
}
fn coordinate(source: &str, offset: usize) -> LineColumn {
    // TOML offsets land on character boundaries; a non-boundary would only shift a column.
    let prefix = source.get(..offset).unwrap_or(source);
    LineColumn {
        line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEAD: &str = "schema = 1\nid = \"demo\"\nkind = \"box\"\nowned = [\"a.rs\"]\n";
    const CRATE: &str = r#"cargo_package = "demo-impl"|path = "impl"|role = "box-contract""#;
    const LISTS: &str = r#"inputs = ["boxology.toml"]|outputs = ["generated/**"]"#;
    const IMPORT: &str = r#"package = "customer"|contract = "customer""#;
    const COMPOSED: &str = "schema = 1\nid = \"demo\"\nkind = \"composition\"\nowned = [\"a\"]\n";
    const BINDING: &str = r#"box = "hello"|capability = "hello.greet"|transport = "in-process""#;
    /// A document whose `[[name]]` section holds one element, a key per `|`-separated field.
    fn section(name: &str, body: &str) -> String {
        let element: String = body.split('|').map(|line| format!("{line}\n")).collect();
        format!("{HEAD}[[{name}]]\n{element}")
    }
    /// The one derived output the tests declare, spelled as `section` splits it.
    fn output() -> String {
        format!(r#"id = "contract"|generator = "boxology-contract"|{LISTS}"#)
    }
    /// A composition package selecting `boxes` and declaring no binding.
    fn composed(boxes: &str) -> String {
        format!("{COMPOSED}[composition]\nboxes = {boxes}\n")
    }
    /// The same document with one binding element, a key per `|`-separated field.
    fn bound(body: &str) -> String {
        let element: String = body.split('|').map(|line| format!("{line}\n")).collect();
        let head = composed("[\"hello\"]");
        format!("{head}[[composition.bindings]]\n{element}")
    }
    fn parse(text: &str) -> Result<Manifest, Diagnostics> {
        let path = RelativePath::new("boxology.toml").expect("test literal is a valid path");
        Manifest::parse(path, text.as_bytes())
    }
    fn path(value: &str) -> RelativePath {
        RelativePath::new(value).expect("test literal is a valid manifest-relative path")
    }
    /// Returns the codes of a rejected document in report order, which is span order per document.
    fn codes(text: &str) -> Vec<Code> {
        match parse(text) {
            Ok(_) => panic!("accepted: {text}"),
            Err(diagnostics) => diagnostics.into_iter().map(Diagnostic::code).collect(),
        }
    }
    fn kinded(kind: &str) -> String {
        format!("schema = 1\nid = \"demo\"\nkind = {kind}\nowned = []\n")
    }
    /// A valid document up to the open bracket of its `owned` list, for one pattern under test.
    const OWNED: &str = "schema = 1\nid = \"d\"\nkind = \"box\"\nowned = [";
    /// Every code this crate emits, ascending. The corpus and the golden below are both driven
    /// from it, so a code that registers nowhere fails loudly instead of going unproven.
    const ALL_CODES: &[Code] = &[
        "BXW0001", "BXW0002", "BXW0003", "BXW0004", "BXW0005", "BXW0006", "BXW0007", "BXW0008",
        "BXW0009", "BXW0010", "BXW0011", "BXW0012", "BXW0013", "BXW0014", "BXW0015", "BXW0016",
        "BXW0017", "BXW0018", "BXW0019", "BXW0020", "BXW0021", "BXW0022", "BXW0023", "BXW0024",
        "BXW0025", "BXW0026", "BXW0027", "BXW0028", "BXW0029", "BXW0030", "BXW0031", "BXW0032",
        "BXW0033", "BXW0034", "BXW0035", "BXW0036", "BXW0037", "BXW0038", "BXW0039", "BXW0040",
        "BXW0041", "BXW0074",
    ];
    const TOP_KEY_NAMES: &[&str] = &[
        "schema",
        "id",
        "kind",
        "owned",
        "display_name",
        "fixtures",
        "protected",
        "quality",
        "crates",
        "derived",
        "imports",
        "composition",
    ];
    /// The one unregistered literal used to prove the rule-table fallback is not a real code.
    const FALLBACK_PROBE: Code = "BXW9999";
    /// One minimal document per code, spelled `<code> <base> <body>` and ordered as `ALL_CODES`
    /// is. Each document provokes its code, so every code is reachable from a real document.
    const CORPUS: &[&str] = &[
        "BXW0001 utf8 -",
        r#"BXW0002 raw schema = 1|id = "a" oops"#,
        r#"BXW0003 raw id = "demo""#,
        "BXW0004 raw schema = 2",
        r#"BXW0005 raw schema = 1|kind = "box"|owned = []"#,
        r#"BXW0006 raw schema = 1|id = "Bad"|kind = "box"|owned = []"#,
        "BXW0007 kind 7",
        r#"BXW0008 kind "provider""#,
        r#"BXW0009 kind "nope""#,
        "BXW0010 head nope = 1",
        "BXW0011 head display_name = 7",
        "BXW0012 head [quality]",
        r#"BXW0013 owned """#,
        r#"BXW0014 owned "/a""#,
        r#"BXW0015 owned "a//b""#,
        r#"BXW0016 owned "../a""#,
        r#"BXW0017 owned "a\tb""#,
        r#"BXW0018 owned "a?""#,
        r#"BXW0019 owned "a**b""#,
        r#"BXW0020 owned "a", "a""#,
        r#"BXW0021 head fixtures = ["f"]"#,
        r#"BXW0022 head [composition]|boxes = ["h"]"#,
        r#"BXW0023 raw schema = 1|id = "d"|kind = "composition"|owned = ["a"]"#,
        r#"BXW0024 imports package = "a"|contract = "b""#,
        r#"BXW0025 imports package = "a"|contract = "a"|[[imports]]|package = "a"|contract = "a""#,
        "BXW0026 head [quality]|commands = []",
        r#"BXW0027 crates cargo_package = "a"|path = "a"|role = "nope""#,
        r#"BXW0028 crates cargo_package = "a"|path = "!a"|role = "platform""#,
        r#"BXW0029 crates cargo_package = "a"|path = "a"|role = "platform"|[[crates]]|cargo_package = "a"|path = "b"|role = "platform""#,
        r#"BXW0030 crates cargo_package = ""|path = "a"|role = "platform""#,
        r#"BXW0031 derived id = "A"|generator = "g"|inputs = ["a"]|outputs = ["b"]"#,
        r#"BXW0032 derived id = "c"|generator = "g"|inputs = ["a"]|outputs = ["b"]|[[derived]]|id = "c"|generator = "g"|inputs = ["a"]|outputs = ["b"]"#,
        r#"BXW0033 derived id = "c"|generator = "G"|inputs = ["a"]|outputs = ["b"]"#,
        "BXW0034 boxes []",
        r#"BXW0035 boxes ["A"]"#,
        r#"BXW0036 boxes ["a", "a"]"#,
        r#"BXW0037 bind box = "hello"|capability = "x"|transport = "http""#,
        r#"BXW0038 bind box = "hello"|capability = "hello.a"|transport = "x""#,
        r#"BXW0039 bind box = "hello"|capability = "hello.a"|transport = "http"|exposure = "x""#,
        r#"BXW0040 bind box = "other"|capability = "other.a"|transport = "http""#,
        r#"BXW0041 bind box = "hello"|capability = "other.a"|transport = "http""#,
        r#"BXW0074 head protected = ["p"]"#,
    ];
    /// The document a corpus entry names: `raw` is the whole of it, `head`, `kind`, `owned`,
    /// `boxes`, and `bind` fill one hole in an otherwise valid one, `utf8` is not text at all,
    /// and every other base names an array-of-tables section holding the body as its elements.
    fn document(base: &str, body: &str) -> Vec<u8> {
        let lines: String = body.split('|').map(|line| format!("{line}\n")).collect();
        let text = match base {
            "utf8" => return vec![0xff],
            "raw" => lines,
            "head" => format!("{HEAD}{lines}"),
            "kind" => kinded(body),
            "owned" => format!("{OWNED}{body}]\n"),
            "boxes" => composed(body),
            "bind" => bound(body),
            name => section(name, body),
        };
        text.into_bytes()
    }
    /// The rendered wording of every code, byte for byte, as `<code> <rule> <source>` per line in
    /// `ALL_CODES` order. Nothing else in this crate pins a rule text or its attribution, so any
    /// edit to either -- including the silent rewording one merged change slipped through -- was
    /// invisible to the suite. A wording diff now shows up here as a diff.
    const EXPECTED: &str = "\
BXW0001 boxology.toml must be valid UTF-8 specs/s5-manifest-and-validation.md D2
BXW0002 boxology.toml must be well-formed TOML specs/s5-manifest-and-validation.md D2
BXW0003 the manifest must declare an integer schema version boxology-details/02-packages.md
BXW0004 this reader supports manifest schema 1 and rejects unknown versions boxology-details/02-packages.md
BXW0005 the manifest must declare a string package id boxology-details/02-packages.md
BXW0006 the package id must match [a-z][a-z0-9-]* boxology-details/02-packages.md
BXW0007 the manifest must declare a string package kind boxology-details/02-packages.md
BXW0008 provider packages are not supported in v0 boxology-details/02-packages.md
BXW0009 the package kind must be box, composition, or platform boxology-details/02-packages.md
BXW0010 schema 1 rejects unknown manifest keys specs/s5-manifest-and-validation.md D2
BXW0011 a known manifest key must hold its declared TOML type specs/s5-manifest-and-validation.md D2
BXW0012 a required manifest key must be present specs/s5-manifest-and-validation.md D2
BXW0013 glob patterns must be non-empty specs/s5-manifest-and-validation.md D2
BXW0014 glob patterns must be relative specs/s5-manifest-and-validation.md D2
BXW0015 glob patterns must not contain empty or . segments specs/s5-manifest-and-validation.md D2
BXW0016 glob patterns must not contain .. segments specs/s5-manifest-and-validation.md D2
BXW0017 glob patterns must not contain backslashes or control characters specs/s5-manifest-and-validation.md D2
BXW0018 the v1 glob dialect supports only * and ** wildcards specs/s5-manifest-and-validation.md D2
BXW0019 ** must stand alone as a complete segment specs/s5-manifest-and-validation.md D2
BXW0020 patterns within one list must be unique specs/s5-manifest-and-validation.md D2
BXW0021 only a platform package may declare fixtures specs/s5-manifest-and-validation.md D2
BXW0022 only composition packages may declare a composition section specs/s5-manifest-and-validation.md D2
BXW0023 a composition package must declare its composition section specs/s5-manifest-and-validation.md D2
BXW0024 v1 imports the package's canonical contract, so contract must equal package boxology-details/02-packages.md
BXW0025 declared import packages must be unique specs/s5-manifest-and-validation.md D2
BXW0026 a quality command must be non-blank text specs/s5-manifest-and-validation.md D2
BXW0027 a crate role must be box-implementation, box-contract, composition, or platform boxology-details/02-packages.md
BXW0028 crate paths must be literal relative paths specs/s5-manifest-and-validation.md D2
BXW0029 crate paths and cargo package names must be unique specs/s5-manifest-and-validation.md D2
BXW0030 cargo package names must be non-empty identifiers specs/s5-manifest-and-validation.md D2
BXW0031 derived output ids must match [a-z][a-z0-9-]* specs/s5-manifest-and-validation.md D2
BXW0032 derived output ids must be unique specs/s5-manifest-and-validation.md D2
BXW0033 generator identities must match [a-z][a-z0-9-]* specs/s5-manifest-and-validation.md D2
BXW0034 this list must contain at least one entry specs/s5-manifest-and-validation.md D2
BXW0035 box references must match [a-z][a-z0-9-]* specs/s5-manifest-and-validation.md D2
BXW0036 selected boxes must be unique specs/s5-manifest-and-validation.md D2
BXW0037 binding capabilities must be box-qualified names specs/s5-manifest-and-validation.md D2
BXW0038 binding transport must be in-process or http specs/s5-manifest-and-validation.md D2
BXW0039 binding exposure must be code_only, internal, or external specs/s5-manifest-and-validation.md D2
BXW0040 every binding must reference a selected box specs/s5-manifest-and-validation.md D2
BXW0041 a binding capability must be qualified by its own box specs/s5-manifest-and-validation.md D2
BXW0074 only a platform package may declare protected control-plane paths specs/s5-manifest-and-validation.md D2
";
    #[test]
    fn rule_text_and_sources_are_locked() {
        // The glob dialect keeps its own rule table in `glob.rs`, so `rule_of` has no arm for
        // BXW0013-BXW0019: locking what each code actually reports covers both tables at once,
        // and rejects the generic fallback a code with no arm of its own would render.
        let generic = rule_of(FALLBACK_PROBE);
        let mut rendered = String::new();
        for spec in CORPUS {
            let (code, rest) = spec.split_once(' ').expect("a corpus entry names its code");
            let (base, body) = rest.split_once(' ').expect("a corpus entry names its base");
            let path = RelativePath::new("boxology.toml").expect("test literal is a valid path");
            let Err(defects) = Manifest::parse(path, &document(base, body)) else {
                panic!("accepted: {spec}");
            };
            let Some(found) = defects.into_iter().find(|d| d.code() == code) else {
                panic!("{spec} reported {defects}");
            };
            assert_ne!(found.rule(), generic, "{code} renders the generic fallback");
            let line = format!("{code} {} {}\n", found.rule(), found.rule_source());
            rendered.push_str(&line);
        }
        assert_eq!(rendered, EXPECTED);
    }
    #[test]
    fn corpus_covers_every_code() {
        // Comparing the two ordered lists proves both directions at once: no code without a
        // document that provokes it, and no document for a code this crate does not emit.
        let covered: Vec<&str> = CORPUS.iter().map(|spec| &spec[..7]).collect();
        assert_eq!(covered, ALL_CODES);
        assert!(ALL_CODES.windows(2).all(|pair| pair[0] < pair[1]));
    }
    fn production_rust_sources(root: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
        let mut pending = vec![root.join("crates")];
        let mut sources = Vec::new();
        while let Some(directory) = pending.pop() {
            let entries = std::fs::read_dir(&directory)
                .unwrap_or_else(|error| panic!("cannot read {directory:?}: {error}"));
            for entry in entries {
                let entry = entry.expect("a crate source directory entry is readable");
                let path = entry.path();
                let kind = entry
                    .file_type()
                    .expect("a crate source directory entry has a type");
                if kind.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs")
                    && path
                        .components()
                        .any(|component| component.as_os_str() == "src")
                {
                    let source = std::fs::read_to_string(&path)
                        .unwrap_or_else(|error| panic!("cannot read {path:?}: {error}"));
                    sources.push((path, source));
                }
            }
        }
        sources.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        sources
    }
    fn bxw0074_is_exclusive(
        sources: &[(std::path::PathBuf, String)],
        allowed: &std::path::Path,
    ) -> bool {
        let needle = format!("{}BXW0074{}", '"', '"');
        sources
            .iter()
            .filter(|(_, source)| source.contains(&needle))
            .map(|(path, _)| path.as_path())
            .eq([allowed])
    }
    fn reserved_codes_are(source: &str, reserved: &[String], expected: &[String]) -> bool {
        let needle = format!("{}BXW", '"');
        let mut actual = Vec::new();
        for line in source
            .lines()
            .filter(|line| line.trim_start().starts_with("const "))
        {
            for (at, _) in line.match_indices(&needle) {
                let Some(code) = line.get(at + 1..at + 8) else {
                    return false;
                };
                if line.as_bytes().get(at + 8) == Some(&b'"')
                    && reserved.iter().any(|reserved| reserved == code)
                {
                    actual.push(code);
                }
            }
        }
        actual.sort_unstable();
        actual
            .iter()
            .copied()
            .eq(expected.iter().map(String::as_str))
    }
    fn cli_allocations_are_exact(owners: &[(&str, &str)], reserved: &[String]) -> bool {
        let expected = [
            ("walk.rs", &reserved[19..22]),
            ("generate.rs", &reserved[22..28]),
            ("execute.rs", &reserved[28..32]),
        ];
        owners.len() == expected.len()
            && owners
                .iter()
                .zip(expected)
                .all(|((name, source), (expected_name, codes))| {
                    name == &expected_name && reserved_codes_are(source, reserved, codes)
                })
    }
    #[test]
    fn repository_bxw_allocations_are_disjoint() {
        // Workspace owns 0042-0060; CLI walk owns 0061-0063, generation planning 0064-0069,
        // and execution 0070-0073. The manifest may resume only at 0074.
        let reserved: Vec<String> = (42..=73)
            .map(|number| format!("BX{}{number:04}", 'W'))
            .collect();
        assert!(
            ALL_CODES
                .iter()
                .all(|code| !reserved.iter().any(|reserved| reserved == code))
        );
        assert_eq!(ALL_CODES.last(), Some(&"BXW0074"));

        let workspace = include_str!("../../boxology-workspace/src/lib.rs");
        assert!(reserved_codes_are(workspace, &reserved, &reserved[..19]));
        let cli = [
            ("walk.rs", include_str!("../../boxology-cli/src/walk.rs")),
            (
                "generate.rs",
                include_str!("../../boxology-cli/src/generate.rs"),
            ),
            (
                "execute.rs",
                include_str!("../../boxology-cli/src/execute.rs"),
            ),
        ];
        assert!(
            cli_allocations_are_exact(&cli, &reserved),
            "CLI BXW allocation owners or ranges changed"
        );

        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(std::path::Path::parent)
            .expect("the manifest crate is under the repository's crates directory");
        let allowed = manifest.join("src/parse.rs");
        let sources = production_rust_sources(root);
        assert!(
            bxw0074_is_exclusive(&sources, &allowed),
            "a production Rust source outside the manifest parser claims BXW0074"
        );
    }
    #[test]
    fn cli_allocation_owner_mutations_are_rejected() {
        let reserved: Vec<String> = (42..=73)
            .map(|number| format!("BX{}{number:04}", 'W'))
            .collect();
        let walk = include_str!("../../boxology-cli/src/walk.rs");
        let generate = include_str!("../../boxology-cli/src/generate.rs");
        let execute = include_str!("../../boxology-cli/src/execute.rs");
        let wrong_walk = format!(
            "{walk}\nconst WRONG_OWNER: Rule = (\"{}\", ROOT_TEXT, RULE_SOURCE);\n",
            reserved[22]
        );
        assert!(
            !cli_allocations_are_exact(
                &[
                    ("walk.rs", wrong_walk.as_str()),
                    ("generate.rs", generate),
                    ("execute.rs", execute),
                ],
                &reserved,
            ),
            "a generation code allocated by walk.rs survived"
        );
        assert!(
            !cli_allocations_are_exact(&[("walk.rs", walk), ("execute.rs", execute)], &reserved,),
            "a missing generate.rs owner survived"
        );
    }
    #[test]
    fn repository_bxw_allocation_scan_catches_third_crate_mutation() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest
            .parent()
            .and_then(std::path::Path::parent)
            .expect("the manifest crate is under the repository's crates directory");
        let allowed = manifest.join("src/parse.rs");
        let mut sources = production_rust_sources(root);
        sources.push((
            root.join("crates/third/src/lib.rs"),
            String::from(r#"const COLLISION: &str = "BXW0074";"#),
        ));
        assert!(
            !bxw0074_is_exclusive(&sources, &allowed),
            "a third-crate allocation collision survived the repository scan"
        );
    }
    fn module_names(source: &str) -> Vec<&str> {
        let mut tokens = source
            .split(|character: char| {
                !(character.is_alphanumeric() || matches!(character, '_' | '#'))
            })
            .filter(|token| !token.is_empty());
        let mut modules = Vec::new();
        while let Some(token) = tokens.next() {
            if token == "mod" {
                modules.push(tokens.next().unwrap_or(""));
            }
        }
        modules
    }
    fn inventory_matches(actual: &str, expected: &[&str]) -> bool {
        actual.split(' ').eq(expected.iter().copied())
    }
    #[test]
    fn all_codes_is_exhaustive() {
        // Every source file's whole text, read at compile time: a code emitted anywhere in the
        // crate but registered nowhere above fails here rather than drifting in unproven. There is
        // no test-module cut, so production appended below a test module remains visible; the one
        // fallback probe is excluded by name rather than by narrowing the scanned source.
        // The module inventory scans Rust-like tokens, so visibility and whitespace cannot hide a
        // declaration. Comments or literals that spell a standalone `mod` fail loudly too.
        let root = include_str!("lib.rs");
        let declared = module_names(root);
        assert_eq!(
            declared,
            ["glob", "parse", "tests"],
            "the crate's source inventory changed"
        );
        let sources = [
            ("lib", root),
            ("parse", include_str!("parse.rs")),
            ("glob", include_str!("glob.rs")),
        ];
        let source_names: Vec<&str> = sources.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            source_names,
            ["lib", "parse", "glob"],
            "the source-file inventory changed"
        );
        for (index, (_, source)) in sources.iter().enumerate() {
            assert!(
                !sources[..index]
                    .iter()
                    .any(|(_, previous)| previous == source),
                "a source file appears more than once"
            );
        }
        let needle = format!("{}BXW", '"');
        let mut seen: Vec<&str> = Vec::new();
        for (_, source) in sources {
            for (at, _) in source.match_indices(needle.as_str()) {
                let code = &source[at + 1..at + 8];
                if code != FALLBACK_PROBE && !seen.contains(&code) {
                    seen.push(code);
                }
            }
        }
        seen.sort_unstable();
        assert_eq!(seen, ALL_CODES);
    }
    #[test]
    fn module_inventory_catches_public_extra_mutation() {
        let root = include_str!("lib.rs");
        let mutated = root.replacen("\nmod parse;", "\npub mod extra;", 1);
        assert_eq!(
            module_names(&mutated),
            ["glob", "extra", "tests"],
            "a visibility-qualified module must remain in the source inventory"
        );
    }
    #[test]
    fn module_inventory_catches_multiline_extra_mutation() {
        let root = include_str!("lib.rs");
        let mutated = root.replacen("\nmod parse;", "\nmod\nextra;", 1);
        assert_eq!(
            module_names(&mutated),
            ["glob", "extra", "tests"],
            "whitespace between `mod` and its name must not narrow the source inventory"
        );
    }
    #[test]
    fn manifest_key_inventories_are_exact() {
        let inventories: &[(&str, &str, &[&str])] = &[
            ("TOP_KEYS", TOP_KEYS, TOP_KEY_NAMES),
            ("QUALITY_KEYS", QUALITY_KEYS, &["commands"]),
            ("COMPOSITION_KEYS", COMPOSITION_KEYS, &["boxes", "bindings"]),
            ("CRATE_KEYS", CRATE_KEYS, &["cargo_package", "path", "role"]),
            (
                "DERIVED_KEYS",
                DERIVED_KEYS,
                &["id", "generator", "inputs", "outputs"],
            ),
            ("IMPORT_KEYS", IMPORT_KEYS, &["package", "contract"]),
            (
                "BINDING_KEYS",
                BINDING_KEYS,
                &["box", "capability", "transport", "exposure"],
            ),
        ];
        for (name, actual, expected) in inventories {
            assert!(
                inventory_matches(actual, expected),
                "{name} changed: {actual:?}"
            );
        }
    }
    #[test]
    fn manifest_key_inventory_catches_duplicate_protected_mutation() {
        let duplicated = TOP_KEYS.replacen("protected", "protected protected", 1);
        assert!(
            !inventory_matches(&duplicated, TOP_KEY_NAMES),
            "a duplicate protected entry must change the inventory"
        );
    }
    #[test]
    fn hello_fixture_parses_green() {
        // The repository's own fixture, byte for byte at compile time: the green path is proven
        // against a real document, and reading it costs no filesystem access at runtime.
        let bytes = include_bytes!("../../fixtures/hello/boxology.toml");
        let path = RelativePath::new("boxology.toml").expect("test literal is a valid path");
        let valid = match Manifest::parse(path, bytes) {
            Ok(valid) => valid,
            Err(defects) => panic!("the hello fixture is rejected:\n{defects}"),
        };
        assert_eq!(valid.id().as_str(), "hello");
        assert_eq!(valid.kind(), Kind::Box);
        assert_eq!(valid.owned().len(), 2);
        assert_eq!(valid.owned()[1].as_str(), "implementation/**");
        assert_eq!(valid.quality_commands().len(), 3);
        assert_eq!(valid.crates().len(), 2);
        assert_eq!(valid.crates()[1].role(), CrateRole::BoxContract);
        assert_eq!(valid.derived().len(), 1);
        assert_eq!(valid.derived()[0].outputs().len(), 3);
        assert!(valid.fixtures().is_empty() && valid.imports().is_empty());
        assert!(valid.composition().is_none());
    }
    #[test]
    fn schema_gate_precedes_field_checks() {
        // A stop-the-world gate reports alone: the first case also breaks three schema-1 rules.
        for (text, code) in [
            ("schema = 2\nid = \"Bad_Id\"\nnope = 1\n", "BXW0004"),
            ("schema = 0\n", "BXW0004"),
            ("id = \"demo\"\n", "BXW0003"),
            ("schema = \"1\"\nnope = 1\n", "BXW0003"),
            ("schema = 1\nid = \"a\" oops\n", "BXW0002"),
            ("schema = 1\nid = \"a\"\nid = \"b\"\n", "BXW0002"),
        ] {
            assert_eq!(codes(text), [code], "{text}");
        }
        let path = RelativePath::new("boxology.toml").expect("test literal is a valid path");
        let Err(invalid) = Manifest::parse(path, &[0xff]) else {
            panic!("non-UTF-8 bytes were accepted");
        };
        let rendered = invalid.to_string();
        assert!(rendered.starts_with("BXW0001 boxology.toml:1:1-1:1"));
        // Spans count characters, not bytes; the report sorts by span, not detection order.
        let text = "schema = 1\nid = \"é\"\nkind = 7\n";
        assert_eq!(codes(text), ["BXW0012", "BXW0006", "BXW0007"]);
        let located = parse(text).expect_err("two field defects").to_string();
        assert!(located.contains("BXW0006 boxology.toml:2:6-2:9"));
    }
    #[test]
    fn unknown_keys_reject_at_every_level() {
        // `[composition]` was the last unmodelled section and left its pin here; it is modelled
        // now, so that case is gone rather than flipped. The last two keys are prefix collisions
        // in both directions: the match is equality, so no near-miss of a known key is accepted.
        for extra in ["nope = 1\n", "import = 1\n", "schemas = 1\n"] {
            assert_eq!(codes(&format!("{HEAD}{extra}")), ["BXW0010"], "{extra}");
        }
        let hostile = parse("schema = 1\n\"a b\\u000A\" = 1\n").expect_err("unknown key");
        let rendered = hostile.to_string();
        assert!(rendered.contains("BXW0010"), "{rendered}");
        assert!(!rendered.contains("a b"), "{rendered}");
        assert_eq!(rendered.lines().count(), hostile.as_slice().len());
    }
    #[test]
    fn kind_vocabulary_and_provider_rejection() {
        assert_eq!(codes(&kinded("\"provider\"")), ["BXW0008"]);
        assert_eq!(codes(&kinded("\"Box\"")), ["BXW0009"]);
        assert_eq!(codes(&kinded("\"\"")), ["BXW0009"]);
        assert_eq!(codes(&kinded("7")), ["BXW0007"]);
        assert_eq!(codes("schema = 1\nid = \"d\"\nowned = []\n"), ["BXW0007"]);
        // `owned = []` parses: the unowned-package check is T2 classification, not parsing.
        for (kind, expected) in [("box", Kind::Box), ("platform", Kind::Platform)] {
            let text = kinded(&format!("\"{kind}\""));
            assert_eq!(parse(&text).expect("valid manifest").kind(), expected);
        }
        let text = composed("[\"hello\"]");
        assert_eq!(parse(&text).expect("valid").kind(), Kind::Composition);
    }
    #[test]
    fn id_grammar_is_payload_safe() {
        for id in ["payload_id", "9payload", "", "payload.x", "payloadé"] {
            let text = format!("schema = 1\nid = \"{id}\"\nkind = \"box\"\nowned = []\n");
            let rendered = parse(&text).expect_err("invalid id").to_string();
            assert_eq!(codes(&text), ["BXW0006"], "{id}");
            assert!(!rendered.contains("payload"), "{rendered}");
        }
        let absent = "schema = 1\nkind = \"box\"\nowned = []\n";
        let wrong = "schema = 1\nid = 7\nkind = \"box\"\nowned = []\n";
        assert_eq!(codes(absent), ["BXW0005"]);
        assert_eq!(codes(wrong), ["BXW0005"]);
        assert_eq!(parse(HEAD).expect("valid").id().as_str(), "demo");
    }
    #[test]
    fn owned_patterns_rules() {
        let head = "schema = 1\nid = \"demo\"\nkind = \"box\"\n";
        assert_eq!(codes(head), ["BXW0012"]);
        assert_eq!(codes(&format!("{head}owned = \"a.rs\"\n")), ["BXW0011"]);
        // Duplicates are BXW0020 and a dialect violation keeps its PR1 code; report order is span
        // order, so one list's entries come out left to right.
        let mixed = format!("{head}owned = [\"a.rs\", \"a.rs\", \"../x\", 7]\n");
        assert_eq!(codes(&mixed), ["BXW0020", "BXW0016", "BXW0011"]);
        let valid = parse(HEAD).expect("valid manifest");
        assert_eq!(valid.owned().len(), 1);
        assert_eq!(valid.owned()[0].as_str(), "a.rs");
        let many = "schema = 1\nid = \"d\"\nkind = \"box\"\nowned = [\"a\", \"b/**\"]\n";
        assert_eq!(parse(many).expect("valid").owned().len(), 2);
    }
    #[test]
    fn display_name_is_optional_and_typed() {
        assert_eq!(parse(HEAD).expect("valid").display_name(), None);
        let named = format!("{HEAD}display_name = \"Demo Box\"\n");
        let valid = parse(&named).expect("a display name is optional, not unknown");
        assert_eq!(valid.display_name(), Some("Demo Box"));
        // A display name is free-form text, so its typed rejection describes the key and no more.
        let wrong = format!("{HEAD}display_name = [\"payload\"]\n");
        assert_eq!(codes(&wrong), ["BXW0011"]);
        let rendered = parse(&wrong).expect_err("wrong type").to_string();
        assert!(rendered.contains("key display_name\""), "{rendered}");
        assert!(!rendered.contains("payload"), "{rendered}");
    }
    #[test]
    fn fixtures_are_platform_only_and_non_empty() {
        let platform = "schema = 1\nid = \"p\"\nkind = \"platform\"\nowned = [\"a\"]\n";
        let declared = format!("{platform}fixtures = [\"f/**\", \"g\"]\n");
        let valid = parse(&declared).expect("a platform package may declare fixtures");
        assert_eq!(valid.fixtures().len(), 2);
        assert_eq!(valid.fixtures()[0].as_str(), "f/**");
        assert!(parse(platform).expect("valid").fixtures().is_empty());
        // Off a platform package the key itself is the defect, whatever it holds.
        let section = "[composition]\nboxes = [\"h\"]\n";
        for (kind, tail) in [("box", ""), ("composition", section)] {
            let head = format!("schema = 1\nid = \"p\"\nkind = \"{kind}\"\nowned = [\"a\"]\n");
            let text = format!("{head}fixtures = [\"f/**\"]\n{tail}");
            assert_eq!(codes(&text), ["BXW0021"], "{kind}");
        }
        assert_eq!(codes(&format!("{platform}fixtures = []\n")), ["BXW0034"]);
        assert_eq!(codes(&format!("{platform}fixtures = \"f\"\n")), ["BXW0011"]);
        // The list rules are `owned`'s: the dialect and the duplicate check apply unchanged.
        let repeated = format!("{platform}fixtures = [\"f/**\", \"f/**\"]\n");
        assert_eq!(codes(&repeated), ["BXW0020"]);
        let escaping = format!("{platform}fixtures = [\"../x\"]\n");
        assert_eq!(codes(&escaping), ["BXW0016"]);
    }
    #[test]
    fn protected_control_plane_declarations_are_platform_only_and_glob_validated() {
        let platform = "schema = 1\nid = \"p\"\nkind = \"platform\"\nowned = [\"a\"]\n";
        let declared =
            format!("{platform}protected = [\".github/workflows/pr.yml\", \"crates/xtask/**\"]\n");
        let valid = parse(&declared).expect("platform protected declarations are valid");
        let protected = valid.protected();
        assert_eq!(
            protected
                .iter()
                .map(GlobPattern::as_str)
                .collect::<Vec<_>>(),
            vec![".github/workflows/pr.yml", "crates/xtask/**"]
        );
        assert!(protected[0].matches(&path(".github/workflows/pr.yml")));
        assert!(!protected[0].matches(&path(".github/workflows/other.yml")));
        assert!(protected[1].matches(&path("crates/xtask/src/main.rs")));
        assert!(!protected[1].matches(&path("crates/xtask")));
        assert!(
            parse(platform)
                .expect("protected is optional")
                .protected()
                .is_empty()
        );

        let box_head = "schema = 1\nid = \"b\"\nkind = \"box\"\nowned = [\"a\"]\n";
        assert_eq!(
            codes(&format!("{box_head}protected = [\"p\"]\n")),
            ["BXW0074"]
        );
        let composition =
            format!("{COMPOSED}protected = [\"p\"]\n[composition]\nboxes = [\"hello\"]\n");
        assert_eq!(codes(&composition), ["BXW0074"]);
        let rendered = parse(&format!("{box_head}protected = [\"p\"]\n"))
            .expect_err("box protected declaration")
            .to_string();
        assert!(
            rendered
                .starts_with("BXW0074 boxology.toml:5:1-5:10 offending=\"manifest key protected\""),
            "{rendered}"
        );
        assert!(
            rendered.ends_with(&format!("source={D2_SOURCE:?}")),
            "{rendered}"
        );

        let empty_box = format!("{box_head}protected = []\n");
        assert_eq!(codes(&empty_box), ["BXW0034", "BXW0074"]);
        let empty_composition =
            format!("{COMPOSED}protected = []\n[composition]\nboxes = [\"hello\"]\n");
        assert_eq!(codes(&empty_composition), ["BXW0034", "BXW0074"]);
        assert_eq!(codes(&format!("{platform}protected = []\n")), ["BXW0034"]);
        let empty_rendered = parse(&empty_box)
            .expect_err("empty protected declaration")
            .to_string();
        for code in ["BXW0034", "BXW0074"] {
            assert!(
                empty_rendered.contains(&format!("{code} boxology.toml:5:1-5:10")),
                "{empty_rendered}"
            );
        }

        assert_eq!(codes(&format!("{platform}protected = 7\n")), ["BXW0011"]);
        assert_eq!(
            codes(&format!("{platform}protected = [\"a\", \"a\"]\n")),
            ["BXW0020"]
        );
        for (literal, code) in [
            (r#""/a""#, "BXW0014"),
            (r#""../a""#, "BXW0016"),
            (r#""a\u001bx""#, "BXW0017"),
            (r#""a?""#, "BXW0018"),
            (r#""a[b]""#, "BXW0018"),
            (r#""!a""#, "BXW0018"),
            (r#""a**b""#, "BXW0019"),
        ] {
            assert_eq!(
                codes(&format!("{platform}protected = [{literal}]\n")),
                [code],
                "{literal}"
            );
        }
        let upward = format!("{platform}protected = [\"../x\"]\n");
        let rendered = parse(&upward).expect_err("protected glob span").to_string();
        assert!(
            rendered.starts_with("BXW0016 boxology.toml:5:14-5:20 offending=\"glob pattern\""),
            "{rendered}"
        );
    }
    #[test]
    fn quality_commands_reject_empty_and_blank() {
        let text = format!("{HEAD}[quality]\ncommands = [\"cargo test\", \"cargo fmt\"]\n");
        let valid = parse(&text).expect("a quality table with commands is valid");
        assert_eq!(valid.quality_commands(), ["cargo test", "cargo fmt"]);
        // The table is optional; the key is required once the table is declared.
        assert!(parse(HEAD).expect("valid").quality_commands().is_empty());
        assert_eq!(codes(&format!("{HEAD}[quality]\n")), ["BXW0012"]);
        for list in ["[]", "[\"\"]", "[\"   \"]", "[\"cargo test\", \"\\t\\n\"]"] {
            let text = format!("{HEAD}[quality]\ncommands = {list}\n");
            assert_eq!(codes(&text), ["BXW0026"], "{list}");
        }
        for list in ["7", "[7]", "\"cargo test\""] {
            let text = format!("{HEAD}[quality]\ncommands = {list}\n");
            assert_eq!(codes(&text), ["BXW0011"], "{list}");
        }
        // Free-form values never reach the report. Assert what the payload IS rather than what
        // it is not: an absence check passes vacuously whenever the value had no path there.
        let rendered = parse(&format!("{HEAD}quality = 7\n"))
            .expect_err("table")
            .to_string();
        assert!(
            rendered.contains("offending=\"manifest key quality\""),
            "{rendered}"
        );
        let blank = format!("{HEAD}[quality]\ncommands = [\"\\t\\n\"]\n");
        let rendered = parse(&blank).expect_err("blank command").to_string();
        assert!(
            rendered.contains("offending=\"manifest key commands\""),
            "{rendered}"
        );
    }
    #[test]
    fn crate_roles_paths_and_names() {
        let valid = parse(&section("crates", CRATE)).expect("a declared crate");
        assert_eq!(valid.crates().len(), 1);
        assert_eq!(valid.crates()[0].cargo_package(), "demo-impl");
        assert_eq!(valid.crates()[0].path().as_str(), "impl");
        assert_eq!(valid.crates()[0].role(), CrateRole::BoxContract);
        assert!(parse(HEAD).expect("valid").crates().is_empty());
        let one = |name: &str, path: &str, role: &str| {
            let body = format!(r#"cargo_package = "{name}"|path = "{path}"|role = "{role}""#);
            section("crates", &body)
        };
        // The role vocabulary is closed and case-sensitive, and is never inferred from a path.
        for role in ["Box-Implementation", "implementation", "box_contract", ""] {
            assert_eq!(codes(&one("a", "x", role)), ["BXW0027"], "{role}");
        }
        for role in ["box-implementation", "composition", "platform"] {
            assert!(parse(&one("a", "a", role)).is_ok(), "{role}");
        }
        // A crate path is a literal directory: no metacharacter, no escape, no absolute path.
        for path in ["g/*", "g?", "g[a]", "!g", "../x", "/a", "a/./b", "a/", ""] {
            assert_eq!(codes(&one("a", path, "platform")), ["BXW0028"], "{path}");
        }
        for name in ["", "bad name", "bad.name", "béta"] {
            assert_eq!(codes(&one(name, "a", "platform")), ["BXW0030"], "{name}");
        }
        // Two crates may share a role, but neither a Cargo name nor a directory.
        let twin = |name: &str, path: &str| {
            let entry = format!(r#"cargo_package = "{name}"|path = "{path}"|role = "platform""#);
            section("crates", &format!("{CRATE}|[[crates]]|{entry}"))
        };
        assert_eq!(codes(&twin("demo-impl", "other")), ["BXW0029"]);
        assert_eq!(codes(&twin("other", "impl")), ["BXW0029"]);
        // A clash reports at the offending element's own key -- not at that element's header, and
        // not at the first occurrence -- so each identity names the one it repeated.
        let named = parse(&twin("demo-impl", "x"))
            .expect_err("name")
            .to_string();
        let at = r#"BXW0029 boxology.toml:10:1-10:14 offending="manifest key cargo_package""#;
        assert!(named.starts_with(at), "{named}");
        let reused = parse(&twin("x", "impl")).expect_err("path").to_string();
        let at = r#"BXW0029 boxology.toml:11:1-11:5 offending="manifest key path""#;
        assert!(reused.starts_with(at), "{reused}");
        let twins = parse(&twin("o", "o")).expect("distinct twins");
        assert_eq!(twins.crates()[1].path().as_str(), "o");
        // Every field is required, and holds a declared type.
        let absent = section("crates", r#"path = "a"|role = "platform""#);
        let mistyped = section("crates", r#"cargo_package = 7|path = "a""#);
        assert_eq!(codes(&absent), ["BXW0012"]);
        assert_eq!(codes(&mistyped), ["BXW0012", "BXW0011"]);
        // An absent key has no span of its own, so it is located at the element it is missing from.
        let rendered = parse(&absent).expect_err("absent key").to_string();
        let located = "BXW0012 boxology.toml:5:1-5:11";
        assert!(rendered.starts_with(located), "{rendered}");
        // Shape confusion is a typed defect, as `[[quality]]` is; an inline element is equivalent.
        for shape in ["[crates]\npath = \"a\"\n", "crates = [1]\n", "crates = 7\n"] {
            assert_eq!(codes(&format!("{HEAD}{shape}")), ["BXW0011"], "{shape}");
        }
        let inline = format!("{HEAD}crates = [{{ {} }}]\n", CRATE.replace('|', ", "));
        let valid = parse(&inline).expect("an inline element is equivalent TOML");
        assert_eq!(valid.crates()[0].role(), CrateRole::BoxContract);
        // The crate-role vocabulary is 02-packages', and a rejected value is named, not echoed.
        let unknown = one("a", "a", "payload");
        let rendered = parse(&unknown).expect_err("unknown role").to_string();
        let source = r#"source="boxology-details/02-packages.md""#;
        let named = r#"BXW0027 boxology.toml:8:1-8:5 offending="manifest key role""#;
        assert!(rendered.contains(source), "{rendered}");
        assert!(rendered.starts_with(named), "{rendered}");
        assert!(!rendered.contains("payload"), "{rendered}");
    }
    #[test]
    fn derived_ids_generators_and_pattern_lists() {
        let valid = parse(&section("derived", &output())).expect("a declared output");
        assert_eq!(valid.derived().len(), 1);
        assert_eq!(valid.derived()[0].id().as_str(), "contract");
        assert_eq!(valid.derived()[0].generator(), "boxology-contract");
        assert_eq!(valid.derived()[0].inputs()[0].as_str(), "boxology.toml");
        assert_eq!(valid.derived()[0].outputs()[0].as_str(), "generated/**");
        assert!(parse(HEAD).expect("valid").derived().is_empty());
        // An output id and a generator identity are both the package-id grammar.
        let named = |id: &str, generator: &str| {
            let body = format!(r#"id = "{id}"|generator = "{generator}"|{LISTS}"#);
            section("derived", &body)
        };
        for id in ["Contract", "0contract", "contract_x", ""] {
            assert_eq!(codes(&named(id, "g")), ["BXW0031"], "{id}");
        }
        for generator in ["Boxology", "boxology_contract", ""] {
            assert_eq!(codes(&named("c", generator)), ["BXW0033"], "{generator}");
        }
        let twice = format!("{}|[[derived]]|{}", output(), output());
        assert_eq!(codes(&section("derived", &twice)), ["BXW0032"]);
        // Both lists are required and non-empty, and each entry keeps the dialect's own codes.
        for (lists, code) in [
            (r#"outputs = ["a"]"#, "BXW0012"),
            (r#"inputs = ["a"]"#, "BXW0012"),
            (r#"inputs = []|outputs = ["a"]"#, "BXW0034"),
            (r#"inputs = ["a"]|outputs = []"#, "BXW0034"),
            (r#"inputs = "a"|outputs = ["a"]"#, "BXW0011"),
            (r#"inputs = ["../x"]|outputs = ["a"]"#, "BXW0016"),
            (r#"inputs = ["a", "a"]|outputs = ["b"]"#, "BXW0020"),
        ] {
            let body = format!(r#"id = "c"|generator = "g"|{lists}"#);
            assert_eq!(codes(&section("derived", &body)), [code], "{lists}");
        }
        // An element declaring none of its keys is coded once per key, not once per element.
        assert_eq!(codes(&section("derived", "")), ["BXW0012"; 4]);
        // Shape confusion is a typed defect here as it is for `[[crates]]`: a section that is not
        // an array of tables, an entry that is not a table, and the plain-table spelling all
        // reject; the inline-element spelling is equivalent TOML and parses.
        for shape in [
            "derived = 7\n",
            "derived = [1]\n",
            "[derived]\nid = \"c\"\n",
        ] {
            assert_eq!(codes(&format!("{HEAD}{shape}")), ["BXW0011"], "{shape}");
        }
        let inline = format!("{HEAD}derived = [{{ {} }}]\n", output().replace('|', ", "));
        let valid = parse(&inline).expect("an inline element is equivalent TOML");
        assert_eq!(valid.derived()[0].id().as_str(), "contract");
        // A rejected generator is reported by key name, never by value.
        let rendered = parse(&named("c", "payload_x")).expect_err("id").to_string();
        let key = r#"BXW0033 boxology.toml:7:1-7:10 offending="manifest key generator""#;
        assert!(rendered.starts_with(key), "{rendered}");
        assert!(!rendered.contains("payload"), "{rendered}");
    }
    #[test]
    fn imports_require_canonical_contract_and_uniqueness() {
        let valid = parse(&section("imports", IMPORT)).expect("an import");
        assert_eq!(valid.imports().len(), 1);
        assert_eq!(valid.imports()[0].package().as_str(), "customer");
        assert!(parse(HEAD).expect("valid").imports().is_empty());
        let pair = |package: &str, contract: &str| {
            let body = format!(r#"package = "{package}"|contract = "{contract}""#);
            section("imports", &body)
        };
        // V1 imports the package's canonical contract and has no parallel-surface selector, so
        // every other contract is a defect and `contract` is required rather than defaulted.
        for wrong in ["supplier", "Customer", "customer-x", ""] {
            assert_eq!(codes(&pair("customer", wrong)), ["BXW0024"], "{wrong}");
        }
        assert_eq!(codes(&section("imports", r#"package = "c""#)), ["BXW0012"]);
        let unnamed = section("imports", r#"contract = "customer""#);
        assert_eq!(codes(&unnamed), ["BXW0012"]);
        // An import package is the package-id grammar, and each package is imported at most once.
        for package in ["Customer", "0customer", "customer_x", ""] {
            assert_eq!(codes(&pair(package, package)), ["BXW0006"], "{package}");
        }
        let twice = format!("{IMPORT}|[[imports]]|{IMPORT}");
        assert_eq!(codes(&section("imports", &twice)), ["BXW0025"]);
        // A clash reports at the offending element's own `package` key -- not at that element's
        // header, and not at the first occurrence -- so every later duplicate names itself.
        let rendered = parse(&section("imports", &twice))
            .expect_err("dup")
            .to_string();
        let at = r#"BXW0025 boxology.toml:9:1-9:8 offending="manifest key package""#;
        assert!(rendered.starts_with(at), "{rendered}");
        let thrice = format!("{twice}|[[imports]]|{IMPORT}");
        assert_eq!(codes(&section("imports", &thrice)), ["BXW0025", "BXW0025"]);
        // An array-of-tables section is a container, so an empty one declares nothing and reads
        // exactly as omitting it, like `crates` and `derived`. A list whose presence is itself a
        // claim rejects empty instead: `fixtures` claims a platform privilege, `inputs`/`outputs`
        // claim input completeness, `commands` claims a quality entry point. `owned` is the
        // deliberate exception -- an unowned package is T2 classification, not a parse defect.
        let empty = format!("{HEAD}imports = []\n");
        assert!(parse(&empty).expect("none declared").imports().is_empty());
        assert_eq!(codes(&section("imports", "")), ["BXW0012", "BXW0012"]);
        // The same shape edges `[[crates]]` and `[[derived]]` cover: only an array of tables, or
        // its equivalent inline spelling, is a section; every other shape is a typed defect.
        for shape in [
            "imports = 7\n",
            "imports = [1]\n",
            "[imports]\npackage = \"c\"\n",
        ] {
            assert_eq!(codes(&format!("{HEAD}{shape}")), ["BXW0011"], "{shape}");
        }
        let inline = format!("{HEAD}imports = [{{ {} }}]\n", IMPORT.replace('|', ", "));
        let valid = parse(&inline).expect("an inline element is equivalent TOML");
        assert_eq!(valid.imports()[0].package().as_str(), "customer");
        // The canonical-contract rule is 02-packages', and names the key, never the value.
        let unequal = pair("customer", "payload");
        let rendered = parse(&unequal).expect_err("contract").to_string();
        let key = r#"BXW0024 boxology.toml:7:1-7:9 offending="manifest key contract""#;
        let source = r#"source="boxology-details/02-packages.md""#;
        assert!(rendered.starts_with(key), "{rendered}");
        assert!(rendered.ends_with(source), "{rendered}");
        assert!(!rendered.contains("payload"), "{rendered}");
    }
    #[test]
    fn array_of_tables_reject_unknown_nested_keys() {
        // `ArrayOfTables` is not `TableLike`, so no signature demands the per-element inventory
        // check: one case per section, each located at the offending key, not its section header.
        for (name, body, line) in [
            ("crates", CRATE, 9),
            ("derived", &output(), 10),
            ("imports", IMPORT, 8),
        ] {
            let text = format!("{}nope = 1\n", section(name, body));
            assert_eq!(codes(&text), ["BXW0010"], "{name}");
            let rendered = parse(&text).expect_err("unknown key").to_string();
            let key = "offending=\"manifest key nope\"";
            let located = format!("BXW0010 boxology.toml:{line}:1-{line}:5 {key}");
            assert!(rendered.starts_with(&located), "{name}: {rendered}");
        }
        // The payload gate reaches inside elements too: a hostile key is described, never echoed.
        let hostile = format!("{}\"a b\" = 1\n", section("crates", CRATE));
        let rendered = parse(&hostile).expect_err("hostile key").to_string();
        assert!(rendered.contains("BXW0010"), "{rendered}");
        assert!(!rendered.contains("a b"), "{rendered}");
        // A binding element nests one level deeper still, and the same funnel reaches it.
        let nested = format!("{}nope = 1\n", bound(BINDING));
        assert_eq!(codes(&nested), ["BXW0010"]);
        let rendered = parse(&nested).expect_err("unknown key").to_string();
        let key = "offending=\"manifest key nope\"";
        assert!(rendered.starts_with(&format!("BXW0010 boxology.toml:11:1-11:5 {key}")));
    }
    #[test]
    fn composition_is_kind_gated() {
        let valid = parse(&composed("[\"hello\"]")).expect("a composition section");
        assert_eq!(valid.kind(), Kind::Composition);
        let composition = valid.composition().expect("declared");
        assert_eq!(composition.boxes()[0].as_str(), "hello");
        assert!(composition.bindings().is_empty());
        // The section is a kind claim in both directions. An absent one has no span of its own,
        // so it reports at the document origin, exactly as an absent `owned` does.
        assert_eq!(codes(COMPOSED), ["BXW0023"]);
        let absent = parse(COMPOSED).expect_err("no section").to_string();
        let at = r#"BXW0023 boxology.toml:1:1-1:1 offending="manifest key composition""#;
        assert!(absent.starts_with(at), "{absent}");
        let source = format!("source={D2_SOURCE:?}");
        assert!(absent.ends_with(&source), "{absent}");
        for kind in ["box", "platform"] {
            let head = format!("schema = 1\nid = \"d\"\nkind = \"{kind}\"\nowned = [\"a\"]\n");
            let text = format!("{head}[composition]\nboxes = [\"hello\"]\n");
            assert_eq!(codes(&text), ["BXW0022"], "{kind}");
            let rendered = parse(&text).expect_err("misplaced").to_string();
            let at = r#"BXW0022 boxology.toml:5:2-5:13 offending="manifest key composition""#;
            assert!(rendered.starts_with(at), "{rendered}");
        }
        // An already-rejected kind adds no second complaint about the section it cannot judge.
        let rejected = "schema = 1\nid = \"d\"\nkind = \"nope\"\nowned = []\n[composition]\n";
        assert_eq!(codes(&format!("{rejected}boxes = [\"h\"]\n")), ["BXW0009"]);
        // The section holds its declared type, and its own key inventory rejects below the root.
        assert_eq!(codes(&format!("{COMPOSED}composition = 7\n")), ["BXW0011"]);
        let extra = format!("{}nope = 1\n", composed("[\"h\"]"));
        assert_eq!(codes(&extra), ["BXW0010"]);
    }
    #[test]
    fn selected_boxes_are_named_and_unique() {
        let valid = parse(&composed("[\"hello\", \"other-1\"]")).expect("two boxes");
        let boxes = valid.composition().expect("declared").boxes();
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[1].as_str(), "other-1");
        // `boxes` is required, and empty is a defect: a section is a container whose emptiness
        // reads as omission, but a list whose presence is itself a claim rejects empty, and a
        // composition selecting nothing composes nothing. `owned` stays the exception: an
        // unowned package is T2 classification rather than a parse defect.
        // An absent `boxes` is blamed on the section header, the nearest construct that can hold it.
        let bare = parse(&format!("{COMPOSED}[composition]\n"))
            .expect_err("bare")
            .to_string();
        let header = r#"BXW0012 boxology.toml:5:1-5:14 offending="manifest key boxes""#;
        assert!(bare.starts_with(header), "{bare}");
        assert_eq!(codes(&composed("[]")), ["BXW0034"]);
        let empty = parse(&composed("[]")).expect_err("empty").to_string();
        let at = r#"BXW0034 boxology.toml:6:1-6:6 offending="manifest key boxes""#;
        assert!(empty.starts_with(at), "{empty}");
        for wrong in ["\"hello\"", "7", "[7]"] {
            assert_eq!(codes(&composed(wrong)), ["BXW0011"], "{wrong}");
        }
        // A selection is the package-id grammar, and never echoes what it rejected.
        for id in ["Payload", "0payload", "payload_x", "payload.x", ""] {
            let text = composed(&format!("[\"{id}\"]"));
            assert_eq!(codes(&text), ["BXW0035"], "{id}");
            let rendered = parse(&text).expect_err("bad id").to_string().to_lowercase();
            assert!(!rendered.contains("payload"), "{rendered}");
        }
        // A duplicate reports at its own entry, not at the first occurrence.
        assert_eq!(codes(&composed("[\"a\", \"a\"]")), ["BXW0036"]);
        let twice = parse(&composed("[\"a\", \"a\"]"))
            .expect_err("dup")
            .to_string();
        let at = r#"BXW0036 boxology.toml:6:15-6:18 offending="manifest key boxes""#;
        assert!(twice.starts_with(at), "{twice}");
    }
    #[test]
    fn bindings_reference_selected_boxes() {
        let valid = parse(&bound(BINDING)).expect("a declared binding");
        let bindings = valid.composition().expect("declared").bindings();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].capability().box_id().as_str(), "hello");
        assert_eq!(bindings[0].capability().to_string(), "hello.greet");
        assert_eq!(bindings[0].transport(), Transport::InProcess);
        assert_eq!(bindings[0].exposure(), None);
        // Declaring no binding reads as omitting the section: selecting boxes is one claim,
        // wiring a capability across them is another.
        let none = parse(&composed("[\"hello\"]")).expect("no bindings");
        assert!(none.composition().expect("declared").bindings().is_empty());
        // 02-packages binds one capability twice over two transports, so repetition is legal and
        // its own example parses: this is that document's `[composition]` block exactly.
        let second = r#"box = "hello"|capability = "hello.greet"|transport = "http""#;
        let twice = format!("{BINDING}|[[composition.bindings]]|{second}|exposure = \"external\"");
        let twice = bound(&twice);
        let valid = parse(&twice).expect("one capability bound twice");
        let pair = valid.composition().expect("declared").bindings();
        assert_eq!(pair.len(), 2);
        assert_eq!(pair[1].transport(), Transport::Http);
        assert_eq!(pair[1].exposure(), Some(Exposure::External));
        let http = r#"|transport = "http""#;
        let wired = |b: &str, c: &str| bound(&format!(r#"box = "{b}"|capability = "{c}"{http}"#));
        // Both membership rules are in-document: the selection list, and the capability's own
        // qualifier. Whether either identity exists at all is T5's cross-document work.
        assert_eq!(codes(&wired("other", "other.greet")), ["BXW0040"]);
        let stray = parse(&wired("other", "other.greet")).expect_err("unselected");
        let at = r#"BXW0040 boxology.toml:8:1-8:4 offending="manifest key box""#;
        assert!(stray.to_string().starts_with(at), "{stray}");
        assert_eq!(codes(&wired("hello", "other.greet")), ["BXW0041"]);
        let foreign = parse(&wired("hello", "other.greet")).expect_err("qualifier");
        let at = r#"BXW0041 boxology.toml:9:1-9:11 offending="manifest key capability""#;
        assert!(foreign.to_string().starts_with(at), "{foreign}");
        // A rejected box reference still qualifies nothing, so both report, in span order.
        let both = ["BXW0035", "BXW0041"];
        assert_eq!(codes(&wired("Hello", "hello.greet")), both);
        // The capability grammar is a box id, the first dot, then [a-z][a-z0-9_]*.
        for name in "hello hello.Greet hello.greet-x hello. .greet hello.a.b".split(' ') {
            assert_eq!(codes(&wired("hello", name)), ["BXW0037"], "{name}");
        }
        for name in ["hello.greet", "hello.a_1", "hello.a__b"] {
            assert!(parse(&wired("hello", name)).is_ok(), "{name}");
        }
        // An absent required key is located at the element it is missing from, and an element
        // missing its `box` is coded as absent rather than also called misqualified.
        let unnamed = bound(r#"capability = "hello.greet"|transport = "http""#);
        assert_eq!(codes(&unnamed), ["BXW0012"]);
        let absent = bound(r#"box = "hello"|transport = "http""#);
        assert_eq!(codes(&absent), ["BXW0012"]);
        let rendered = parse(&absent).expect_err("absent").to_string();
        let at = r#"BXW0012 boxology.toml:7:1-7:25 offending="manifest key capability""#;
        assert!(rendered.starts_with(at), "{rendered}");
    }
    #[test]
    fn binding_transport_and_exposure_vocabularies() {
        // Exposure orders as S4 D5 fixes it, so T5's max_exposure comparison cannot invert.
        let order = [Exposure::CodeOnly, Exposure::Internal, Exposure::External];
        assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
        let head = r#"box = "hello"|capability = "hello.greet""#;
        let wired = |tail: &str| bound(&format!("{head}|{tail}"));
        let exposed = |t: &str, e: &str| wired(&format!(r#"transport = "{t}"|exposure = "{e}""#));
        for (word, expected) in ["code_only", "internal", "external"].into_iter().zip(order) {
            let valid = parse(&exposed("http", word)).expect("a declared exposure");
            let bindings = valid.composition().expect("declared").bindings();
            assert_eq!(bindings[0].exposure(), Some(expected), "{word}");
            assert_eq!(bindings[0].transport(), Transport::Http, "{word}");
        }
        // Both vocabularies are closed and case-sensitive; exposure alone is optional.
        for word in ["Code_Only", "code-only", "payload", ""] {
            assert_eq!(codes(&exposed("http", word)), ["BXW0039"], "{word}");
        }
        for word in ["In-Process", "in_process", "payload", ""] {
            let tail = format!(r#"transport = "{word}""#);
            assert_eq!(codes(&wired(&tail)), ["BXW0038"], "{word}");
        }
        for tail in ["transport = 7", "transport = \"http\"|exposure = 7"] {
            assert_eq!(codes(&wired(tail)), ["BXW0011"], "{tail}");
        }
        assert_eq!(
            codes(&bound(r#"box = "hello"|capability = "hello.greet""#)),
            ["BXW0012"]
        );
        // Neither vocabulary echoes the word it rejected; both name their own key.
        let rendered = parse(&exposed("payload", "payload"))
            .expect_err("both")
            .to_string();
        let at = r#"BXW0038 boxology.toml:10:1-10:10 offending="manifest key transport""#;
        let then = r#"BXW0039 boxology.toml:11:1-11:9 offending="manifest key exposure""#;
        assert!(rendered.starts_with(at), "{rendered}");
        assert!(rendered.contains(then), "{rendered}");
        assert!(!rendered.contains("payload"), "{rendered}");
    }
    #[test]
    fn fixtures_key_spans_are_pinned() {
        // For both codes the key's presence is the defect, not the value it holds, so both span
        // the key itself. Their interaction is pinned too: each defect reports independently.
        let head = "schema = 1\nid = \"p\"\nkind = \"box\"\nowned = [\"a\"]\n";
        let empty = format!("{head}fixtures = []\n");
        assert_eq!(codes(&empty), ["BXW0021", "BXW0034"]);
        let rendered = parse(&empty).expect_err("two fixture defects").to_string();
        for code in ["BXW0021", "BXW0034"] {
            let located = format!("{code} boxology.toml:5:1-5:9");
            assert!(rendered.contains(&located), "{rendered}");
        }
        // A dialect defect is located at its own pattern, so the pair comes out in span order.
        let escaping = format!("{head}fixtures = [\"../x\"]\n");
        assert_eq!(codes(&escaping), ["BXW0021", "BXW0016"]);
        let rendered = parse(&escaping).expect_err("kind and dialect").to_string();
        let located = "BXW0016 boxology.toml:5:13-5:19";
        assert!(rendered.contains(located), "{rendered}");
    }
    #[test]
    fn unknown_keys_reject_inside_nested_tables() {
        // `[quality]` is the first modelled nested table, so key rejection must now be shown to
        // reach a level below the root, and to point at the offending key, not its table header.
        let text = format!("{HEAD}[quality]\ncommands = [\"cargo test\"]\nnope = 1\n");
        assert_eq!(codes(&text), ["BXW0010"]);
        let rendered = parse(&text).expect_err("unknown nested key").to_string();
        let located = "BXW0010 boxology.toml:7:1-7:5 offending=\"manifest key nope\"";
        assert!(rendered.starts_with(located), "{rendered}");
        // Nesting inherits the payload gate: a hostile nested key is described, never echoed.
        let hostile = format!("{HEAD}[quality]\ncommands = [\"cargo test\"]\n\"a b\" = 1\n");
        let rendered = parse(&hostile).expect_err("hostile nested key").to_string();
        assert!(rendered.contains("BXW0010"), "{rendered}");
        assert!(!rendered.contains("a b"), "{rendered}");
    }
}
