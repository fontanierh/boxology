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
//! word this crate renders is a `&'static str` it chose. An [`Entry::Manifest`] line is
//! `boxology_manifest`'s own rendering, whose only caller-derived part is a manifest key name it
//! echoes just when the name is plain `[A-Za-z0-9_-]` text.
//!
//! **What the CLI still owns.** A [`FileEntry::symlink`] target is judged *lexically*, against
//! the link's own parent directory: physical resolution through an intermediate symlinked
//! directory is a question a pure library cannot answer, and the complementary walk that never
//! follows a link is T5's CLI-side obligation.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::BoxId;
use boxology_manifest::{Diagnostic, GlobPattern, Manifest, RelativePath};
use std::{cmp::Ordering, fmt};
/// A coded rule: its stable `BXW####` code and the static text of the obligation it states.
type Rule = (&'static str, &'static str);
/// The normative source of the discovery rules this crate enforces.
const WALK_SOURCE: &str = "boxology-details/02-packages.md discovery walk";
const ESCAPE_TEXT: &str = "symlink targets must stay inside the workspace root";
const ESCAPE: Rule = ("BXW0048", ESCAPE_TEXT);
const DUPLICATE_TEXT: &str = "one package identity must be declared by exactly one manifest";
const DUPLICATE: Rule = ("BXW0042", DUPLICATE_TEXT);
const SELF_CLAIM_TEXT: &str = "a fixtures pattern must not claim its own declaring manifest";
const SELF_CLAIM: Rule = ("BXW0043", SELF_CLAIM_TEXT);
/// The one file name a package manifest may carry.
const MANIFEST: &str = "boxology.toml";
// `BXW####` allocation, recorded so this task's slices cannot collide or strand gaps. T1 landed
// BXW0001–BXW0041, the whole schema-1 manifest inventory, so T2 opens at BXW0042 and reserves
// through BXW0054. Landed: BXW0042 duplicate identity, BXW0043 self-claiming fixture pattern,
// BXW0048 symlink escape. Allocated: BXW0044–BXW0047 ownership, BXW0049–BXW0051 derived outputs,
// BXW0052–BXW0054 crate-role mapping.
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
    /// inputs are clean. Later findings join the same report.
    pub fn check(&self) -> Option<Findings> {
        self.discover().1
    }
    /// Discovers the workspace packages, and reports every defect these inputs prove.
    ///
    /// A *candidate* manifest is one the caller supplied bytes for whose final path segment is
    /// exactly `boxology.toml` and which the listing tracks as a plain file: a symlinked manifest
    /// is never a candidate and classifies later as an ordinary path. Every candidate is parsed
    /// and its result held **unjudged**, because whether a `boxology.toml` is a package or opaque
    /// fixture data is decided next.
    ///
    /// Accepted packages are walked in ascending `(depth, path)` order, and a candidate claimed by
    /// an earlier one's `fixtures` pattern is pruned: its held result is discarded whether it
    /// parsed or not, and it contributes no claim of its own. That discard is fixture opacity, and
    /// it is what lets a repository carry a deliberately malformed corpus without any of it
    /// entering validation. Only a platform package may declare `fixtures` (BXW0021), so this one
    /// walk is the platform walk. It is also enough: a claimer is necessarily a *strict* ancestor
    /// of what it claims, because a pattern is anchored at its declaring manifest's own directory
    /// and cannot escape it (BXW0016), and the one claimer that could sit in that same directory
    /// is the declaring manifest itself, which BXW0043 refuses rather than honours. A
    /// strict ancestor has strictly smaller depth, so it is already accepted or already pruned
    /// when its claim is consulted, and no fixpoint iteration is needed.
    pub fn discover(&self) -> (Vec<Package>, Option<Findings>) {
        let plain = |at: &RelativePath| {
            let same = |entry: &&FileEntry| entry.path() == at;
            self.files.iter().find(same).is_some_and(|e| e.1.is_none())
        };
        let mut candidates = Vec::new();
        for (at, bytes) in &self.manifests {
            if is_manifest(at) && plain(at) {
                candidates.push((at.clone(), Manifest::parse(at.clone(), bytes)));
            }
        }
        // `manifests` is bytewise sorted and `sort_by_key` is stable, so a depth key alone
        // yields ascending `(depth, path)`.
        candidates.sort_by_key(|(at, _)| at.as_str().matches('/').count());
        let (mut packages, mut entries): (Vec<Package>, Vec<Entry>) = (Vec::new(), Vec::new());
        for (at, held) in candidates {
            if packages.iter().any(|owner| !owner.claims(&at).is_empty()) {
                continue;
            }
            match held {
                Ok(manifest) => {
                    let package = Package::new(at, manifest);
                    entries.extend(package.self_claim().map(Entry::Workspace));
                    packages.push(package);
                }
                Err(rejected) => {
                    let held = rejected.into_vec().into_iter();
                    entries.extend(held.map(Entry::Manifest));
                }
            }
        }
        // Every carrier of a duplicated identity is named, each located at its own manifest, so
        // the report points at every document that has to change, not only at the later one.
        for package in &packages {
            let same = |other: &&Package| other.id() == package.id();
            if packages.iter().filter(same).count() > 1 {
                let at = package.manifest_path.clone();
                let owner = Some(package.id().clone());
                let finding = Finding::new(DUPLICATE, at, owner, Vec::new());
                entries.push(Entry::Workspace(finding));
            }
        }
        let escapes = self.files.iter().filter_map(FileEntry::escape);
        entries.extend(escapes.map(Entry::Workspace));
        (packages, Findings::new(entries))
    }
}
/// Reports whether `path`'s final segment is exactly `boxology.toml`.
fn is_manifest(path: &RelativePath) -> bool {
    let head = path.as_str().strip_suffix(MANIFEST);
    matches!(head, Some(head) if head.is_empty() || head.ends_with('/'))
}
/// One discovered package: a manifest that parsed and that no accepted package's fixture claim
/// prunes. Discovery constructs it: the manifest, where it sits, and where its patterns anchor.
#[derive(Debug, Eq, PartialEq)]
pub struct Package {
    manifest: Manifest,
    manifest_path: RelativePath,
    root: Option<RelativePath>,
}
impl Package {
    fn new(manifest_path: RelativePath, manifest: Manifest) -> Self {
        let parent = manifest_path.as_str().rsplit_once('/');
        let root = parent.and_then(|(head, _)| RelativePath::new(head).ok());
        Self {
            manifest,
            manifest_path,
            root,
        }
    }
    /// Returns the declared package identity.
    pub fn id(&self) -> &BoxId {
        self.manifest.id()
    }
    /// Returns the directory the package's patterns are anchored at. `None` is the workspace root
    /// itself, which no [`RelativePath`] can spell, and which the platform manifest occupies.
    pub fn root(&self) -> Option<&RelativePath> {
        self.root.as_ref()
    }
    ref_getters! {
        #[doc = "Returns the validated manifest."] manifest: &Manifest = manifest;
        #[doc = "Returns the manifest's path."] manifest_path: &RelativePath = manifest_path;
    }
    /// Re-anchors a workspace-relative path at this package's own root: `None` when the path lies
    /// outside the package, which no pattern of this manifest can reach.
    fn under(&self, path: &RelativePath) -> Option<RelativePath> {
        let Some(root) = &self.root else {
            return Some(path.clone());
        };
        let rest = path.as_str().strip_prefix(root.as_str())?;
        RelativePath::new(rest.strip_prefix('/')?).ok()
    }
    /// Returns every `fixtures` pattern of this package claiming `path`, in declaration order.
    /// Matching is [`GlobPattern::matches`], the single definition of the frozen dialect.
    fn claims(&self, path: &RelativePath) -> Vec<&GlobPattern> {
        let Some(under) = self.under(path) else {
            return Vec::new();
        };
        let hit = |pattern: &&GlobPattern| pattern.matches(&under);
        self.manifest.fixtures().iter().filter(hit).collect()
    }
    /// Reports BXW0043 for every `fixtures` pattern claiming this package's own manifest. The
    /// package keeps its identity and every other claim: honouring a self-claim would erase the
    /// package that declares the fixtures, which turns one located defect into a cascade of
    /// unrelated ones, so the self-claim alone is refused, and reported here instead.
    fn self_claim(&self) -> Option<Finding> {
        let at = &self.manifest_path;
        let name =
            |claim: &GlobPattern| Candidate::new(self.id().clone(), at.clone(), claim.clone());
        let named: Vec<Candidate> = self.claims(at).into_iter().map(name).collect();
        if named.is_empty() {
            return None;
        }
        let owner = Some(self.id().clone());
        Some(Finding::new(SELF_CLAIM, at.clone(), owner, named))
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
/// One stable coded workspace finding, rendered on a single line. The derived order agrees with
/// [`Entry`]'s frozen report key on attributed package identity — an unattributed finding sorts
/// first, under the empty id — then workspace-relative path, then code. It does **not** agree
/// past that: `Entry` breaks the remaining tie on the whole rendered line, whose closing bracket
/// sorts above every byte a payload may hold, so a payload that is a prefix of another orders
/// after it there and before it here. The report is what `Entry` decides; sort by that.
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
/// One line of a workspace report: a coded workspace [`Finding`], or one [`Diagnostic`] the parse
/// of a discovered manifest produced. A pruned manifest reaches neither.
#[derive(Debug, Eq, PartialEq)]
pub enum Entry {
    /// A defect the inputs prove about the workspace.
    Workspace(Finding),
    /// A defect of one discovered manifest document, reported exactly as `boxology_manifest` states
    /// it, span included.
    Manifest(Diagnostic),
}
impl Entry {
    /// The frozen report key, extended to cover both kinds: attributed package identity — never
    /// any, for a parse diagnostic, since a manifest that does not parse names no package — then
    /// workspace-relative path, then code, then the rendered line itself, which decides every
    /// remaining tie and makes the order total over the exact bytes reported.
    fn key(&self) -> (Option<&BoxId>, &RelativePath, &str, String) {
        let (package, path, code) = match self {
            Self::Workspace(finding) => (finding.package(), &finding.path, finding.code),
            Self::Manifest(diagnostic) => (None, diagnostic.path(), diagnostic.code()),
        };
        (package, path, code, self.to_string())
    }
}
impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}
impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl fmt::Display for Entry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Workspace(finding) => finding.fmt(formatter),
            Self::Manifest(diagnostic) => diagnostic.fmt(formatter),
        }
    }
}
/// A nonempty report collection in the frozen report order.
#[derive(Debug, Eq, PartialEq)]
pub struct Findings(Vec<Entry>);
impl Findings {
    /// Sorts accumulated entries into report order; returns `None` when there are none.
    pub fn new(mut entries: Vec<Entry>) -> Option<Self> {
        entries.sort();
        (!entries.is_empty()).then_some(Self(entries))
    }
    ref_getters! {
        #[doc = "Returns the sorted entries."] as_slice: &[Entry] = 0;
    }
}
impl<'a> IntoIterator for &'a Findings {
    type Item = &'a Entry;
    type IntoIter = std::slice::Iter<'a, Entry>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl fmt::Display for Findings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<String> = self.0.iter().map(Entry::to_string).collect();
        formatter.write_str(&lines.join("\n"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use boxology_manifest::{Kind, LineColumn, Span};
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
    /// A minimal valid manifest, carrying the given `fixtures` list when it is nonempty.
    fn document(id: &str, kind: &str, fixtures: &[&str]) -> Vec<u8> {
        let mut text = format!("schema = 1\nid = {id:?}\nkind = {kind:?}\nowned = []\n");
        let quoted: Vec<String> = fixtures.iter().map(|f| format!("{f:?}")).collect();
        if !quoted.is_empty() {
            text.push_str(&format!("fixtures = [{}]\n", quoted.join(", ")));
        }
        text.into_bytes()
    }
    /// Inputs whose listing tracks every named manifest as a plain file.
    fn workspace(manifests: Vec<(&str, Vec<u8>)>) -> WorkspaceInputs {
        let entry = |(at, _): &(&str, Vec<u8>)| FileEntry::file(path(at));
        let files = manifests.iter().map(entry).collect();
        let held = manifests.into_iter().map(|(at, bytes)| (path(at), bytes));
        WorkspaceInputs::new(files, held.collect(), "{}").expect("distinct test paths")
    }
    /// The one diagnostic `OPAQUE` bytes provoke, as a report entry located at `at`.
    fn rejected(at: &str) -> Entry {
        let held = Manifest::parse(path(at), OPAQUE);
        let mut errors = held.expect_err("opaque bytes are no manifest").into_vec();
        Entry::Manifest(errors.pop().expect("the schema gate reports one"))
    }
    /// The frozen order is (attributed package id or "", path, code, rendered line), over both
    /// entry kinds: a manifest parse diagnostic interleaves with workspace findings by that same
    /// key rather than grouping ahead of or behind them. The input below is deliberately shuffled
    /// — neither sorted nor reversed, with the diagnostic third of seven — and every component of
    /// the key decides one adjacent pair of the result, so the expected sequence needs a sort.
    #[test]
    fn report_order_is_frozen() {
        let finding = |package: Option<&str>, at, code, claims: &[&str]| {
            let named = claims.iter().copied().map(claim).collect();
            let rule = (code, ESCAPE_TEXT);
            Entry::Workspace(Finding::new(rule, path(at), package.map(id), named))
        };
        let diagnosed = rejected("m.toml").to_string();
        assert!(diagnosed.starts_with("BXW0002 m.toml:"), "{diagnosed}");
        let shuffled = vec![
            finding(Some("zebra"), "a.rs", "BXW0042", &[]),
            finding(None, "z.rs", "BXW0042", &[]),
            rejected("m.toml"),
            finding(Some("alpha"), "a.rs", "BXW0043", &["b/*"]),
            finding(Some("alpha"), "a.rs", "BXW0043", &["a/*", "c/*"]),
            finding(Some("alpha"), "a.rs", "BXW0042", &[]),
            finding(None, "a.rs", "BXW0048", &[]),
        ];
        let findings = Findings::new(shuffled).expect("seven entries are nonempty");
        assert_eq!(
            findings.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0048 a.rs package= candidates=[]",
                diagnosed.as_str(),
                "BXW0042 z.rs package= candidates=[]",
                "BXW0042 a.rs package=alpha candidates=[]",
                "BXW0043 a.rs package=alpha candidates=[owner m.toml a/*,owner m.toml c/*]",
                "BXW0043 a.rs package=alpha candidates=[owner m.toml b/*]",
                "BXW0042 a.rs package=zebra candidates=[]",
            ]
        );
        let last = findings.into_iter().next_back().expect("nonempty");
        assert_eq!(*last, finding(Some("zebra"), "a.rs", "BXW0042", &[]));
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
            let [Entry::Workspace(finding)] = findings.as_slice() else {
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
        let refused = WorkspaceInputs::new(Vec::new(), repeated, "");
        assert_eq!(refused, Err(InputError));
    }
    /// Fixture opacity, the property this design exists for: a claimed `boxology.toml` is pruned
    /// and its held parse result discarded, so the deliberately malformed manifest below — inside
    /// a claimed subtree — produces no entry at all, while its unclaimed sibling stays a package.
    #[test]
    fn claimed_fixture_manifests_are_opaque() {
        let claiming = document("platform", "platform", &["corpus/**"]);
        let manifests = vec![
            ("boxology.toml", claiming),
            ("corpus/bad/boxology.toml", OPAQUE.to_vec()),
            ("corpus/good/boxology.toml", document("good", "box", &[])),
            ("other/boxology.toml", document("other", "box", &[])),
        ];
        let (packages, findings) = workspace(manifests).discover();
        let ids: Vec<&str> = packages.iter().map(|p| p.id().as_str()).collect();
        assert_eq!(ids, ["platform", "other"]);
        assert_eq!(findings, None);
    }
    /// A candidate is a tracked *plain file* whose final segment is exactly `boxology.toml`. Every
    /// path below that is not one carries `OPAQUE` bytes, so treating it as a candidate would put
    /// its parse diagnostic in the report; the empty report is what proves it was never parsed.
    #[test]
    fn only_tracked_plain_manifest_files_are_candidates() {
        let opaque = ["a/xboxology.toml", "b/boxology.tom", "link/boxology.toml"];
        let mut files = vec![FileEntry::symlink(path(opaque[2]), "../a".into())];
        files.extend(["boxology.toml", "a/boxology.toml"].map(|at| FileEntry::file(path(at))));
        files.extend(opaque[..2].iter().map(|at| FileEntry::file(path(at))));
        let mut held = vec![
            (path("boxology.toml"), document("root", "platform", &[])),
            (path("a/boxology.toml"), document("nested", "box", &[])),
        ];
        held.extend(opaque.map(|at| (path(at), OPAQUE.to_vec())));
        let inputs = WorkspaceInputs::new(files, held, "{}").expect("distinct test paths");
        let (packages, findings) = inputs.discover();
        assert_eq!(findings, None);
        let [root, nested] = &packages[..] else {
            panic!("two candidates parse");
        };
        assert_eq!(root.id(), &id("root"));
        assert_eq!(root.manifest_path(), &path("boxology.toml"));
        assert_eq!(root.root(), None);
        assert_eq!(nested.manifest().kind(), Kind::Box);
        assert_eq!(nested.root(), Some(&path("a")));
    }
    /// An unclaimed manifest that fails to parse reports its own T1 diagnostics, unchanged.
    #[test]
    fn unpruned_parse_failures_reach_the_report() {
        let manifests = vec![("a/boxology.toml", OPAQUE.to_vec())];
        let report = workspace(manifests).check().expect("the parse failed");
        assert_eq!(report.to_string(), rejected("a/boxology.toml").to_string());
    }
    /// BXW0042 names every manifest carrying a duplicated identity, each at its own path.
    #[test]
    fn duplicate_identity_names_every_carrier() {
        let manifests = vec![
            ("a/boxology.toml", document("twin", "box", &[])),
            ("b/boxology.toml", document("twin", "box", &[])),
            ("c/boxology.toml", document("alone", "box", &[])),
        ];
        let report = workspace(manifests).check().expect("twin declared twice");
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0042 a/boxology.toml package=twin candidates=[]",
                "BXW0042 b/boxology.toml package=twin candidates=[]",
            ]
        );
    }
    /// BXW0043 reports a `fixtures` pattern claiming its own declaring manifest. The package
    /// survives with every other claim honoured: the malformed manifest under `sub/` below is
    /// still pruned, so the self-claim is the whole report.
    #[test]
    fn self_claiming_fixture_pattern_keeps_its_other_claims() {
        // The claimer is nested, and a prefix-colliding sibling sits beside its claimed subtree:
        // a root taken from the first path segment, or a claim matched by bare prefix rather than
        // by whole segment, silently prunes the wrong manifest and loses its diagnostic.
        let claiming = document("tools", "platform", &[MANIFEST, "a/**"]);
        let manifests = vec![
            ("tools/deep/boxology.toml", claiming),
            ("tools/deep/a/x/boxology.toml", OPAQUE.to_vec()),
            ("tools/deepa/x/boxology.toml", OPAQUE.to_vec()),
        ];
        let (packages, findings) = workspace(manifests).discover();
        let ids: Vec<&str> = packages.iter().map(|p| p.id().as_str()).collect();
        assert_eq!(ids, ["tools"]);
        let report = findings.expect("the self-claim is reported");
        assert_eq!(
            report.to_string(),
            "BXW0002 tools/deepa/x/boxology.toml:2:5-2:5 offending=\"manifest document\" \
             rule=\"boxology.toml must be well-formed TOML\" \
             source=\"specs/s5-manifest-and-validation.md D2\"\n\
             BXW0043 tools/deep/boxology.toml package=tools \
             candidates=[tools tools/deep/boxology.toml boxology.toml]"
        );
    }
    #[test]
    fn public_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(FileEntry, InputError, WorkspaceInputs, Package)>();
        bounds::<(Candidate, Finding, Entry, Findings)>();
    }
}
