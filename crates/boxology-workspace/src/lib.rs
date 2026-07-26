//! Pure workspace inputs, coded findings, and the frozen report order of Boxology workspaces.
//!
//! Callers supply every byte and logical path this crate reads: a file listing, manifest bytes,
//! and a `cargo metadata` document are data arguments. The crate consults no filesystem, process,
//! environment, network, locale, or clock, and it has no uncoded failure path — caller misuse is
//! a typed [`InputError`] and every document defect a coded [`Finding`].
//!
//! **Payload safety.** A rendered finding or classification echoes only grammar-validated values: a
//! [`BoxId`] (`[a-z][a-z0-9-]*`) — a package identity, or a `[[derived]]` element's id, which
//! `boxology_manifest` admits through no other grammar (BXW0031), so the model carries the proof
//! and this crate never re-validates it — a [`RelativePath`] and [`GlobPattern`] (no NUL, tab,
//! line break, or backslash; no `..` in a path). Rejecting line breaks holds one finding to one
//! line; other control bytes reach a report, a residual gap in the grammar this crate consumes.
//! **No value read out of the `cargo metadata` document is echoed at all** — its paths are absolute
//! and its names unvalidated — and a defect of that document is reported at a location this crate
//! names, never at one the document spells.
//! Every other word this crate renders is a `&'static str` it chose. An [`Entry::Manifest`] line is
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
use boxology_manifest::{DerivedOutput, Diagnostic, GlobPattern, Kind, Manifest, RelativePath};
use serde_json::Value;
use std::{cmp::Ordering, fmt};
/// A coded rule: its stable `BXW####` code and the static text of the obligation it states.
type Rule = (&'static str, &'static str);
/// The normative source of the discovery rules this crate enforces.
const WALK_SOURCE: &str = "boxology-details/02-packages.md discovery walk";
/// The normative source of the crate-role vocabulary and the exactly-one matching rule.
const CRATE_SOURCE: &str = "boxology-details/02-packages.md crate roles";
const ESCAPE_TEXT: &str = "symlink targets must stay inside the workspace root";
const ESCAPE: Rule = ("BXW0048", ESCAPE_TEXT);
const DUPLICATE_TEXT: &str = "one package identity must be declared by exactly one manifest";
const DUPLICATE: Rule = ("BXW0042", DUPLICATE_TEXT);
const SELF_CLAIM_TEXT: &str = "a fixtures pattern must not claim its own declaring manifest";
const SELF_CLAIM: Rule = ("BXW0043", SELF_CLAIM_TEXT);
const UNOWNED_TEXT: &str = "every tracked file must classify under some package";
const UNOWNED: Rule = ("BXW0044", UNOWNED_TEXT);
const OVERLAP_TEXT: &str = "at most one package may claim a non-derived path";
const OVERLAP: Rule = ("BXW0045", OVERLAP_TEXT);
const RIVALS_TEXT: &str = "at most one declared derived output may claim a path";
const RIVALS: Rule = ("BXW0046", RIVALS_TEXT);
const BOTH_TEXT: &str = "a declared derived output must not also be claimed as a non-derived path";
const BOTH: Rule = ("BXW0047", BOTH_TEXT);
const LOCK_TEXT: &str = "Cargo.lock must be a platform package's declared global derived artifact";
const LOCK: Rule = ("BXW0049", LOCK_TEXT);
const DOCUMENT_TEXT: &str = "cargo metadata must be a readable workspace document";
const DOCUMENT: Rule = ("BXW0050", DOCUMENT_TEXT);
/// The one file name a package manifest may carry.
const MANIFEST: &str = "boxology.toml";
/// The workspace's own lockfile, spelled as the whole path it is: never one inside a subtree.
const LOCKFILE: &str = "Cargo.lock";
/// The Cargo manifest name: the final segment of every member's `manifest_path`, and — at the
/// workspace root — the one document a defect of the `cargo metadata` document is reported at.
const CARGO_MANIFEST: &str = "Cargo.toml";
// `BXW####` allocation, recorded so this task's slices cannot collide or strand gaps. T1 landed
// BXW0001–BXW0041, the whole schema-1 manifest inventory, so T2 opens at BXW0042 and ends at
// BXW0054: crate-role mapping needs five codes rather than the three first reserved, so its block
// is extended by two — the range stays dense and T3 still opens at BXW0055, as recorded on the
// tracker. Landed: BXW0042 duplicate identity, BXW0043 self-claiming fixture pattern, BXW0044
// unowned path, BXW0045 overlapping ownership, BXW0046 rival derived outputs, BXW0047 derived and
// non-derived at once, BXW0048 symlink escape, BXW0049 the workspace lockfile, BXW0050 an
// unreadable `cargo metadata` document. Allocated: BXW0051 an unmapped Cargo workspace member,
// BXW0052 an unmatched `[[crates]]` entry, BXW0053 a member two entries claim, BXW0054 an
// impossible crate role — the four codes matching this crate's members against those entries
// needs, which is the next slice.
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
    cargo_manifest: RelativePath,
}
impl WorkspaceInputs {
    /// Sorts files and manifests bytewise by path, rejecting a path repeated within either list
    /// as caller misuse. Manifests arrive as **bytes**, not as parsed models, deliberately:
    /// whether a `boxology.toml` is a real package or opaque fixture data depends on the platform
    /// package's `fixtures` patterns, which only this crate computes, so a pre-parsed argument
    /// would make a deliberately malformed corpus manifest fail in the *caller* and defeat
    /// fixture opacity. The bytes are stored unexamined, as is `cargo_metadata`, until discovery
    /// and the metadata reading below take them up.
    pub fn new(
        files: Vec<FileEntry>,
        manifests: Vec<(RelativePath, Vec<u8>)>,
        cargo_metadata: &str,
    ) -> Result<Self, InputError> {
        Ok(Self {
            files: sorted_unique(files, FileEntry::path)?,
            manifests: sorted_unique(manifests, |entry| &entry.0)?,
            cargo_metadata: String::from(cargo_metadata),
            // The one location a defect of the metadata document is reported at, promoted through
            // the caller-facing path grammar exactly here. `RelativePath::new` is fallible over
            // caller data; this argument is a `&'static str` this crate chose, and admitting the
            // impossible rejection as caller misuse is what leaves the crate with no fallible
            // construction, and so no fallback value, on any later path.
            cargo_manifest: RelativePath::new(CARGO_MANIFEST).map_err(|_| InputError)?,
        })
    }
    /// Classifies every tracked file, or reports every defect these inputs prove in the frozen
    /// report order.
    ///
    /// The `Result` is the point: a [`Workspace`] is the *proof* that classification succeeded, so
    /// no caller can read an attribution out of inputs that never earned one, and no `Ok` value can
    /// hold a partial model of a workspace that does not exist.
    ///
    /// Classification runs only over a package set discovery accepted whole. A manifest that did
    /// not parse, or a duplicated identity, declares no usable ownership, so classifying beside it
    /// would answer one located defect with a BXW0044 for every path the missing declarations would
    /// have claimed. Discovery's own findings are returned alone instead. Reading the `cargo
    /// metadata` document needs no package at all, so it runs beside classification and its one
    /// coded defect joins that report rather than pre-empting it.
    pub fn check(&self) -> Result<Workspace, Findings> {
        let (mut packages, found) = self.discover();
        if let Some(findings) = found {
            return Err(findings);
        }
        let (classifications, mut defects) = self.classify(&packages);
        let (cargo_members, found) = self.members();
        defects.extend(found);
        if let Some(findings) = Findings::new(defects) {
            return Err(findings);
        }
        packages.sort_by(|left, right| left.id().cmp(right.id()));
        Ok(Workspace {
            packages,
            classifications,
            cargo_members,
        })
    }
    /// Reads the Cargo workspace members out of the `cargo metadata` document, or reports the one
    /// coded defect an unreadable document is.
    ///
    /// The document is *input*, so every way of failing to read it is BXW0050 and none of them is a
    /// panic: malformed JSON, a missing or mistyped field this crate reads, and a `manifest_path`
    /// that will not normalize under `workspace_root` are the same coded answer, located at the
    /// workspace's own `Cargo.toml` — the document has no other workspace-relative location, and
    /// the absolute paths it carries are never echoed. A defect of the document is the whole
    /// reading: no partial member list escapes it, because matching a manifest's `[[crates]]`
    /// entries against half a workspace would answer one located defect with a finding for every
    /// entry the workspace declares.
    fn members(&self) -> (Vec<CargoMember>, Vec<Entry>) {
        let Some(mut members) = read(&self.cargo_metadata) else {
            let at = self.cargo_manifest.clone();
            let found = Finding::about(DOCUMENT, CRATE_SOURCE, at, None, String::new());
            return (Vec::new(), vec![Entry::Workspace(found)]);
        };
        members.sort_by(|left, right| left.name.cmp(&right.name));
        (members, Vec::new())
    }
    /// Classifies every tracked file under the one claim it has, and reports the files that
    /// classify no times or more than once. D3 admits exactly two kinds of claim, and a path may
    /// hold only one of them: a package claims a path *non-derived* when any `owned` or `fixtures`
    /// pattern of its manifest matches — so a pruned fixture manifest, not a package itself, still
    /// classifies here as the owned file of whichever package claimed it — and claims it *derived*
    /// when an `outputs` pattern of one of its `[[derived]]` elements matches.
    ///
    /// Rival claims of the same kind are BXW0045 (non-derived) and BXW0046 (derived); a path
    /// claimed under both kinds is BXW0047, whether one manifest or two make the two claims. That
    /// is deliberately **one** code, not a same-package and a cross-package one: the rule broken is
    /// the same sentence either way, the candidate list already names each claim with its manifest
    /// path so the distinction is read off the payload rather than off the code, and a split would
    /// have to invent a precedence for the mixed case where one package claims a path both ways and
    /// a second package claims it too.
    fn classify(&self, packages: &[Package]) -> (Vec<FileClassification>, Vec<Entry>) {
        let (mut classified, mut defects) = (Vec::new(), Vec::new());
        for file in &self.files {
            let path = file.path();
            let owning = |package: &&Package| !package.owns(path).is_empty();
            let deriving = |package: &&Package| !package.derives(path).is_empty();
            let claiming = |package: &&Package| owning(package) || deriving(package);
            let rivals: Vec<&Package> = packages.iter().filter(claiming).collect();
            let owners: Vec<&Package> = rivals.iter().copied().filter(owning).collect();
            let mut outputs: Vec<(&Package, &DerivedOutput)> = Vec::new();
            for package in &rivals {
                outputs.extend(package.derives(path).into_iter().map(|o| (*package, o)));
            }
            let every = || every_claim(&rivals, path);
            let attributed = match (outputs.as_slice(), owners.as_slice()) {
                ([(package, output)], []) => Ok((*package, Some(*output))),
                ([], [package]) => Ok((*package, None)),
                ([], []) => Err((UNOWNED, Vec::new())),
                ([], _) => Err((OVERLAP, every())),
                (_, []) => Err((RIVALS, every())),
                (_, _) => Err((BOTH, every())),
            };
            match attributed {
                Ok((package, output)) => {
                    classified.push(FileClassification::new(path, package, output));
                    defects.extend(package.lockfile(path, output));
                }
                Err((rule, named)) => {
                    let found = Finding::new(rule, path.clone(), None, named);
                    defects.push(Entry::Workspace(found));
                }
            }
        }
        classified.sort_by(|left, right| left.key().cmp(&right.key()));
        (classified, defects)
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
        // the report points at every document that has to change, not only at the later one. One
        // line cannot also name its rivals: a `Candidate`'s third component is the *pattern* a
        // claim is made under, and an identity is claimed under no pattern, so naming them would
        // need a fabricated pattern or a widened public type. D3's "naming every candidate" is met
        // here in the one-line-per-carrier sense, and in the payload sense by BXW0045.
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
/// Names every claim every rival makes on `path`. The order is the caller's, never the rendering's:
/// packages in the walk order discovery fixed — ascending `(depth, path)`, so the outermost claimant
/// leads — and within one package its non-derived patterns in declaration order, then every
/// `outputs` pattern of every declared derived output claiming the path, likewise in declaration
/// order. A derived claim names the output it is made under, which is the only thing separating it
/// from a non-derived claim of the same package on the same pattern text.
fn every_claim(rivals: &[&Package], path: &RelativePath) -> Vec<Candidate> {
    let mut named = Vec::new();
    for package in rivals {
        let at = || (package.id().clone(), package.manifest_path.clone());
        for claim in package.owns(path) {
            let (id, manifest) = at();
            named.push(Candidate::new(id, manifest, claim.clone()));
        }
        for output in package.derives(path) {
            for claim in package.matching(path, output.outputs()) {
                let (id, manifest) = at();
                let named_by = output.id().clone();
                named.push(Candidate::derived(id, manifest, claim.clone(), named_by));
            }
        }
    }
    named
}
/// Reads the workspace members of a `cargo metadata` document. `None` is BXW0050: the whole
/// document is a defect, never a partial reading, so every `None` below — malformed JSON, a missing
/// or mistyped field, or a member path [`member`] cannot normalize — is that one coded answer and
/// not a discarded failure.
///
/// Exactly three top-level names are read, and exactly three of each package: D4's declaration-based
/// reading is what makes purity over one document sound, and `resolve` — null under the `--no-deps`
/// invocation T5 owns — is never consulted. A `packages[]` element whose id no `workspace_members`
/// entry spells is a dependency of the workspace, not a member of it: it is skipped before its
/// manifest path is normalized, so a registry package living outside the workspace root is no
/// defect. Ids are matched as opaque strings, never parsed.
fn read(document: &str) -> Option<Vec<CargoMember>> {
    let document: Value = serde_json::from_str(document).ok()?;
    let root = document.get("workspace_root")?.as_str()?;
    let mut ids = Vec::new();
    for id in document.get("workspace_members")?.as_array()? {
        ids.push(id.as_str()?);
    }
    let mut members = Vec::new();
    for package in document.get("packages")?.as_array()? {
        let id = package.get("id")?.as_str()?;
        let name = package.get("name")?.as_str()?;
        let at = package.get("manifest_path")?.as_str()?;
        if ids.contains(&id) {
            members.push(member(root, at, name)?);
        }
    }
    Some(members)
}
/// Normalizes one member's absolute `manifest_path` against the absolute `workspace_root`.
///
/// The root must be followed by a separator, so a sibling root whose name this one is a prefix of
/// never normalizes, and the remainder must be exactly `Cargo.toml` or end in `/Cargo.toml`, whose
/// head is the crate directory — absent at the workspace root, which no [`RelativePath`] can spell.
/// Separators are `/` only: a drive-prefixed or backslash-separated document is BXW0050 rather than
/// a second path dialect, because [`RelativePath`] admits neither and this crate reports no path it
/// has not re-validated.
fn member(root: &str, manifest_path: &str, name: &str) -> Option<CargoMember> {
    let rest = manifest_path.strip_prefix(root)?.strip_prefix('/')?;
    let head = rest.strip_suffix(CARGO_MANIFEST)?;
    let directory = match head {
        "" => None,
        head => Some(RelativePath::new(head.strip_suffix('/')?).ok()?),
    };
    Some(CargoMember {
        manifest_path: RelativePath::new(rest).ok()?,
        directory,
        name: String::from(name),
    })
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
    /// Returns every pattern of `declared` claiming `path`, in declaration order. Matching is
    /// [`GlobPattern::matches`], the single definition of the frozen dialect.
    fn matching<'a>(&self, at: &RelativePath, list: &'a [GlobPattern]) -> Vec<&'a GlobPattern> {
        let Some(under) = self.under(at) else {
            return Vec::new();
        };
        let hit = |pattern: &&GlobPattern| pattern.matches(&under);
        list.iter().filter(hit).collect()
    }
    /// Returns every `fixtures` pattern of this package claiming `path`: the pruning question.
    fn claims(&self, path: &RelativePath) -> Vec<&GlobPattern> {
        self.matching(path, self.manifest.fixtures())
    }
    /// Returns every `owned` or `fixtures` pattern of this package claiming `path`, `owned` first:
    /// the classification question. Fixture data is that package's own owned non-derived material
    /// (D2), so the two lists are one attribution and never two competing ones.
    fn owns(&self, path: &RelativePath) -> Vec<&GlobPattern> {
        let mut claimed = self.matching(path, self.manifest.owned());
        claimed.extend(self.claims(path));
        claimed
    }
    /// Returns every declared derived output of this package whose `outputs` patterns claim `path`,
    /// in declaration order: the other, exclusive half of the classification question. An output
    /// claiming a path under several of its own patterns is still that one output, exactly as a
    /// package claiming a path under several of its own patterns is still that one package.
    fn derives(&self, path: &RelativePath) -> Vec<&DerivedOutput> {
        let hit = |output: &&DerivedOutput| !self.matching(path, output.outputs()).is_empty();
        self.manifest.derived().iter().filter(hit).collect()
    }
    /// Reports BXW0049 when the workspace's own `Cargo.lock` is not this platform package's
    /// declared derived output. The rule is judged on the classification a path actually earned, so
    /// a lockfile no claim or too many claims reached is answered by the finding that denied it one
    /// and never a second time. The match is on the whole workspace-relative path, so a
    /// `Cargo.lock` inside a fixture subtree is untouched by this rule: it is the ordinary owned
    /// non-derived material of whichever package's `fixtures` pattern claimed it.
    fn lockfile(&self, path: &RelativePath, output: Option<&DerivedOutput>) -> Option<Entry> {
        let global = output.is_some() && self.manifest.kind() == Kind::Platform;
        if path.as_str() != LOCKFILE || global {
            return None;
        }
        let owner = Some(self.id().clone());
        let found = Finding::new(LOCK, path.clone(), owner, every_claim(&[self], path));
        Some(Entry::Workspace(found))
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
/// A workspace no finding rejects: every discovered package, the one classification every tracked
/// file has, and every Cargo workspace member the metadata document proved readable. Only [`WorkspaceInputs::check`] builds one, so D3's "every tracked file
/// classifies exactly once" is a property of this type's existence, not a check a consumer repeats.
#[derive(Debug, Eq, PartialEq)]
pub struct Workspace {
    packages: Vec<Package>,
    classifications: Vec<FileClassification>,
    cargo_members: Vec<CargoMember>,
}
impl Workspace {
    ref_getters! {
        #[doc = "Returns every discovered package, sorted by identity."]
        packages: &[Package] = packages;
        #[doc = "Returns every tracked file's classification, in report order."]
        classifications: &[FileClassification] = classifications;
        #[doc = "Returns every Cargo workspace member, sorted by Cargo package name."]
        cargo_members: &[CargoMember] = cargo_members;
    }
    /// Renders the classification body: one line per tracked file, ordered by package identity then
    /// workspace-relative path, exactly as [`Workspace::classifications`] holds them.
    pub fn render_report(&self) -> String {
        let render = FileClassification::render;
        let lines: Vec<String> = self.classifications.iter().map(render).collect();
        lines.join("\n")
    }
}
/// One Cargo workspace member the metadata document names, normalized into this repository's own
/// terms: the crate's directory, its `Cargo.toml` re-validated as a workspace-relative path, and
/// its Cargo package name. Only [`WorkspaceInputs::check`] builds one, so a value of this type is
/// the proof the document was readable — and the absolute paths it carries reach nothing else.
///
/// This is the left-hand side of 02-packages' rule that every Cargo package match exactly one
/// manifest `[[crates]]` entry by normalized manifest path and Cargo package name. The match
/// itself, and the roles it assigns, are the next slice.
#[derive(Debug, Eq, PartialEq)]
pub struct CargoMember {
    manifest_path: RelativePath,
    directory: Option<RelativePath>,
    name: String,
}
impl CargoMember {
    /// Returns the crate's workspace-relative directory. `None` is the workspace root itself, which
    /// no [`RelativePath`] can spell — and which no `[[crates]]` path can spell either, so a Cargo
    /// package sitting there is unmatchable and the next slice codes it.
    pub fn crate_dir(&self) -> Option<&RelativePath> {
        self.directory.as_ref()
    }
    ref_getters! {
        #[doc = "Returns the Cargo package name, exactly as the document spells it."]
        cargo_package: &str = name;
        #[doc = "Returns the workspace-relative path of the crate's own `Cargo.toml`."]
        manifest_path: &RelativePath = manifest_path;
    }
}
/// The one classification of one tracked file: the package accountable for it, and the declaration
/// it is a derived output of when it is one.
#[derive(Debug, Eq, PartialEq)]
pub struct FileClassification {
    path: RelativePath,
    package: BoxId,
    derived_output: Option<BoxId>,
}
impl FileClassification {
    fn new(path: &RelativePath, package: &Package, output: Option<&DerivedOutput>) -> Self {
        Self {
            path: path.clone(),
            package: package.id().clone(),
            derived_output: output.map(|output| output.id().clone()),
        }
    }
    /// Returns the declaration this file is a derived output of, `None` for a non-derived owned
    /// file. The id is a [`BoxId`] rather than free text because it is echoed: `boxology_manifest`
    /// admits a `[[derived]]` element's id through that grammar alone (BXW0031), and this type
    /// carries that proof instead of restating the check or trusting a `String`.
    pub fn derived_output(&self) -> Option<&BoxId> {
        self.derived_output.as_ref()
    }
    ref_getters! {
        #[doc = "Returns the workspace-relative logical path."] path: &RelativePath = path;
        #[doc = "Returns the accountable package."] package: &BoxId = package;
    }
    /// The frozen classification order: package identity, then path. Paths are unique across a
    /// listing, so no tie remains for the rendering to break.
    fn key(&self) -> (&BoxId, &RelativePath) {
        (&self.package, &self.path)
    }
    fn render(&self) -> String {
        let derived = self.derived_output.as_ref().map_or("", BoxId::as_str);
        let (package, path) = (self.package.as_str(), self.path.as_str());
        format!("{package} {path} derived={derived}")
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
pub struct Candidate(BoxId, RelativePath, GlobPattern, Option<BoxId>);
impl Candidate {
    /// Names a package, its manifest, and the pattern under which it claims a path as owned
    /// non-derived material.
    pub fn new(package: BoxId, manifest_path: RelativePath, claim: GlobPattern) -> Self {
        Self(package, manifest_path, claim, None)
    }
    /// [`Candidate::new`], for a claim made under the named declared derived output instead. The
    /// output id is what a report needs to be repairable: one manifest may spell the same pattern
    /// text in `owned` and in two `[[derived]]` elements, and without the id those three claims
    /// render identically and name no document line to change.
    pub fn derived(
        package: BoxId,
        manifest_path: RelativePath,
        claim: GlobPattern,
        output: BoxId,
    ) -> Self {
        Self(package, manifest_path, claim, Some(output))
    }
    /// Returns the derived output the claim is made under, `None` for a non-derived claim.
    pub fn output(&self) -> Option<&BoxId> {
        self.3.as_ref()
    }
    ref_getters! {
        #[doc = "Returns the claiming package identity."] package: &BoxId = 0;
        #[doc = "Returns the claiming manifest's path."] manifest_path: &RelativePath = 1;
        #[doc = "Returns the claiming pattern."] claim: &GlobPattern = 2;
    }
    fn render(&self) -> String {
        let derived = self.3.as_ref().map(|id| format!(" derived={id}"));
        let (at, claim) = (self.1.as_str(), self.2.as_str());
        format!("{} {at} {claim}{}", self.0, derived.unwrap_or_default())
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
        let payload = rendered.join(",");
        let mut found = Self::about(rule, WALK_SOURCE, path, package, payload);
        found.candidates = named;
        found
    }
    /// [`Finding::new`], for a rule from another normative source whose payload names no pattern
    /// claim. A defect of the `cargo metadata` document is about the whole document, not about a
    /// glob some manifest spells, so its [`Finding::candidates`] list is empty and stays so.
    fn about(
        rule: Rule,
        source: &'static str,
        path: RelativePath,
        package: Option<BoxId>,
        payload: String,
    ) -> Self {
        Self {
            package,
            path,
            code: rule.0,
            payload,
            candidates: Vec::new(),
            rule: rule.1,
            rule_source: source,
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
    /// The report key decides the order; the structural comparison below decides only what the key
    /// cannot. Two distinct entries *can* render identically — a comma and a space are literal in
    /// the glob dialect, so one candidate's pattern can spell what two candidates render — and an
    /// `Ord` that called them equal would be inconsistent with `Eq`, silently swallowing one of
    /// them in a `BTreeSet` or a `dedup`. It never reorders a report: it breaks ties the key left.
    fn cmp(&self, other: &Self) -> Ordering {
        let structural = || match (self, other) {
            (Self::Workspace(left), Self::Workspace(right)) => left.cmp(right),
            (Self::Manifest(left), Self::Manifest(right)) => left.cmp(right),
            (Self::Workspace(_), Self::Manifest(_)) => Ordering::Less,
            (Self::Manifest(_), Self::Workspace(_)) => Ordering::Greater,
        };
        self.key().cmp(&other.key()).then_with(structural)
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
    /// A `cargo metadata` document naming no workspace member: what a listing under test that is
    /// not about crate-role mapping supplies, so it reports only what it is about.
    const EMPTY: &str = r#"{"workspace_root":"/w","workspace_members":[],"packages":[]}"#;
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
    /// Inputs carrying one root package that owns every path, so a listing under test reports only
    /// what it is about and never the unowned paths of a workspace with no manifest at all.
    fn inputs(mut entries: Vec<FileEntry>) -> WorkspaceInputs {
        let at = path(MANIFEST);
        entries.push(FileEntry::file(at.clone()));
        let held = vec![(at, owning("root", "platform", &["**"], &[]))];
        WorkspaceInputs::new(entries, held, EMPTY).expect("distinct test paths")
    }
    /// A minimal valid manifest, carrying the given `fixtures` list when it is nonempty.
    fn document(id: &str, kind: &str, fixtures: &[&str]) -> Vec<u8> {
        owning(id, kind, &[], fixtures)
    }
    /// [`document`], declaring an `owned` list as well: what the package actually claims.
    fn owning(id: &str, kind: &str, owned: &[&str], fixtures: &[&str]) -> Vec<u8> {
        let list = |patterns: &[&str]| {
            let quoted: Vec<String> = patterns.iter().map(|p| format!("{p:?}")).collect();
            quoted.join(", ")
        };
        let head = format!("schema = 1\nid = {id:?}\nkind = {kind:?}\n");
        let mut text = format!("{head}owned = [{}]\n", list(owned));
        if !fixtures.is_empty() {
            text.push_str(&format!("fixtures = [{}]\n", list(fixtures)));
        }
        text.into_bytes()
    }
    /// A manifest document with `[[derived]]` elements appended, each one an output id and the
    /// `outputs` patterns it declares, in declaration order. Every element declares the same fixed
    /// nonempty `inputs`, which this slice never reads.
    fn deriving(base: Vec<u8>, outputs: &[(&str, &[&str])]) -> Vec<u8> {
        let mut text = String::from_utf8(base).expect("test manifests are ASCII");
        for (id, patterns) in outputs {
            let quoted: Vec<String> = patterns.iter().map(|p| format!("{p:?}")).collect();
            let head = format!("[[derived]]\nid = {id:?}\ngenerator = \"cargo\"\n");
            let lists = format!(
                "inputs = [{MANIFEST:?}]\noutputs = [{}]\n",
                quoted.join(", ")
            );
            text.push_str(&head);
            text.push_str(&lists);
        }
        text.into_bytes()
    }
    /// Inputs whose listing tracks every named manifest as a plain file.
    fn workspace(manifests: Vec<(&str, Vec<u8>)>) -> WorkspaceInputs {
        listing(manifests, &[])
    }
    /// [`workspace`], plus `tracked` ordinary files the listing also carries.
    fn listing(manifests: Vec<(&str, Vec<u8>)>, tracked: &[&str]) -> WorkspaceInputs {
        mapped(manifests, tracked, EMPTY)
    }
    /// [`listing`], carrying a `cargo metadata` document crate-role mapping actually reads.
    fn mapped(held: Vec<(&str, Vec<u8>)>, tracked: &[&str], document: &str) -> WorkspaceInputs {
        let entry = |(at, _): &(&str, Vec<u8>)| FileEntry::file(path(at));
        let mut files: Vec<FileEntry> = held.iter().map(entry).collect();
        files.extend(tracked.iter().map(|at| FileEntry::file(path(at))));
        let manifests = held.into_iter().map(|(at, bytes)| (path(at), bytes));
        WorkspaceInputs::new(files, manifests.collect(), document).expect("distinct test paths")
    }
    /// A `cargo metadata` document under workspace root `/w`, naming one member per `(directory,
    /// Cargo package name)` pair — the empty directory being the workspace root itself — plus
    /// `strangers`, raw `packages[]` elements that no `workspace_members` entry names.
    fn metadata(members: &[(&str, &str)], strangers: &[&str]) -> String {
        let (mut ids, mut packages) = (Vec::new(), Vec::from(strangers).join(","));
        for (at, name) in members {
            let head = if at.is_empty() {
                String::new()
            } else {
                format!("{at}/")
            };
            let id = format!("path+file:///w/{head}#0.0.0");
            let one = format!("\"name\":{name:?},\"manifest_path\":\"/w/{head}Cargo.toml\"");
            packages.push_str(&format!(",{{\"id\":{id:?},{one}}}"));
            ids.push(format!("{id:?}"));
        }
        let listed = ids.join(",");
        let body = packages.trim_start_matches(',');
        format!(r#"{{"workspace_root":"/w","workspace_members":[{listed}],"packages":[{body}]}}"#)
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
            let Err(findings) = inputs(vec![entry]).check() else {
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
            assert_eq!(
                finding.rule(),
                "symlink targets must stay inside the workspace root"
            );
            assert_eq!(finding.rule_source(), WALK_SOURCE, "{case:?}");
        }
        // The exact rendering, over a target chosen to wreck a report were it ever echoed.
        let hostile = FileEntry::symlink(path("a/link"), "../../\npackage=x".into());
        let report = inputs(vec![hostile]).check().expect_err("an escape");
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
        let report = workspace(manifests).check().expect_err("the parse failed");
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
        let report = workspace(manifests)
            .check()
            .expect_err("twin declared twice");
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
    /// D3: every tracked file classifies exactly once. The fixture is built so a wrong answer is
    /// reachable — the claiming package is nested two levels deep; `pkg/deepsrc/a.rs` is a
    /// prefix-colliding sibling of its root that a bare-prefix re-anchoring would hand it; and the
    /// pruned fixture manifest, which is no package, is still a tracked path that must classify.
    #[test]
    fn every_tracked_file_classifies_exactly_once() {
        let claimed = [MANIFEST, "pkg/deepsrc/*"];
        let deep = owning("deep", "box", &[MANIFEST, "src/*.rs"], &[]);
        let manifests = vec![
            (
                "boxology.toml",
                owning("root", "platform", &claimed, &["corpus/**"]),
            ),
            ("corpus/bad/boxology.toml", OPAQUE.to_vec()),
            ("pkg/deep/boxology.toml", deep),
        ];
        let inputs = listing(manifests, &["pkg/deep/src/a.rs", "pkg/deepsrc/a.rs"]);
        let checked = inputs.check().expect("each path classifies");
        let ids: Vec<&str> = checked.packages().iter().map(|p| p.id().as_str()).collect();
        // Walk order is ("root", "deep"); `packages` is sorted by identity, so this is not it.
        assert_eq!(ids, ["deep", "root"]);
        let seen: Vec<(&str, &str, Option<&str>)> = checked
            .classifications()
            .iter()
            .map(|c| {
                let derived = c.derived_output().map(BoxId::as_str);
                (c.package().as_str(), c.path().as_str(), derived)
            })
            .collect();
        assert_eq!(
            seen,
            [
                ("deep", "pkg/deep/boxology.toml", None),
                ("deep", "pkg/deep/src/a.rs", None),
                ("root", "boxology.toml", None),
                ("root", "corpus/bad/boxology.toml", None),
                ("root", "pkg/deepsrc/a.rs", None),
            ]
        );
        assert_eq!(
            checked.render_report().lines().collect::<Vec<_>>(),
            [
                "deep pkg/deep/boxology.toml derived=",
                "deep pkg/deep/src/a.rs derived=",
                "root boxology.toml derived=",
                "root corpus/bad/boxology.toml derived=",
                "root pkg/deepsrc/a.rs derived=",
            ]
        );
    }
    /// BXW0044 codes a path no package claims; BXW0045 codes one two packages claim, naming every
    /// claim of every rival. Both payloads below are deliberately *unsorted*: `zulu` is the root
    /// package and leads because the walk reached it first, not because "zulu" sorts first, and its
    /// two claims on `pkg/x/f.rs` render in declaration order, not in pattern order.
    #[test]
    fn unowned_and_overlapping_paths_are_coded() {
        let owned = [MANIFEST, "pkg/x/*.rs", "pkg/**"];
        let alpha = owning("alpha", "box", &[MANIFEST, "f.rs"], &[]);
        let zulu = owning("zulu", "platform", &owned, &[]);
        let manifests = vec![("boxology.toml", zulu), ("pkg/x/boxology.toml", alpha)];
        let inputs = listing(manifests, &["orphan.rs", "pkg/x/f.rs"]);
        let report = inputs.check().expect_err("one orphan and two overlaps");
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0044 orphan.rs package= candidates=[]",
                "BXW0045 pkg/x/boxology.toml package= candidates=[zulu boxology.toml pkg/**,\
                 alpha pkg/x/boxology.toml boxology.toml]",
                "BXW0045 pkg/x/f.rs package= candidates=[zulu boxology.toml pkg/x/*.rs,\
                 zulu boxology.toml pkg/**,alpha pkg/x/boxology.toml f.rs]",
            ]
        );
        let [Entry::Workspace(unowned), _, Entry::Workspace(overlap)] = report.as_slice() else {
            panic!("three workspace findings: {report}");
        };
        assert_eq!(unowned.code(), "BXW0044");
        assert_eq!(unowned.path(), &path("orphan.rs"));
        assert_eq!(unowned.package(), None);
        assert_eq!(unowned.candidates(), []);
        assert_eq!(
            unowned.rule(),
            "every tracked file must classify under some package"
        );
        assert_eq!(unowned.rule_source(), WALK_SOURCE);
        assert_eq!(
            overlap.rule(),
            "at most one package may claim a non-derived path"
        );
        assert_eq!(
            overlap.package(),
            None,
            "no rival is accountable for the other"
        );
        let named: Vec<(&str, &str, &str)> = overlap
            .candidates()
            .iter()
            .map(|c| {
                (
                    c.package().as_str(),
                    c.manifest_path().as_str(),
                    c.claim().as_str(),
                )
            })
            .collect();
        assert_eq!(
            named,
            [
                ("zulu", "boxology.toml", "pkg/x/*.rs"),
                ("zulu", "boxology.toml", "pkg/**"),
                ("alpha", "pkg/x/boxology.toml", "f.rs"),
            ]
        );
    }
    /// Two distinct entries can render identically: a comma and a space are literal in the glob
    /// dialect, so one candidate's pattern spells what two candidates render. The order must still
    /// separate them, or a `BTreeSet` or `dedup` over a report would swallow one.
    #[test]
    fn identically_rendered_entries_stay_distinct() {
        let entry = |named| Entry::Workspace(Finding::new(UNOWNED, path("a.rs"), None, named));
        let one = entry(vec![claim("a,owner m.toml b")]);
        let two = entry(vec![claim("a"), claim("b")]);
        assert_eq!(one.to_string(), two.to_string());
        assert_ne!(one, two);
        assert_ne!(one.cmp(&two), Ordering::Equal);
    }
    /// D3's "exactly once" counts *packages*, not matches: `solo/y.rs` below is claimed by one
    /// package under two of its own patterns and still classifies, while `a/x.rs` is claimed by
    /// two packages and is the whole report. Within one package `owned` names precede `fixtures`.
    #[test]
    fn one_package_claiming_a_path_twice_is_one_claim() {
        let owned = [MANIFEST, "a/*.rs", "solo/**"];
        let zulu = owning("zulu", "platform", &owned, &["a/x.rs", "solo/*.rs"]);
        let alpha = owning("alpha", "box", &[MANIFEST, "x.rs"], &[]);
        let manifests = vec![("boxology.toml", zulu), ("a/boxology.toml", alpha)];
        let inputs = listing(manifests, &["a/x.rs", "solo/y.rs"]);
        let report = inputs.check().expect_err("only a/x.rs overlaps");
        assert_eq!(
            report.to_string(),
            "BXW0045 a/x.rs package= candidates=[zulu boxology.toml a/*.rs,\
             zulu boxology.toml a/x.rs,alpha a/boxology.toml x.rs]"
        );
    }
    /// D3's other classification: a path an `outputs` pattern claims is *that declared output*, and
    /// its id reaches the rendered line. The fixture is built so a wrong answer is reachable — the
    /// declaring package is nested, so its outputs re-anchor at its own root and not at the
    /// workspace root; `gen` is a strict prefix of `gen-extra`, so a prefix comparison anywhere
    /// answers with the wrong id; and `pkg/api/v1.rs` is claimed by *two* patterns of the same
    /// output, which is one claim, exactly as two patterns of one package are.
    #[test]
    fn declared_outputs_classify_as_that_output() {
        let outputs: [(&str, &[&str]); 2] = [
            ("gen-extra", &["extra/*.rs"]),
            ("gen", &["api/**", "api/v1.rs"]),
        ];
        let deep = deriving(
            owning("deep", "box", &[MANIFEST, "src/*.rs"], &[]),
            &outputs,
        );
        let root = owning("root", "platform", &[MANIFEST], &[]);
        let manifests = vec![("boxology.toml", root), ("pkg/boxology.toml", deep)];
        let tracked = ["pkg/api/v1.rs", "pkg/extra/e.rs", "pkg/src/a.rs"];
        let checked = listing(manifests, &tracked)
            .check()
            .expect("each classifies");
        assert_eq!(
            checked.render_report().lines().collect::<Vec<_>>(),
            [
                "deep pkg/api/v1.rs derived=gen",
                "deep pkg/boxology.toml derived=",
                "deep pkg/extra/e.rs derived=gen-extra",
                "deep pkg/src/a.rs derived=",
                "root boxology.toml derived=",
            ]
        );
        let first = &checked.classifications()[0];
        assert_eq!(first.derived_output(), Some(&id("gen")));
        assert_eq!(first.package(), &id("deep"));
        assert_eq!(checked.classifications()[1].derived_output(), None);
    }
    /// BXW0046 codes a path more than one declared derived output claims, naming every one. The
    /// same-manifest pair below spells the *identical pattern text* in both elements, so nothing
    /// but the named output id separates the two claims in the report.
    #[test]
    fn rival_derived_outputs_are_coded() {
        let owned = [MANIFEST, "pkg/boxology.toml"];
        let claims: [(&str, &[&str]); 3] = [
            ("one", &["gen/*.rs"]),
            ("two", &["gen/*.rs"]),
            ("three", &["pkg/y.rs"]),
        ];
        let zulu = deriving(owning("zulu", "platform", &owned, &[]), &claims);
        let alpha = deriving(owning("alpha", "box", &[], &[]), &[("out", &["y.rs"])]);
        let manifests = vec![("boxology.toml", zulu), ("pkg/boxology.toml", alpha)];
        let inputs = listing(manifests, &["gen/a.rs", "pkg/y.rs"]);
        let report = inputs.check().expect_err("two paths are claimed twice");
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0046 gen/a.rs package= candidates=[zulu boxology.toml gen/*.rs derived=one,\
                 zulu boxology.toml gen/*.rs derived=two]",
                "BXW0046 pkg/y.rs package= candidates=[zulu boxology.toml pkg/y.rs derived=three,\
                 alpha pkg/boxology.toml y.rs derived=out]",
            ]
        );
        let [Entry::Workspace(rivals), _] = report.as_slice() else {
            panic!("two workspace findings: {report}");
        };
        assert_eq!(rivals.code(), "BXW0046");
        assert_eq!(rivals.path(), &path("gen/a.rs"));
        assert_eq!(rivals.package(), None);
        assert_eq!(
            rivals.rule(),
            "at most one declared derived output may claim a path"
        );
        assert_eq!(rivals.rule_source(), WALK_SOURCE);
        let named: Vec<Option<&BoxId>> =
            rivals.candidates().iter().map(Candidate::output).collect();
        assert_eq!(named, [Some(&id("one")), Some(&id("two"))]);
    }
    /// BXW0047 codes a path claimed as owned non-derived material *and* as a declared derived
    /// output. One code covers both the same-manifest case — `solo/a.rs`, where `owned` and a
    /// `[[derived]]` element spell the identical pattern, and `fix/a.rs`, where the non-derived
    /// claim comes from `fixtures` rather than `owned` — and the cross-manifest one, `pkg/b.rs`.
    #[test]
    fn a_path_owned_and_derived_at_once_is_coded() {
        let owned = [MANIFEST, "solo/*.rs", "pkg/b.rs", "pkg/boxology.toml"];
        let claims: [(&str, &[&str]); 2] = [("fix", &["fix/a.rs"]), ("gen", &["solo/*.rs"])];
        let zulu = deriving(owning("zulu", "platform", &owned, &["fix/**"]), &claims);
        let alpha = deriving(
            owning("alpha", "box", &["b.rs"], &[]),
            &[("out", &["b.rs"])],
        );
        let manifests = vec![("boxology.toml", zulu), ("pkg/boxology.toml", alpha)];
        let tracked = ["fix/a.rs", "pkg/b.rs", "solo/a.rs"];
        let report = listing(manifests, &tracked)
            .check()
            .expect_err("three collisions");
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0047 fix/a.rs package= candidates=[zulu boxology.toml fix/**,\
                 zulu boxology.toml fix/a.rs derived=fix]",
                "BXW0047 pkg/b.rs package= candidates=[zulu boxology.toml pkg/b.rs,\
                 alpha pkg/boxology.toml b.rs,alpha pkg/boxology.toml b.rs derived=out]",
                "BXW0047 solo/a.rs package= candidates=[zulu boxology.toml solo/*.rs,\
                 zulu boxology.toml solo/*.rs derived=gen]",
            ]
        );
        let [_, _, Entry::Workspace(both)] = report.as_slice() else {
            panic!("three workspace findings: {report}");
        };
        assert_eq!(both.code(), "BXW0047");
        assert_eq!(both.package(), None, "neither claim is accountable");
        assert_eq!(
            both.rule(),
            "a declared derived output must not also be claimed as a non-derived path"
        );
        assert_eq!(both.rule_source(), WALK_SOURCE);
        let named: Vec<Option<&BoxId>> = both.candidates().iter().map(Candidate::output).collect();
        assert_eq!(named, [None, Some(&id("gen"))]);
    }
    /// D3's lockfile sentence: the workspace's `Cargo.lock` classifies as the platform package's
    /// declared global derived artifact, and nothing else satisfies the rule. The green case is
    /// declared by the package sitting *at the workspace root*, whose patterns anchor at no
    /// directory at all, and the same listing carries a `Cargo.lock` **inside a fixture subtree** —
    /// ordinary owned non-derived material — which this rule must leave entirely alone.
    #[test]
    fn the_workspace_lockfile_is_a_platform_derived_output() {
        let lock: [(&str, &[&str]); 1] = [("lockfile", &[LOCKFILE])];
        let owned = [MANIFEST, "Cargo.toml", LOCKFILE];
        let declared = owning("root", "platform", &owned[..2], &["corpus/**"]);
        let root = deriving(declared, &lock);
        let tracked = ["Cargo.lock", "Cargo.toml", "corpus/p/Cargo.lock"];
        let held = vec![("boxology.toml", root)];
        let checked = listing(held, &tracked).check().expect("each classifies");
        assert_eq!(
            checked.render_report().lines().collect::<Vec<_>>(),
            [
                "root Cargo.lock derived=lockfile",
                "root Cargo.toml derived=",
                "root boxology.toml derived=",
                "root corpus/p/Cargo.lock derived=",
            ]
        );
        // Claimed by the right package, in the wrong way: owned non-derived instead of declared.
        let plain = owning("root", "platform", &owned, &["corpus/**"]);
        let report = listing(vec![("boxology.toml", plain)], &tracked)
            .check()
            .expect_err("the lockfile is not declared derived");
        assert_eq!(
            report.to_string(),
            "BXW0049 Cargo.lock package=root candidates=[root boxology.toml Cargo.lock]"
        );
        let [Entry::Workspace(found)] = report.as_slice() else {
            panic!("one workspace finding: {report}");
        };
        assert_eq!(found.code(), "BXW0049");
        assert_eq!(found.path(), &path(LOCKFILE));
        assert_eq!(found.package(), Some(&id("root")));
        assert_eq!(
            found.rule(),
            "Cargo.lock must be a platform package's declared global derived artifact"
        );
        assert_eq!(found.rule_source(), WALK_SOURCE);
        // Declared derived, by a package that is not the platform. Only a workspace-root manifest
        // can reach the path at all, so the wrong-kind case is a root manifest of the wrong kind.
        let boxed = deriving(owning("root", "box", &owned[..2], &[]), &lock);
        let report = listing(vec![("boxology.toml", boxed)], &tracked[..2])
            .check()
            .expect_err("a box package declares no global artifact");
        assert_eq!(
            report.to_string(),
            "BXW0049 Cargo.lock package=root \
             candidates=[root boxology.toml Cargo.lock derived=lockfile]"
        );
        // A composition root is equally not the platform: the rule names one kind, not "not a box".
        let composed = deriving(owning("root", "composition", &owned[..2], &[]), &lock);
        let mut text = String::from_utf8(composed).expect("test manifests are ASCII");
        text.push_str("[composition]\nboxes = [\"hello\"]\n");
        let report = listing(vec![("boxology.toml", text.into_bytes())], &tracked[..2])
            .check()
            .expect_err("a composition package declares no global artifact");
        assert_eq!(
            report.to_string(),
            "BXW0049 Cargo.lock package=root \
             candidates=[root boxology.toml Cargo.lock derived=lockfile]"
        );
    }
    /// 02-packages' left-hand side: the checker reads Cargo metadata, selects the *workspace
    /// members*, and normalizes each `manifest_path` against `workspace_root`. The fixture is built
    /// so a wrong answer is reachable — `crates/foo` is a strict prefix of `crates/foo-bar`, so a
    /// prefix comparison anywhere loses one; the members are declared out of sorted order; one sits
    /// at the workspace root, whose directory no `RelativePath` can spell; and `packages[]` carries
    /// two elements no `workspace_members` entry names, one a registry package whose *name collides
    /// with a member's* and one under a sibling root `/w2` that is not `/w`, so selecting by name,
    /// or normalizing before selecting, either doubles a member or fails the whole document.
    #[test]
    fn cargo_members_are_selected_and_normalized() {
        let out = "/home/u/.cargo/registry/src/index/foo-1.0.0/Cargo.toml";
        let registry = format!(r#"{{"id":"registry-foo","name":"foo","manifest_path":{out:?}}}"#);
        let sibling = r#"{"id":"sibling","name":"other","manifest_path":"/w2/crates/foo/x.toml"}"#;
        let members = [
            ("tools", "root-tools"),
            ("crates/foo-bar", "foo-bar"),
            ("", "whole-workspace"),
            ("crates/foo", "foo"),
        ];
        let held = vec![(MANIFEST, owning("root", "platform", &[MANIFEST], &[]))];
        let document = metadata(&members, &[registry.as_str(), sibling]);
        let checked = mapped(held, &[], &document)
            .check()
            .expect("a clean listing");
        let seen: Vec<(&str, &str, &str)> = checked
            .cargo_members()
            .iter()
            .map(|member| {
                let at = member.crate_dir().map_or("", RelativePath::as_str);
                (member.cargo_package(), at, member.manifest_path().as_str())
            })
            .collect();
        assert_eq!(
            seen,
            [
                ("foo", "crates/foo", "crates/foo/Cargo.toml"),
                ("foo-bar", "crates/foo-bar", "crates/foo-bar/Cargo.toml"),
                ("root-tools", "tools", "tools/Cargo.toml"),
                ("whole-workspace", "", "Cargo.toml"),
            ]
        );
    }
    /// BXW0050 codes every defect of the metadata document itself, including a `manifest_path` that
    /// will not normalize under `workspace_root`. Every case below is the *same* rendered line: no
    /// partial member list survives a defect, and no byte of the document — least of all one of its
    /// absolute paths — reaches the report.
    #[test]
    fn unreadable_metadata_documents_are_coded() {
        // A one-member document, from its whole `packages[]` element or from that member's path.
        let named = |package: &str| {
            format!(
                r#"{{"workspace_root":"/w","workspace_members":["i"],"packages":[{{{package}}}]}}"#
            )
        };
        let one = |at: &str| {
            named(&format!(
                r#""id":"i","name":"solo-crate","manifest_path":{at:?}"#
            ))
        };
        // Absolute `manifest_path` values, `!`-prefixed when the document is readable.
        let paths = "/w2/crate/Cargo.toml,/other/crate/Cargo.toml,/wCargo.toml,\
                     /w/crateCargo.toml,/w/crate/cargo.toml,/w/../x/Cargo.toml,/w/./Cargo.toml,\
                     /w/a\\b/Cargo.toml,\
                     !/w/crate/Cargo.toml,!/w/Cargo.toml";
        let mut cases: Vec<String> = paths.split(',').map(String::from).collect();
        cases.extend(
            [
                "not json",
                "[]",
                r#"{"workspace_members":[],"packages":[]}"#,
                r#"{"workspace_root":7,"workspace_members":[],"packages":[]}"#,
                r#"{"workspace_root":"/w","packages":[]}"#,
                r#"{"workspace_root":"/w","workspace_members":7,"packages":[]}"#,
                r#"{"workspace_root":"/w","workspace_members":[7],"packages":[]}"#,
                r#"{"workspace_root":"/w","workspace_members":[]}"#,
                r#"{"workspace_root":"/w","workspace_members":[],"packages":{}}"#,
                r#"{"workspace_root":"/w","workspace_members":["i"],"packages":[{"id":"i"}]}"#,
                &named(r#""id":"i","name":"n""#),
                &named(r#""name":"n","manifest_path":"/w/a/Cargo.toml""#),
                &named(r#""id":"i","name":7,"manifest_path":"/w/a/Cargo.toml""#),
                &named(r#""id":7,"name":"n","manifest_path":"/w/a/Cargo.toml""#),
                &named(r#""id":"i","name":"n","manifest_path":7"#),
                r#"{"workspace_root":"/w/","workspace_members":["i"],"packages":[{"id":"i",
                   "name":"n","manifest_path":"/w/a/Cargo.toml"}]}"#,
            ]
            .map(String::from),
        );
        for case in &cases {
            let readable = case.strip_prefix('!');
            let document = match readable.unwrap_or(case) {
                whole if whole.starts_with('/') => one(whole),
                whole => String::from(whole),
            };
            let held = vec![(MANIFEST, owning("solo", "platform", &[MANIFEST], &[]))];
            let Err(report) = mapped(held, &[], &document).check() else {
                // `/w/crate/Cargo.toml` and `/w/Cargo.toml` normalize; nothing else here does.
                assert!(readable.is_some(), "{case:?} is unreadable");
                continue;
            };
            assert!(
                readable.is_none(),
                "{case:?} is readable, and reported {report}"
            );
            let [Entry::Workspace(found)] = report.as_slice() else {
                panic!("{case:?} reported {report}");
            };
            assert_eq!(found.code(), "BXW0050", "{case:?}");
            assert_eq!(found.path(), &path(CARGO_MANIFEST), "{case:?}");
            assert_eq!(found.package(), None, "{case:?}");
            assert_eq!(found.candidates(), [], "{case:?}");
            // The literal text, not the constant: an assertion against the constant it guards is
            // green for every value that constant could hold, which proves nothing about either.
            let stated = "cargo metadata must be a readable workspace document";
            assert_eq!(found.rule(), stated, "{case:?}");
            let source = "boxology-details/02-packages.md crate roles";
            assert_eq!(found.rule_source(), source, "{case:?}");
            assert_eq!(
                report.to_string(),
                "BXW0050 Cargo.toml package= candidates=[]",
                "{case:?} must yield one line, echoing no byte of the document"
            );
        }
    }
    /// A defect of the metadata document joins the classification report rather than pre-empting
    /// it, and takes its place in the frozen order like any other unattributed finding.
    #[test]
    fn an_unreadable_document_joins_the_classification_report() {
        let held = vec![(MANIFEST, owning("solo", "platform", &["only.txt"], &[]))];
        let report = mapped(held, &["unowned.txt"], "not json")
            .check()
            .expect_err("both defects are reported");
        assert_eq!(
            report.to_string(),
            "BXW0050 Cargo.toml package= candidates=[]\n\
             BXW0044 boxology.toml package= candidates=[]\n\
             BXW0044 unowned.txt package= candidates=[]"
        );
    }
    #[test]
    fn public_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(FileEntry, InputError, WorkspaceInputs, Package)>();
        bounds::<(Candidate, Finding, Entry, Findings)>();
        bounds::<(Workspace, FileClassification, CargoMember)>();
    }
}
