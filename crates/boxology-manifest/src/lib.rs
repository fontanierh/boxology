//! Pure model types, coded diagnostics, and the frozen glob dialect of Boxology manifests.
//!
//! Callers provide every logical path and byte. This crate consults no filesystem, environment,
//! network, locale, or clock, and it has no uncoded failure path.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod glob;

pub use glob::GlobPattern;
pub use parse::{Binding, Composition, CrateEntry, CrateRole, DerivedOutput, Exposure};
pub use parse::{Import, Kind, Manifest, Transport};

use std::fmt;

/// The normative source of the manifest rules this crate enforces.
const D2_SOURCE: &str = "specs/s5-manifest-and-validation.md D2";
// `BXW####` allocation, recorded so this task's slices cannot collide or strand gaps. Landed:
// BXW0013–BXW0019 the glob dialect; BXW0001–BXW0012 the document gates, identity, kind, and key
// inventory; BXW0020 duplicate list patterns; BXW0021 `fixtures` and BXW0034 a list whose presence
// is a claim left empty; BXW0026 `[quality].commands`; BXW0027–BXW0030 `[[crates]]`; BXW0031–BXW0033 `[[derived]]`;
// BXW0024 and BXW0025 `[[imports]]`; BXW0022 and BXW0023 the `[composition]` kind gate, with
// BXW0035–BXW0041 its boxes and bindings. That completes the schema-1 key inventory, so BXW0042 is
// the next free code and is unallocated: discovery and classification claim from there.
macro_rules! ref_getters {
    ($(#[$meta:meta] $name:ident: $return:ty = $field:tt;)*) => {$(
        #[$meta] pub fn $name(&self) -> $return { &self.$field }
    )*};
}
macro_rules! copy_getters {
    ($(#[$meta:meta] $name:ident: $return:ty = $field:ident;)*) => {$(
        #[$meta] pub fn $name(&self) -> $return { self.$field }
    )*};
}
mod parse;
/// An owned, package-relative UTF-8 logical path with bytewise equality and order.
///
/// Deliberately stricter than `boxology_generator_model::RelativePath`, which permits leading
/// `..` segments so a generator can name a checked-in schema outside its own crate. Manifest
/// paths are anchored at the manifest's own directory and never leave that subtree, so `..` is
/// rejected outright here rather than resolved against anything.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RelativePath(String);
impl RelativePath {
    /// Validates a manifest-relative path: nonempty, no root or drive prefix, no backslash or
    /// control byte, and no empty, `.`, or `..` segment.
    pub fn new(value: impl Into<String>) -> Result<Self, PathError> {
        let value = value.into();
        let bytes = value.as_bytes();
        if value.is_empty()
            || value.starts_with('/')
            || has_drive_prefix(bytes)
            || bytes.iter().any(|byte| is_forbidden_byte(*byte))
            || value.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(PathError);
        }
        Ok(Self(value))
    }
    /// Returns the exact, unnormalized spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// The location-free rejection of a malformed logical path; callers add code and span.
