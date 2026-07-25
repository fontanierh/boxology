//! Pure workspace inputs, coded findings, and the frozen report order of Boxology workspaces.
//!
//! Callers supply every byte and logical path this crate reads: a file listing, manifest bytes,
//! and a `cargo metadata` document are data arguments. The crate consults no filesystem, process,
//! environment, network, locale, or clock, and it has no uncoded failure path — caller misuse is
//! a typed [`InputError`] and every document defect a coded [`Finding`].
//!
//! **Payload safety.** A rendered finding echoes only values a grammar already validated: a
//! [`BoxId`] (`[a-z][a-z0-9-]*`), a [`RelativePath`] and [`GlobPattern`] (no NUL, tab, line break,
//! or backslash; no `..` in a path). Rejecting line breaks holds one finding to one line; other
//! control bytes reach a report, a residual gap in the grammar this crate consumes. Every other
//! rendered word is a `&'static str` this crate chose, so no caller string reaches a report.
//!
//! **What the CLI still owns.** A [`FileEntry::symlink`] target is judged *lexically*, against
//! the link's own parent directory: physical resolution through an intermediate symlinked
//! directory is a question a pure library cannot answer, and the complementary walk that never
//! follows a link is T5's CLI-side obligation.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::BoxId;
use boxology_manifest::{GlobPattern, RelativePath};
use std::fmt;
/// A coded rule: its stable `BXW####` code and the static text of the obligation it states.
type Rule = (&'static str, &'static str);
/// The normative source of the discovery rules this crate enforces.
const WALK_SOURCE: &str = "boxology-details/02-packages.md discovery walk";
const ESCAPE_TEXT: &str = "symlink targets must stay inside the workspace root";
const ESCAPE: Rule = ("BXW0048", ESCAPE_TEXT);
// `BXW####` allocation, recorded so this task's slices cannot collide or strand gaps. T1 landed
// BXW0001–BXW0041, the whole schema-1 manifest inventory, so T2 opens at BXW0042 and reserves
// through BXW0054. Landed: BXW0048 symlink escape. Allocated: BXW0042–BXW0047 discovery and
// ownership, BXW0049–BXW0051 derived outputs, BXW0052–BXW0054 crate-role mapping.
macro_rules! ref_getters {
    ($(#[$meta:meta] $name:ident: $return:ty = $field:tt;)*) => {$(
        #[$meta] pub fn $name(&self) -> $return { &self.$field }
    )*};
}
/// One tracked workspace file: its logical path and, for a symlink, the exact target git stores
/// as the link's blob content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileEntry(RelativePath, Option<String>);
impl FileEntry {
    /// Records a plain tracked file.
    pub fn file(path: RelativePath) -> Self {
        Self(path, None)
    }
    /// Records a tracked symlink and its target: unvalidated caller data, judged but never echoed.
    pub fn symlink(path: RelativePath, target: String) -> Self {
        Self(path, Some(target))
    }
    ref_getters! {
        #[doc = "Returns the workspace-relative logical path."] path: &RelativePath = 0;
    }
    fn escape(&self) -> Option<Finding> {
        let target = self.1.as_deref()?;
        escapes(&self.0, target).then(|| Finding::new(ESCAPE, self.0.clone(), None, Vec::new()))
    }
}
/// The rejection of an ill-formed input listing, deliberately opaque and deliberately not a
/// `BXW####` code, mirroring `boxology_manifest`'s `PathError`: a listing naming one path twice is
/// a caller programming error, not a diagnosable defect of a document, so it has no consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputError;
/// The complete normalized input of a workspace check.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceInputs {
    files: Vec<FileEntry>,
    manifests: Vec<(RelativePath, Vec<u8>)>,
    cargo_metadata: String,
}
impl WorkspaceInputs {
    /// Sorts files and manifests bytewise by path, rejecting a path repeated within either list
    /// as caller misuse. Manifests arrive as **bytes**, not as parsed models, deliberately:
    /// whether a `boxology.toml` is a real package or opaque fixture data depends on the platform
    /// package's `fixtures` patterns, which only this crate computes, so a pre-parsed argument
    /// would make a deliberately malformed corpus manifest fail in the *caller* and defeat
    /// fixture opacity. The bytes are stored unexamined, as is `cargo_metadata`, until discovery
    /// and crate-role mapping read them in later slices.
    pub fn new(
        files: Vec<FileEntry>,
        manifests: Vec<(RelativePath, Vec<u8>)>,
        cargo_metadata: &str,
    ) -> Result<Self, InputError> {
        Ok(Self {
            files: sorted_unique(files, FileEntry::path)?,
            manifests: sorted_unique(manifests, |entry| &entry.0)?,
            cargo_metadata: String::from(cargo_metadata),
        })
    }
    /// Reports every defect these inputs prove, in the frozen report order; `None` means the
    /// listing is clean. This slice judges symlink escapes; later findings join the same report.
    pub fn check(&self) -> Option<Findings> {
        Findings::new(self.files.iter().filter_map(FileEntry::escape).collect())
    }
}
fn sorted_unique<T>(mut items: Vec<T>, key: fn(&T) -> &RelativePath) -> Result<Vec<T>, InputError> {
    items.sort_by(|left, right| key(left).cmp(key(right)));
    let same = |pair: &[T]| matches!(pair, [left, right] if key(left) == key(right));
    let unique = !items.windows(2).any(same);
    unique.then_some(items).ok_or(InputError)
}
/// Reports whether `target`, read from the symlink at `path`, leaves the workspace root. The
/// judgment is lexical, against the link's own parent directory: an absolute or drive-prefixed
/// target, one carrying a backslash or control byte, an empty one, and one whose `..` segments
/// outnumber that parent's depth all escape. A lexically in-tree target is no finding at all.
fn escapes(path: &RelativePath, target: &str) -> bool {
    let bytes = target.as_bytes();
    let drive = matches!(bytes.first(), Some(byte) if byte.is_ascii_alphabetic())
        && bytes.get(1) == Some(&b':');
    let unusable = |byte: &u8| byte.is_ascii_control() || *byte == b'\\';
    if bytes.is_empty() || bytes.first() == Some(&b'/') || drive || bytes.iter().any(unusable) {
        return true;
    }
    let root = path.as_str().matches('/').count();
    let step = |depth: usize, segment: &str| match segment {
        "" | "." => Some(depth),
        ".." => depth.checked_sub(1),
        _ => Some(depth.saturating_add(1)),
    };
    target.split('/').try_fold(root, step).is_none()
}
/// One claim named by a finding that reports competing or missing attribution. Publicly
/// constructible on purpose: every field is already grammar-validated, so a candidate list carries
/// dynamic data into a report without carrying caller text.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Candidate(BoxId, RelativePath, GlobPattern);
impl Candidate {
    /// Names a package, its manifest, and the pattern under which it claims a path.
    pub fn new(package: BoxId, manifest_path: RelativePath, claim: GlobPattern) -> Self {
        Self(package, manifest_path, claim)
    }
    ref_getters! {
        #[doc = "Returns the claiming package identity."] package: &BoxId = 0;
        #[doc = "Returns the claiming manifest's path."] manifest_path: &RelativePath = 1;
        #[doc = "Returns the claiming pattern."] claim: &GlobPattern = 2;
    }
    fn render(&self) -> String {
        format!("{} {} {}", self.0, self.1.as_str(), self.2.as_str())
    }
}
/// One stable coded workspace finding, rendered on a single line. The derived order **is** the
/// frozen report order: attributed package identity — an unattributed finding sorts first, under
/// the empty id — then workspace-relative path, then code, then rendered payload. Field order is
/// load-bearing, and `payload` precedes the `candidates` it renders so the key decides first.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Finding {
    package: Option<BoxId>,
    path: RelativePath,
    code: &'static str,
    payload: String,
    candidates: Vec<Candidate>,
    rule: &'static str,
    rule_source: &'static str,
}
impl Finding {
    fn new(rule: Rule, path: RelativePath, package: Option<BoxId>, named: Vec<Candidate>) -> Self {
        let rendered: Vec<String> = named.iter().map(Candidate::render).collect();
        Self {
            package,
            path,
            code: rule.0,
            payload: rendered.join(","),
            candidates: named,
            rule: rule.1,
            rule_source: WALK_SOURCE,
        }
    }
    /// Returns the stable `BXW####` code.
    pub fn code(&self) -> &'static str {
        self.code
    }
    /// Returns the accountable package, when the finding attributes one.
    pub fn package(&self) -> Option<&BoxId> {
        self.package.as_ref()
    }
    ref_getters! {
        #[doc = "Returns the workspace-relative logical path."] path: &RelativePath = path;
        #[doc = "Returns every claim the finding names."] candidates: &[Candidate] = candidates;
        #[doc = "Returns the violated rule."] rule: &str = rule;
        #[doc = "Returns the normative source of the rule."] rule_source: &str = rule_source;
    }
}
// The renderer is deliberately minimal in this slice: it emits exactly the frozen order key, so a
// line and its sort position cannot drift apart. The carried rule text and its source stay
// queryable and join the rendering with T5's human and JSON report formats.
impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let package = self.package.as_ref().map_or("", BoxId::as_str);
        let (code, path, payload) = (self.code, self.path.as_str(), &self.payload);
        let head = format!("{code} {path} package={package}");
        write!(formatter, "{head} candidates=[{payload}]")
    }
}
/// A nonempty finding collection in the frozen report order.
#[derive(Debug, Eq, PartialEq)]
pub struct Findings(Vec<Finding>);
impl Findings {
    /// Sorts accumulated findings into report order; returns `None` when there are none.
    pub fn new(mut findings: Vec<Finding>) -> Option<Self> {
        findings.sort();
        (!findings.is_empty()).then_some(Self(findings))
    }
    ref_getters! {
        #[doc = "Returns the sorted findings."] as_slice: &[Finding] = 0;
    }
}
impl<'a> IntoIterator for &'a Findings {
    type Item = &'a Finding;
    type IntoIter = std::slice::Iter<'a, Finding>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl fmt::Display for Findings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<String> = self.0.iter().map(Finding::to_string).collect();
        formatter.write_str(&lines.join("\n"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use boxology_manifest::{LineColumn, Span};
    const OPAQUE: &[u8] = b"schema = 9\nnot toml";
    fn path(value: &str) -> RelativePath {
        RelativePath::new(value).expect("test literals are workspace-relative paths")
    }
    fn id(value: &str) -> BoxId {
        BoxId::new(value).expect("test literals are box identifiers")
    }
    fn claim(pattern: &str) -> Candidate {
        let point = Span::new(LineColumn::new(1, 1), LineColumn::new(1, 1));
        let glob = GlobPattern::parse(pattern, &path("m.toml"), point).expect("a valid pattern");
        Candidate::new(id("owner"), path("m.toml"), glob)
    }
    fn inputs(entries: Vec<FileEntry>) -> WorkspaceInputs {
        WorkspaceInputs::new(entries, Vec::new(), "{}").expect("distinct test paths")
    }
    /// The frozen order is (attributed package id or "", path, code, rendered payload). The input
    /// below is deliberately shuffled — it is neither sorted nor reversed, and every component of
    /// the key decides one adjacent pair of the result — so the expected sequence needs a sort.
    #[test]
    fn report_order_is_frozen() {
        let finding = |package: Option<&str>, at, code, claims: &[&str]| {
            let named = claims.iter().copied().map(claim).collect();
            Finding::new((code, ESCAPE_TEXT), path(at), package.map(id), named)
        };
        let shuffled = vec![
            finding(Some("zebra"), "a.rs", "BXW0042", &[]),
            finding(None, "z.rs", "BXW0042", &[]),
            finding(Some("alpha"), "a.rs", "BXW0043", &["b/*"]),
            finding(Some("alpha"), "a.rs", "BXW0043", &["a/*", "c/*"]),
            finding(Some("alpha"), "a.rs", "BXW0042", &[]),
            finding(None, "a.rs", "BXW0048", &[]),
        ];
        let findings = Findings::new(shuffled).expect("six findings are nonempty");
        assert_eq!(
            findings.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0048 a.rs package= candidates=[]",
                "BXW0042 z.rs package= candidates=[]",
                "BXW0042 a.rs package=alpha candidates=[]",
                "BXW0043 a.rs package=alpha candidates=[owner m.toml a/*,owner m.toml c/*]",
                "BXW0043 a.rs package=alpha candidates=[owner m.toml b/*]",
                "BXW0042 a.rs package=zebra candidates=[]",
            ]
        );
        let last = findings.into_iter().next_back().expect("nonempty");
        assert_eq!(last.package(), Some(&id("zebra")));
        assert!(Findings::new(Vec::new()).is_none());
    }
    #[test]
    fn symlink_escapes_are_coded_and_payload_safe() {
        // "<link path> <target>", `!`-prefixed when the target stays inside the workspace root.
        let cases = "link ../outside,a/link ../../outside,a/b/link ../../../outside,link ,\
                     link /etc/passwd,link c:/windows,link a\\b,link a\nb,link a\0b,\
                     a/link x/../../../y,!link sibling.rs,!link a/../b,!link ./a/b,\
                     !a/link ../x/y.rs,!a/b/link ../../c.rs,!a/b/link c/,!a/b/link x//y";
        for case in cases.split(',') {
            let contained = case.strip_prefix('!');
            let Some((at, target)) = contained.unwrap_or(case).split_once(' ') else {
                panic!("malformed case {case:?}");
            };
            let entry = FileEntry::symlink(path(at), String::from(target));
            let Some(findings) = inputs(vec![entry]).check() else {
                assert!(contained.is_some(), "{case:?} escapes");
                continue;
            };
            assert!(contained.is_none(), "{case:?} stays inside");
            let [finding] = findings.as_slice() else {
                panic!("{case:?} reported {findings}");
            };
            assert_eq!(finding.code(), "BXW0048", "{case:?}");
            assert_eq!(finding.path().as_str(), at, "{case:?}");
            assert_eq!(finding.package(), None, "{case:?}");
            assert_eq!(finding.rule(), ESCAPE_TEXT, "{case:?}");
            assert_eq!(finding.rule_source(), WALK_SOURCE, "{case:?}");
        }
        // The exact rendering, over a target chosen to wreck a report were it ever echoed.
        let hostile = FileEntry::symlink(path("a/link"), "../../\npackage=x".into());
        let report = inputs(vec![hostile]).check().expect("an escape");
        assert_eq!(report.to_string(), "BXW0048 a/link package= candidates=[]");
    }
    #[test]
    fn listings_are_sorted_deduplicated_and_stored_unexamined() {
        let [last, first] = ["b.rs", "a.rs"].map(|at| FileEntry::file(path(at)));
        let link = FileEntry::symlink(path("a/link"), "../b.rs".into());
        let manifests = vec![(path("z.toml"), vec![0]), (path("m.toml"), OPAQUE.to_vec())];
        let files = vec![last, link, first];
        let inputs = WorkspaceInputs::new(files, manifests, "not json").expect("distinct paths");
        let paths: Vec<_> = inputs.files.iter().map(FileEntry::path).collect();
        assert_eq!(paths, [&path("a.rs"), &path("a/link"), &path("b.rs")]);
        let sorted = [(path("m.toml"), OPAQUE.to_vec()), (path("z.toml"), vec![0])];
        assert_eq!(inputs.manifests, sorted);
        assert_eq!(inputs.cargo_metadata, "not json");
        let twice = Vec::from(["a.rs", "a.rs"].map(|at| FileEntry::file(path(at))));
        assert_eq!(WorkspaceInputs::new(twice, Vec::new(), ""), Err(InputError));
        let repeated = vec![(path("m.toml"), Vec::new()), (path("m.toml"), vec![1])];
        let rejected = WorkspaceInputs::new(Vec::new(), repeated, "");
        assert_eq!(rejected, Err(InputError));
    }
}
