use crate::{D2_SOURCE, Diagnostic, Diagnostics, GlobPattern, LineColumn, RelativePath, Span};
use boxology_contract::BoxId;
use std::ops::Range;
use toml_edit::{Document, Item, TableLike};

type Code = &'static str;
const PACKAGES: Code = "boxology-details/02-packages.md";
const ORIGIN: LineColumn = LineColumn { line: 1, column: 1 };
const POINT: Span = Span {
    start: ORIGIN,
    end: ORIGIN,
};
/// The schema-1 keys this slice models. `[[imports]]`, `[[crates]]`, `[[derived]]`, and
/// `[composition]` are absent on purpose: until modelled they are unknown keys and reject, so no
/// intermediate state of this crate accepts what it cannot check.
const TOP_KEYS: &str = "schema id kind owned display_name fixtures quality";
/// The only key `[quality]` models; nesting inherits the same fail-closed inventory rule.
const QUALITY_KEYS: &str = "commands";

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
                let span = item_span(source, Some(item));
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
        match (Diagnostics::new(parser.errors), id.zip(kind)) {
            (None, Some((id, kind))) => Ok(Manifest {
                id,
                kind,
                owned,
                display_name,
                fixtures,
                quality_commands,
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
        let plain = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-';
        let what = match !name.is_empty() && name.bytes().all(plain) {
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
        "BXW0026" => "a quality command must be non-blank text",
        "BXW0034" => "a declared fixtures list must not be empty",
        _ => "the manifest must satisfy schema 1",
    }
}
/// The shape, id grammar, and kind vocabulary are 02-packages'; the rest is the S5 spec's D2.
fn source_of(code: Code) -> Code {
    match ("BXW0003"..="BXW0009").contains(&code) {
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
        // Later slices model the last four; until then schema 1 rejects them, fail-closed. Each
        // case must flip to an accepted key exactly when its slice lands.
        for extra in [
            "nope = 1\n",
            "[composition]\nboxes = []\n",
            "[[imports]]\nid = \"x\"\n",
            "[[crates]]\npath = \"a\"\n",
            "[[derived]]\nid = \"c\"\n",
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
