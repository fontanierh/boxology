//! The coded rejection vocabulary of the strict format-1 reader.
//!
//! The reader entry point is deliberately absent: nothing tolerant ever ships, so this crate
//! exposes no partial parser. What lands here is the complete inventory of rejections the reader
//! may ever report, locked before the reader has a single consumer.

use std::fmt;

/// A stable `BXC####` diagnostic code, or one of the frozen texts a code renders.
type Code = &'static str;

/// Where each rule is written down: the narrowing gates are the strict reader's own, so S4's; the
/// identity namespaces, the contract grammar, and the revision spelling are S2's.
const READER: Code = "specs/s4-contract-change-classification.md D1";
const FINGERPRINT: Code = "specs/s2-contract-generator.md D6";
const IDENTITY: Code = "specs/s2-contract-generator.md D4";
const GRAMMAR: Code = "specs/s2-contract-generator.md D3";

/// Where in a schema document a diagnostic points: a JSON-pointer-style path such as
/// `/capabilities/0/shape`, and the empty pointer for the document itself. `serde_json` records no
/// byte spans, so `boxology_manifest`'s line-and-column model has nothing to read from, and a
/// structural pointer also stays correct under any reformatting of the same document.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Location(String);

// The reader is the one consumer of the constructors and tables here and lands in a later slice.
// Waived per item rather than per module, so the validators the next slice adds to this file are
// still held to dead-code detection — an unwired validator is what a blanket waiver would hide.
#[allow(dead_code)]
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
    #[allow(dead_code)]
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
        "BXC0022" => "format 1's only variant payload is unit",
        "BXC0023" => "a capability error must name a declared type",
        _ => "a schema document must satisfy format 1",
    }
}

/// BXC0001-BXC0008 are the reader's own, including the two narrowings that reject values S2 D3's
/// *contract* grammar lists as legal: the one emitter provably cannot write them, so citing D3
/// there would point a reader at text saying the opposite. BXC0009 is D6's fingerprint spelling,
/// BXC0010-BXC0014 D4's identity namespaces, BXC0015-BXC0023 D3's grammar — where the uniqueness
/// rules are actually written (D4 states none) and the only text reaching an input parameter name.
fn source_of(code: Code) -> Code {
    match code {
        "BXC0009" => FINGERPRINT,
        _ if ("BXC0010"..="BXC0014").contains(&code) => IDENTITY,
        _ if ("BXC0015"..="BXC0023").contains(&code) => GRAMMAR,
        _ => READER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every code this crate emits, ascending; the golden below is driven from it. Its reachability
    /// half — `corpus_covers_every_code`, one minimal document provoking each code — cannot exist
    /// until something can parse a document, so it lands with the reader, and **that slice may not
    /// merge without it**.
    #[rustfmt::skip]
    const ALL_CODES: &[Code] = &[
        "BXC0001", "BXC0002", "BXC0003", "BXC0004", "BXC0005", "BXC0006", "BXC0007", "BXC0008",
        "BXC0009", "BXC0010", "BXC0011", "BXC0012", "BXC0013", "BXC0014", "BXC0015", "BXC0016",
        "BXC0017", "BXC0018", "BXC0019", "BXC0020", "BXC0021", "BXC0022", "BXC0023",
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
BXC0022 format 1's only variant payload is unit specs/s2-contract-generator.md D3
BXC0023 a capability error must name a declared type specs/s2-contract-generator.md D3
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
        assert_eq!(seen, ALL_CODES);
        // Dense from BXC0001: ascending alone would let a gap open unnoticed.
        let spell = |n| format!("BX{}{n:04}", 'C');
        let dense: Vec<String> = (1..=ALL_CODES.len()).map(spell).collect();
        assert_eq!(dense, ALL_CODES);
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
    fn public_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(Diagnostic, Diagnostics)>();
    }
}