///
/// Deliberately kept opaque rather than split into a discriminant per violation. No manifest value
/// reaches `RelativePath::new`: the path-shaped values a manifest declares are globs, which carry
/// their own seven codes, and the manifest's own location is supplied by the caller. Every path
/// rejection here is therefore a caller error, not a diagnosable document defect, and a public
/// six-variant vocabulary would have no reporting consumer. Discovery gains the discriminant, with
/// the codes it needs, when and if it starts reading paths out of documents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathError;
fn has_drive_prefix(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(byte) if byte.is_ascii_alphabetic()) && bytes.get(1) == Some(&b':')
}
fn is_forbidden_byte(byte: u8) -> bool {
    matches!(byte, b'\\' | b'\t' | b'\n' | b'\r' | 0)
}
/// A source coordinate with one-based line and character-column units.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LineColumn {
    line: usize,
    column: usize,
}
impl LineColumn {
    /// Creates a one-based source coordinate.
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
    copy_getters! {
        #[doc = "Returns the one-based line."] line: usize = line;
        #[doc = "Returns the one-based column."] column: usize = column;
    }
}
/// A source span with one-based start and end coordinates.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Span {
    start: LineColumn,
    end: LineColumn,
}
impl Span {
    /// Creates a span from its start and end coordinates.
    pub fn new(start: LineColumn, end: LineColumn) -> Self {
        Self { start, end }
    }
    copy_getters! {
        #[doc = "Returns the start coordinate."] start: LineColumn = start;
        #[doc = "Returns the end coordinate."] end: LineColumn = end;
    }
}
/// One stable coded manifest diagnostic. The offending construct is always static,
/// caller-independent text: manifest values are located by span and described by rule, never
/// echoed back to the reader.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Diagnostic {
    path: RelativePath,
    span: Span,
    code: &'static str,
    offending: String,
    rule: &'static str,
    rule_source: &'static str,
}
impl Diagnostic {
    copy_getters! {
        #[doc = "Returns the stable `BXW####` code."] code: &'static str = code;
        #[doc = "Returns the source span."] span: Span = span;
    }
    ref_getters! {
        #[doc = "Returns the manifest-relative logical path."] path: &RelativePath = path;
        #[doc = "Returns the offending construct description."] offending_construct: &str = offending;
        #[doc = "Returns the violated rule."] rule: &str = rule;
        #[doc = "Returns the normative source of the rule."] rule_source: &str = rule_source;
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}:{}:{}-{}:{} offending={:?} rule={:?} source={:?}",
            self.code,
            self.path.as_str(),
            self.span.start.line,
            self.span.start.column,
            self.span.end.line,
            self.span.end.column,
            self.offending,
            self.rule,
            self.rule_source
        )
    }
}
/// A nonempty, deterministically sorted diagnostic collection.
#[derive(Debug, Eq, PartialEq)]
pub struct Diagnostics(Vec<Diagnostic>);
impl Diagnostics {
    /// Sorts accumulated diagnostics into report order; returns `None` when there are none.
    pub fn new(mut diagnostics: Vec<Diagnostic>) -> Option<Self> {
        diagnostics.sort();
        (!diagnostics.is_empty()).then_some(Self(diagnostics))
    }
    /// Consumes the collection into its sorted diagnostics, which a consumer moves into a report
    /// of its own. `Diagnostic` is deliberately not `Clone`: one diagnostic has one owner.
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.0
    }
    ref_getters! {
        #[doc = "Returns the sorted diagnostics."] as_slice: &[Diagnostic] = 0;
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
        for (index, diagnostic) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str("\n")?;
            }
            diagnostic.fmt(formatter)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn point() -> Span {
        Span::new(LineColumn::new(1, 1), LineColumn::new(1, 1))
    }
    fn path(value: &str) -> RelativePath {
        RelativePath::new(value).expect("test literals are valid manifest-relative paths")
    }
    fn glob(pattern: &str) -> GlobPattern {
        match GlobPattern::parse(pattern, &path("boxology.toml"), point()) {
            Ok(glob) => glob,
            Err(diagnostic) => panic!("{diagnostic}"),
        }
    }
    #[test]
    fn glob_corner_cases_match_frozen_dialect() {
        // "<pattern> | <matching path>... !<non-matching path>...", one frozen case per line.
        for case in [
            "boxology.toml | boxology.toml !a/boxology.toml !boxology.tom",
            "*.rs | a.rs .rs !src/a.rs !a.rs/b !a.md",
            "implementation/* | implementation/a.rs !implementation/a/b.rs !implementation",
            "** | a a/b/c",
            "**/x | x a/x a/b/x !x/a !xa",
            "a/**/b | a/b a/x/b a/x/y/b !a !b !a/b/c",
            "a/** | a/b a/b/c !a !ab/c !b/a",
            "**/**/x | x a/b/x !a/x/b",
            "src/**/*.rs | src/a.rs src/a/b.rs !src/a.md !a.rs",
            "implementation/** | implementation/a/b.rs !implementation !impl/a",
            "A.rs | A.rs !a.rs",
        ] {
            let Some((pattern, expectations)) = case.split_once(" | ") else {
                panic!("malformed case {case}");
            };
            let glob = glob(pattern);
            assert_eq!(glob.as_str(), pattern);
            for expectation in expectations.split(' ') {
                let rejected = expectation.strip_prefix('!');
                let value = rejected.unwrap_or(expectation);
                let expected = rejected.is_none();
                assert_eq!(glob.matches(&path(value)), expected, "{pattern} vs {value}");
            }
        }
        let literal = glob("a{b} !].rs");
        assert!(literal.matches(&path("a{b} !].rs")) && !literal.matches(&path("a{b}!].rs")));
        let untrimmed = glob("a.rs ");
        assert!(untrimmed.matches(&path("a.rs ")) && !untrimmed.matches(&path("a.rs")));
    }
    #[test]
    fn glob_rejections_are_coded_and_payload_safe() {
        // Deliberately not in code order: the first case sorts last, so the accumulated set only
        // comes out ascending if `Diagnostics::new` really sorts it.
        let cases = [
            ("payload**", "BXW0019"),
            ("", "BXW0013"),
            ("/payload", "BXW0014"),
            ("c:/payload", "BXW0014"),
            ("payload//x", "BXW0015"),
            ("payload/", "BXW0015"),
            (".", "BXW0015"),
            ("../payload", "BXW0016"),
            ("payload\\x", "BXW0017"),
            ("payload?", "BXW0018"),
            ("payload[", "BXW0018"),
            ("!payload", "BXW0018"),
            ("**payload", "BXW0019"),
            ("payload**x", "BXW0019"),
        ];
        let mut all = Vec::new();
        for (pattern, code) in cases {
            let here = path("boxology.toml");
            let Err(diagnostic) = GlobPattern::parse(pattern, &here, point()) else {
                panic!("{code} pattern {pattern:?} was accepted");
            };
            let rendered = diagnostic.to_string();
            assert_eq!(diagnostic.code(), code);
            assert_eq!(diagnostic.span(), point());
            assert_eq!(diagnostic.rule_source(), D2_SOURCE);
            assert!(!rendered.contains("payload"), "{code} echoed its input");
            assert!(!rendered.contains(['\n', '\r']));
            let prefix = format!("{code} boxology.toml:1:1-1:1 offending=");
            assert!(rendered.starts_with(&prefix), "{rendered}");
            all.push(diagnostic);
        }
        let diagnostics = Diagnostics::new(all).expect("every case is rejected");
        let sorted = diagnostics.into_iter().collect::<Vec<_>>();
        assert!(sorted.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(diagnostics.to_string().lines().count(), cases.len());
        assert!(Diagnostics::new(Vec::new()).is_none());
    }
    /// Check order, not input shape, decides the code an input violating two rules reports. The
    /// frozen priority is structural rejection before dialect rejection: a pattern that cannot be
    /// anchored at all (absolute, drive-prefixed, control-bearing) is rejected as such before any
    /// question about the wildcard vocabulary, and segment shape is judged left to right. Pinning
    /// it here keeps corpus goldens stable when the checks are ever reordered or fused.
    #[test]
    fn multi_violation_priority_is_frozen() {
        let here = path("boxology.toml");
        for (pattern, code) in [
            ("/..", "BXW0014"),
            ("c:/**x", "BXW0014"),
            ("//..", "BXW0014"),
            ("..\\x", "BXW0017"),
            ("../?", "BXW0018"),
            ("a//..", "BXW0015"),
        ] {
            let Err(diagnostic) = GlobPattern::parse(pattern, &here, point()) else {
                panic!("{pattern:?} was accepted");
            };
            assert_eq!(diagnostic.code(), code, "{pattern:?}");
        }
    }
    #[test]
    fn relative_path_grammar() {
        for accepted in ["boxology.toml", "a/b/c.rs", "a b/c!.rs", "..a/b", "a.rs "] {
            assert!(RelativePath::new(accepted).is_ok(), "{accepted}");
        }
        // `../a` and `..` are accepted by boxology-generator-model and rejected here.
        for rejected in [
            "", "/a", "c:/a", "a\\b", "a\tb", "a\nb", "a\rb", "a//b", "a/./b", ".", "..", "../a",
            "a/../b", "a/",
        ] {
            assert!(RelativePath::new(rejected).is_err(), "{rejected:?}");
        }
        assert_eq!(path("a/b").as_str(), "a/b");
    }
    #[test]
    fn public_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(RelativePath, PathError, GlobPattern)>();
        bounds::<(LineColumn, Span, Diagnostic, Diagnostics)>();
        bounds::<(Kind, Manifest)>();
        bounds::<(CrateEntry, CrateRole, DerivedOutput, Import)>();
        bounds::<(Binding, Composition, Exposure, Transport)>();
    }
}
