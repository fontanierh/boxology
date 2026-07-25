use crate::{D2_SOURCE, Diagnostic, Diagnostics, GlobPattern, LineColumn, RelativePath, Span};
use boxology_contract::BoxId;
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
/// The schema-1 keys this slice models. `[composition]` is absent on purpose: until modelled it is
/// an unknown key and rejects, so no intermediate state of this crate accepts what it cannot check.
const TOP_KEYS: &str = "schema id kind owned display_name fixtures quality crates derived imports";
/// The only key `[quality]` models; nesting inherits the same fail-closed inventory rule.
const QUALITY_KEYS: &str = "commands";
/// The key inventory of one element of each array-of-tables section, applied per element.
const CRATE_KEYS: &str = "cargo_package path role";
const DERIVED_KEYS: &str = "id generator inputs outputs";
const IMPORT_KEYS: &str = "package contract";

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
    id: String,
    generator: String,
    inputs: Vec<GlobPattern>,
    outputs: Vec<GlobPattern>,
}
impl DerivedOutput {
    ref_getters! {
        #[doc = "Returns the package-local output id."] id: &str = id;
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

/// A validated schema-1 `boxology.toml`. Construction is the validation: a `Manifest` exists only
/// for a document that satisfied every rule this crate knows, so consumers never re-check it.
#[derive(Debug, Eq, PartialEq)]
pub struct Manifest {
    id: BoxId,
    kind: Kind,
    owned: Vec<GlobPattern>,
    display_name: Option<String>,
    fixtures: Vec<GlobPattern>,
    quality_commands: Vec<String>,
    crates: Vec<CrateEntry>,
    derived: Vec<DerivedOutput>,
    imports: Vec<Import>,
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
        let quality_commands = match root.get("quality") {
            Some(item) => parser.quality(item),
            None => Vec::new(),
        };
        let crates = root.get("crates").map_or(Vec::new(), |i| parser.crates(i));
        let outputs = root.get("derived");
        let derived = outputs.map_or(Vec::new(), |i| parser.derived(i));
        let declared = root.get("imports");
        let imports = declared.map_or(Vec::new(), |i| parser.imports(i));
        match (Diagnostics::new(parser.errors), id.zip(kind)) {
            (None, Some((id, kind))) => Ok(Manifest {
                id,
                kind,
                owned,
                display_name,
                fixtures,
                quality_commands,
                crates,
                derived,
                imports,
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
    ref_getters! {
        #[doc = "Returns the validated package id."] id: &BoxId = id;
        #[doc = "Returns the declared owned patterns, in declaration order."] owned: &[GlobPattern] = owned;
        #[doc = "Returns the declared fixture patterns; always empty off a platform package."] fixtures: &[GlobPattern] = fixtures;
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
            let id = raw.and_then(|t| self.check(table, "id", "BXW0031", identity(t)));
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
        "BXW0024" => "v1 imports the package's canonical contract, so contract must equal package",
        "BXW0025" => "declared import packages must be unique",
        "BXW0021" => "only a platform package may declare fixtures",
        "BXW0026" => "a quality command must be non-blank text",
        "BXW0027" => ROLES,
        "BXW0028" => "crate paths must be literal relative paths",
        "BXW0029" => "crate paths and cargo package names must be unique",
        "BXW0030" => "cargo package names must be non-empty identifiers",
        "BXW0031" => "derived output ids must match [a-z][a-z0-9-]*",
        "BXW0032" => "derived output ids must be unique",
        "BXW0033" => "generator identities must match [a-z][a-z0-9-]*",
        "BXW0034" => "this list must contain at least one entry",
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
    /// A document whose `[[name]]` section holds one element, a key per `|`-separated field.
    fn section(name: &str, body: &str) -> String {
        let element: String = body.split('|').map(|line| format!("{line}\n")).collect();
        format!("{HEAD}[[{name}]]\n{element}")
    }
    /// The one derived output the tests declare, spelled as `section` splits it.
    fn output() -> String {
        format!(r#"id = "contract"|generator = "boxology-contract"|{LISTS}"#)
    }
    fn parse(text: &str) -> Result<Manifest, Diagnostics> {
        let path = RelativePath::new("boxology.toml").expect("test literal is a valid path");
        Manifest::parse(path, text.as_bytes())
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
        // `[composition]` is the last unmodelled section: until then schema 1 rejects it,
        // fail-closed, and its case flips to an accepted key when that slice lands, as the
        // `[[imports]]` case did in this one. The last two keys are prefix collisions in both
        // directions: the inventory match is equality, so no near-miss of a known key is accepted.
        for extra in [
            "nope = 1\n",
            "[composition]\nboxes = []\n",
            "import = 1\n",
            "schemas = 1\n",
        ] {
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
        let text = kinded("\"composition\"");
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
        for kind in ["box", "composition"] {
            let head = format!("schema = 1\nid = \"p\"\nkind = \"{kind}\"\nowned = [\"a\"]\n");
            let text = format!("{head}fixtures = [\"f/**\"]\n");
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
        for shape in ["[crates]\npath = \"a\"\n", "crates = [1]\n"] {
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
        assert_eq!(valid.derived()[0].id(), "contract");
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
