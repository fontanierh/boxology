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
//! and this crate never re-validates it — a [`RelativePath`] and [`GlobPattern`] (no backslash and
//! no ASCII control byte, the whole C0 range and DEL; no `..` in a path). Rejecting line breaks
//! holds one finding to one line, and rejecting the rest of the C0 range is what makes a payload
//! safe to write to a terminal: no escape sequence, bell, or carriage return can reach a report
//! through a path or a pattern. The residual is narrow and named: the C1 range (`U+0080`-`U+009F`)
//! is multi-byte in UTF-8 and survives a bytewise grammar.
//! **No value the `cargo metadata` document spells is echoed as text** — its names are unvalidated
//! and its paths absolute — and a defect of that document is reported at a location this crate
//! names. The one document-derived value a report carries is a member's `manifest_path`
//! **re-validated as a [`RelativePath`]**, locating the member a workspace fails to map: it carries
//! that grammar's proof like every other echoed path, and a package name reaches no report at all.
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
use boxology_manifest::{CrateEntry, CrateRole, DerivedOutput, Diagnostic, GlobPattern};
use boxology_manifest::{Kind, Manifest, RelativePath};
use serde_json::Value;
use std::{cmp::Ordering, fmt};
/// A coded rule: its stable `BXW####` code and the static text of the obligation it states.
type Rule = (&'static str, &'static str);
/// The normative source of the discovery rules this crate enforces.
const WALK_SOURCE: &str = "boxology-details/02-packages.md discovery walk";
/// The normative source of the crate-role vocabulary and the exactly-one matching rule.
const CRATE_SOURCE: &str = "boxology-details/02-packages.md crate roles";
/// The normative source of the two obligations 02-packages does *not* state: it requires every
/// Cargo package to match one entry, says nothing about the converse, and delegates crate-role
/// policy to 08-topology. This spec states both — unmatched crates and role mismatches are failures.
const D4_SOURCE: &str = "specs/s5-manifest-and-validation.md D4";
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
const UNMAPPED_TEXT: &str = "every Cargo workspace member must match one declared crate entry";
const UNMAPPED: Rule = ("BXW0051", UNMAPPED_TEXT);
const UNMATCHED_TEXT: &str = "every declared crate entry must match one Cargo workspace member";
const UNMATCHED: Rule = ("BXW0052", UNMATCHED_TEXT);
const CLAIMED_TEXT: &str = "at most one declared crate entry may match a Cargo workspace member";
const CLAIMED: Rule = ("BXW0053", CLAIMED_TEXT);
const ROLE_TEXT: &str = "a declared crate role must be one its package kind can host";
const ROLE: Rule = ("BXW0054", ROLE_TEXT);
/// The normative source of the role-pair edge table this crate's edge policy enforces.
const EDGE_SOURCE: &str = "boxology-details/08-rust-build-topology.md edge table";
/// A coded edge rule paired with its authority. BXW0055–BXW0057 use 08 where directly stated;
/// BXW0058 binds through S5 D4 because 08 supplies X→I while #325/S5-D4 supplies X→C.
/// BXW0059/0060 also bind through S5 D4; S7 D4/#325 supports BXW0060's adoption inference.
type EdgeRule = (Rule, &'static str);
const CONTRACT_TEXT: &str = "a box contract crate must depend on no box implementation";
const CONTRACT: EdgeRule = (("BXW0055", CONTRACT_TEXT), EDGE_SOURCE);
const FOREIGN_TEXT: &str = "a box implementation must depend on no foreign box implementation";
const FOREIGN: EdgeRule = (("BXW0056", FOREIGN_TEXT), EDGE_SOURCE);
const DECLARED_TEXT: &str = "a box crate's edge to a foreign contract must be a declared import";
const DECLARED: EdgeRule = (("BXW0057", DECLARED_TEXT), EDGE_SOURCE);
const SELECTED_TEXT: &str = "a composition edge must target a selected box";
const SELECTED: EdgeRule = (("BXW0058", SELECTED_TEXT), D4_SOURCE);
// Scope is load-bearing: inferred same-package C→C is BXW0059, while declared foreign C→C is legal.
const IMPOSSIBLE_TEXT: &str =
    "no rule permits an edge between these crate roles at this package scope";
const IMPOSSIBLE: EdgeRule = (("BXW0059", IMPOSSIBLE_TEXT), D4_SOURCE);
const NON_MEMBER_TEXT: &str =
    "a path dependency onto a non-member is allowed only from a platform crate";
const NON_MEMBER: EdgeRule = (("BXW0060", NON_MEMBER_TEXT), D4_SOURCE);
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
// unreadable `cargo metadata` document, BXW0051 an unmapped Cargo workspace member, BXW0052 an
// unmatched `[[crates]]` entry, BXW0053 a member two entries claim, BXW0054 an impossible role.
// T3 then closes densely at BXW0055–BXW0060: contract, foreign implementation, undeclared
// contract, unselected composition, impossible role/scope, and non-member respectively.
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
        let (cargo_members, unreadable) = self.members();
        // A declared role is a property of one manifest alone, so it is judged whether or not the
        // document read. Matching needs the members, and an unreadable document proves none:
        // a BXW0052 per declared entry beside BXW0050 answers one located defect with a cascade.
        defects.extend(roles(&packages));
        if unreadable.is_empty() {
            // The edge policy reads the association matching just computed, so it is gated on the
            // same readable document and needs no second one: an unread document proves no member,
            // and so no edge either.
            let (roled, unmatched) = map(&packages, &cargo_members);
            defects.extend(unmatched);
            defects.extend(edges(&roled, &cargo_members));
        }
        defects.extend(unreadable);
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
/// Reports whether `entry`, declared by `package`, names `member`.
///
/// 02-packages fixes the match as a **pair**: "every Cargo package [matches] exactly one manifest
/// `[[crates]]` entry by normalized manifest path and Cargo package name". An entry whose
/// `cargo_package` names a member but whose `path` locates another directory therefore matches
/// nothing, and the member is unmapped rather than adopted by the half that agreed. The entry's
/// path is package-relative, so the member's directory re-anchors at the declaring package's own
/// root — as every pattern of that manifest does — and a member outside the package, at the
/// workspace root, or at the package's own root re-anchors to nothing.
fn maps(package: &Package, entry: &CrateEntry, member: &CargoMember) -> bool {
    let at = member.crate_dir().and_then(|dir| package.under(dir));
    entry.cargo_package() == member.cargo_package() && at.as_ref() == Some(entry.path())
}
/// Names one claiming entry: its package identity, its declaring manifest, and the package-relative
/// directory it spells — three grammar-validated values, and no byte of the metadata document.
fn spell(package: &Package, entry: &CrateEntry) -> String {
    let (at, dir) = (package.manifest_path.as_str(), entry.path().as_str());
    format!("{} {at} {dir}", package.id())
}
/// Matches every Cargo member against every declared `[[crates]]` entry: BXW0051 a member no entry
/// maps, BXW0052 an entry no member matches, BXW0053 a member two entries claim — which needs two
/// manifests, since BXW0029 refuses a repeated name or path inside one, and stays reachable because
/// two anchors can resolve onto one directory. A name-matching, path-disagreeing entry is *both* a
/// BXW0051 and a BXW0052, which is what makes [`maps`]' pair rule observable rather than assumed.
///
/// A member in a `fixtures` subtree is an ordinary member: D2's opacity hides a `boxology.toml` from
/// *discovery*, and every entry it declares with it, but removes nothing from `cargo metadata`. It
/// is therefore BXW0051 — not exempt, which would leave Cargo graph nodes with no role — until S7
/// D4's T4 migration takes fixture crates out of this workspace's membership, which is how that spec
/// resolves it. The owning platform package is *no* escape hatch: its kind hosts only `platform`, so
/// mapping a fixture box implementation declares a false role. A member at the workspace root, or at
/// a declaring package's own root, is likewise unmatchable: no `[[crates]].path` is empty or `.`.
///
/// The [`Mapped`] list is the other half of the answer: the association this function already
/// computes, returned instead of discarded, because the edge policy judges roles and a role exists
/// only where a member matched exactly one entry its package kind can host.
fn map<'a>(packages: &'a [Package], members: &'a [CargoMember]) -> (Vec<Mapped<'a>>, Vec<Entry>) {
    let mut declared: Vec<(&Package, &CrateEntry)> = Vec::new();
    for package in packages {
        declared.extend(package.manifest.crates().iter().map(|e| (package, e)));
    }
    let (mut roled, mut defects) = (Vec::new(), Vec::new());
    for member in members {
        // Walk order, not sorted order: `check` sorts packages by identity only after this runs,
        // so the outermost claimant leads, exactly as a candidate list does.
        let claiming = |(p, e): &&(&Package, &CrateEntry)| maps(p, e, member);
        let claims: Vec<_> = declared.iter().filter(claiming).collect();
        let rule = match claims.as_slice() {
            [claim] => {
                let (package, entry) = **claim;
                if hosts(package.manifest.kind(), entry.role()) {
                    roled.push(Mapped {
                        member,
                        package,
                        entry,
                    });
                }
                continue;
            }
            [] => UNMAPPED,
            _ => CLAIMED,
        };
        let named: Vec<String> = claims.iter().map(|(p, e)| spell(p, e)).collect();
        let at = member.manifest_path().clone();
        let found = Finding::about(rule, CRATE_SOURCE, at, None, named.join(","));
        defects.push(Entry::Workspace(found));
    }
    for (package, entry) in &declared {
        if members.iter().any(|member| maps(package, entry, member)) {
            continue;
        }
        let at = package.manifest_path.clone();
        let owner = Some(package.id().clone());
        let payload = String::from(entry.path().as_str());
        let found = Finding::about(UNMATCHED, D4_SOURCE, at, owner, payload);
        defects.push(Entry::Workspace(found));
    }
    (roled, defects)
}
/// One Cargo member, the declared entry that maps it, and the package declaring that entry: the
/// association [`map`] computes, retained for the edge policy alone. A member no entry maps
/// (BXW0051), one two entries claim (BXW0053), and one whose entry declares a role its package kind
/// cannot host (BXW0054) each yield **none** of these: every one of them is already a located
/// finding naming the document to change, and an edge verdict about it would rest on a role that
/// does not exist. The workspace-root member is the permanent case — no `[[crates]].path` spells it
/// — so the edge policy never assumes role coverage and is total over whatever subset mapped.
struct Mapped<'a> {
    member: &'a CargoMember,
    package: &'a Package,
    entry: &'a CrateEntry,
}
/// Reports whether a package of `kind` can host a crate of `role`.
///
/// **Ten** of the twelve cells are textually determined. "A native box owns a handwritten
/// implementation crate and a mechanically generated contract crate. Both compilation units belong
/// to the same logical box" gives `box-implementation` and `box-contract` to a box package and to no
/// other kind; "Conversely, an application composition is a separate logical owner even when it
/// compiles both box implementations into one binary" gives `composition` to a composition package
/// and denies a box or a platform package one. The remaining two — `box`/`platform` and
/// `composition`/`platform` — are **inferred**, from "Repository-wide ownership policy, CI, build
/// tooling ... belong to platform packages", a sentence about material rather than about crate
/// roles. S5 D4 licenses the whole table generically, so the relation is the identity between a
/// role's owning kind and the declaring kind, with `box` the one kind hosting two. The match is over
/// the closed vocabulary, so a role added later fails to compile here rather than defaulting to
/// possible; **how many** crates of a role a package hosts is a different sentence.
fn hosts(kind: Kind, role: CrateRole) -> bool {
    match role {
        CrateRole::BoxImplementation | CrateRole::BoxContract => kind == Kind::Box,
        CrateRole::Composition => kind == Kind::Composition,
        CrateRole::Platform => kind == Kind::Platform,
    }
}
/// Reports BXW0054 for every declared crate role its declaring package's kind cannot host, located
/// at that manifest and naming the entry by the directory it spells — unique within one manifest by
/// BXW0029, so the finding names the document line to change.
fn roles(packages: &[Package]) -> Vec<Entry> {
    let mut defects = Vec::new();
    for package in packages {
        let kind = package.manifest.kind();
        let impossible = |entry: &&CrateEntry| !hosts(kind, entry.role());
        for entry in package.manifest.crates().iter().filter(impossible) {
            let at = package.manifest_path.clone();
            let owner = Some(package.id().clone());
            let payload = String::from(entry.path().as_str());
            let found = Finding::about(ROLE, D4_SOURCE, at, owner, payload);
            defects.push(Entry::Workspace(found));
        }
    }
    defects
}
/// **Known v0 limit, recorded in the code and not only in the task spec.** A crate that reaches
/// another crate's source through `include!` declares no Cargo dependency, so that edge is absent
/// from the `cargo metadata` document and invisible to every rule in this section. Four instances
/// exist in this repository today — `crates/fixtures/hello/implementation/src/lib.rs`,
/// `crates/fixtures/ping/implementation/src/lib.rs`, `crates/boxology-http/src/binding.rs`, and
/// `crates/boxology-generator/src/lib.rs` — so a forbidden dependency concealed that way passes.
/// Closing it needs a source-level check, which is no part of reading one metadata document.
fn edges(roled: &[Mapped], members: &[CargoMember]) -> Vec<Entry> {
    let mut defects = Vec::new();
    for source in roled {
        for held in source.member.edges() {
            let kind = held.kind.word();
            let judged = match &held.target {
                EdgeTarget::Root => None,
                EdgeTarget::InRoot(at) => {
                    let occupies = |other: &&Mapped| other.member.crate_dir() == Some(at);
                    if let Some(target) = roled.iter().find(occupies) {
                        let same = source.package.id() == target.package.id();
                        let declared = source
                            .package
                            .manifest()
                            .imports()
                            .iter()
                            .any(|import| import.package() == target.package.id());
                        let selected = source
                            .package
                            .manifest()
                            .composition()
                            .is_some_and(|c| c.boxes().contains(target.package.id()));
                        judged(
                            source.entry.role(),
                            target.entry.role(),
                            same,
                            declared,
                            selected,
                        )
                        .map(|rule| {
                            let id = target.package.id();
                            (rule, format!("{id} {} {kind}", at.as_str()))
                        })
                    } else if members.iter().any(|member| member.crate_dir() == Some(at))
                        || source.entry.role() == CrateRole::Platform
                    {
                        None
                    } else {
                        Some((NON_MEMBER, format!("{} {kind}", at.as_str())))
                    }
                }
                EdgeTarget::OutOfRoot if source.entry.role() == CrateRole::Platform => None,
                EdgeTarget::OutOfRoot => Some((NON_MEMBER, format!("outside {kind}"))),
            };
            let Some(((rule, stated), payload)) = judged else {
                continue;
            };
            let owner = Some(source.package.id().clone());
            let located = source.member.manifest_path().clone();
            defects.push(Entry::Workspace(Finding::about(
                rule, stated, located, owner, payload,
            )));
        }
    }
    defects
}
/// Reports the rule an edge from a `source`-role crate onto a `target`-role crate breaks, and `None`
/// for one it permits.
///
/// Authority is explicit: 08 supplies BXW0055–BXW0057 where stated. BXW0058 binds through S5 D4:
/// 08 supplies composition→implementation, while #325's unlisted/S5-D4 ruling supplies
/// composition→contract. S5 D4 also fail-closes unlisted role/scope cells as BXW0059. Both C→C
/// rulings bind through S5 D4: sibling is fail-closed; declared foreign is allowed. 08's declared-
/// contract/acyclicity paragraphs support the foreign allowance. Tracker #325 allows every role onto
/// platform; platform as source remains fail-closed onto I/C/X. BXW0060 is outside this role grid
/// and binds through S5 D4; S7 D4/#325 is the supporting adoption inference.
fn judged(
    source: CrateRole,
    target: CrateRole,
    same: bool,
    declared: bool,
    selected: bool,
) -> Option<EdgeRule> {
    use CrateRole::{BoxContract as C, BoxImplementation as I, Composition as X, Platform as P};
    match (source, target) {
        // Allowed on the `#325` silence-1 resolution alone — 08 spells no such row.
        (_, P) => None,
        (I, C) if same || declared => None,
        (C, C) if !same && declared => None,
        (I, C) => Some(DECLARED),
        (C, C) if !same => Some(DECLARED),
        (X, I) | (X, C) if selected => None,
        (X, I) | (X, C) => Some(SELECTED),
        (C, I) => Some(CONTRACT),
        (I, I) if !same => Some(FOREIGN),
        (I, I) | (C, C) | (C, X) | (I, X) => Some(IMPOSSIBLE),
        (X, X) | (P, I) | (P, C) | (P, X) => Some(IMPOSSIBLE),
    }
}
/// Reads the workspace members of a `cargo metadata` document. `None` is BXW0050: the whole
/// document is a defect, never a partial reading, so every `None` below — malformed JSON, a missing
/// or mistyped field, or a member path [`member`] cannot normalize — is that one coded answer and
/// not a discarded failure.
///
/// Exactly three top-level names are read, and exactly four of each *member* package: D4's
/// declaration-based reading is what makes purity over one document sound, and `resolve` — null
/// under the `--no-deps` invocation T5 owns, so consulting it is impossible rather than merely
/// forbidden — is never consulted. A `packages[]` element whose id no `workspace_members` entry
/// spells is a dependency of the workspace, not a member of it: it is skipped before its manifest
/// path is normalized and before its `dependencies` are read, so a registry package living outside
/// the workspace root, or one an older document spells without that field, is no defect. Ids are
/// matched as opaque strings, never parsed.
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
            let edges = declared(root, package)?;
            members.push(member(root, at, name, edges)?);
        }
    }
    Some(members)
}
/// The position a declared dependency takes in the manifest that declares it: the three values
/// `dependencies[].kind` may hold, each carrying the word this crate — never the document — spells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EdgeKind {
    Normal,
    Build,
    Dev,
}
impl EdgeKind {
    /// Returns the `&'static str` this crate chose for the kind. A rendered report echoes it
    /// safely because it is this crate's own literal and no byte of the document.
    fn word(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Build => "build",
            Self::Dev => "dev",
        }
    }
}
/// Where one declared path dependency points, in this crate's own terms.
#[derive(Clone, Debug, Eq, PartialEq)]
enum EdgeTarget {
    /// A directory strictly under `workspace_root`, re-validated as a workspace-relative path.
    InRoot(RelativePath),
    /// The workspace root's own directory: a Cargo member may occupy it, and `path = ".."` onto
    /// that member is legal, so the document spells it. Kept distinct from
    /// [`EdgeTarget::OutOfRoot`] because it is *inside* the workspace — no [`RelativePath`]
    /// spells it, which is a property of that grammar and not of the workspace, and
    /// [`CargoMember::crate_dir`] names the same directory `None` for exactly that reason.
    /// Collapsing the two would let a later slice report an in-workspace member as lying outside
    /// the workspace, and a unit variant destroys the distinction at read time beyond recovery.
    Root,
    /// A directory the workspace root does not contain: a sibling root whose name this one is a
    /// prefix of, or any absolute path elsewhere. A unit variant on purpose — this is the one
    /// value here no grammar has proved, so the type makes retaining any byte of it impossible.
    OutOfRoot,
}
/// One declared dependency edge of a Cargo workspace member: how it is declared, and where it
/// points. A registry dependency is not one of these — see [`declared`].
#[derive(Clone, Debug, Eq, PartialEq)]
struct DeclaredEdge {
    kind: EdgeKind,
    target: EdgeTarget,
}
/// Reads one member package's declared dependency edges. `None` is BXW0050 like every other
/// reading failure: `dependencies` is **required** — cargo emits it, empty or not — so its absence
/// or a mistyped element is a defect of the whole document and never a silently empty edge list.
/// An unknown or mistyped `kind`, a future kind included, is that same coded answer: fail-closed,
/// because an unread edge is an unenforced rule.
///
/// **Exactly two things of each element are read, and that is the load-bearing decision here.**
/// `name`, `rename`, `optional`, `features`, `target`, and `source` are deliberately never
/// consulted: an entry is an edge whatever those attributes say, and a target's identity is its
/// normalized path, which is injective over members because one directory holds one Cargo package.
/// Renamed, optional, feature-activated, and target-specific dependencies are therefore covered
/// *by construction* rather than by enumerating them — and it is why no unvalidated byte of the
/// document is consulted at all, let alone echoed. Reading declarations rather than the resolved
/// graph is the same decision seen from the other side: an optional dependency whose feature is
/// unactivated is absent from `resolve` and present here.
///
/// A `path` key absent is a registry dependency: no crate role, allowed by every rule, so it is
/// stored nowhere and its unvalidated `name` and `source` are never looked at. An element that is
/// no object at all spells no `kind` either, so it is the same rejection and needs no second one.
fn declared(root: &str, package: &Value) -> Option<Vec<DeclaredEdge>> {
    let mut edges = Vec::new();
    for entry in package.get("dependencies")?.as_array()? {
        let kind = match entry.get("kind")? {
            Value::Null => EdgeKind::Normal,
            Value::String(word) if word == "build" => EdgeKind::Build,
            Value::String(word) if word == "dev" => EdgeKind::Dev,
            _ => return None,
        };
        let Some(at) = entry.get("path") else {
            continue;
        };
        let target = points(root, at.as_str()?)?;
        edges.push(DeclaredEdge { kind, target });
    }
    Some(edges)
}
/// Normalizes one declared dependency's absolute `path` — the target's *directory* — against the
/// absolute `workspace_root`, exactly as [`member`] normalizes a member's manifest path.
///
/// The root must be followed by a separator, so a sibling root whose name this one is a prefix of
/// is out of root rather than a bogus in-root remainder. The root spelled *exactly* is neither: a
/// root package is an ordinary Cargo member, a `path = ".."` dependency onto it is legal and cargo
/// emits that path as the root verbatim, so reading it as out of root would both reject nothing
/// and let a later slice report an in-workspace member as lying outside the workspace. A remainder
/// the [`RelativePath`] grammar refuses — a `..` segment, a backslash, a control byte — is `None`,
/// which is BXW0050: the same posture [`member`] takes on a path it cannot re-validate, because
/// this crate reports no path whose grammar it has not proved.
fn points(root: &str, path: &str) -> Option<EdgeTarget> {
    let Some(rest) = path.strip_prefix(root) else {
        return Some(EdgeTarget::OutOfRoot);
    };
    match rest.strip_prefix('/') {
        None if rest.is_empty() => Some(EdgeTarget::Root),
        None => Some(EdgeTarget::OutOfRoot),
        Some(under) => Some(EdgeTarget::InRoot(RelativePath::new(under).ok()?)),
    }
}
/// Normalizes one member's absolute `manifest_path` against the absolute `workspace_root`.
///
/// The root must be followed by a separator, so a sibling root whose name this one is a prefix of
/// never normalizes, and the remainder must be exactly `Cargo.toml` or end in `/Cargo.toml`, whose
/// head is the crate directory — absent at the workspace root, which no [`RelativePath`] can spell.
/// Separators are `/` only: a drive-prefixed or backslash-separated document is BXW0050 rather than
/// a second path dialect, because [`RelativePath`] admits neither and this crate reports no path it
/// has not re-validated.
fn member(
    root: &str,
    manifest_path: &str,
    name: &str,
    edges: Vec<DeclaredEdge>,
) -> Option<CargoMember> {
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
        edges,
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
/// file has, and every Cargo workspace member the metadata document proved readable. Only
/// [`WorkspaceInputs::check`] builds one, so D3's "every tracked file classifies exactly once" — and
/// 02-packages' "every Cargo package matches exactly one `[[crates]]` entry", whose role its
/// declaring kind can host — are properties of this type's existence, not checks a consumer repeats.
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
/// terms: the crate's directory, its `Cargo.toml` re-validated as a workspace-relative path, its
/// Cargo package name, and every dependency edge it declares. Only [`WorkspaceInputs::check`]
/// builds one, so a value of this type is the proof the document was readable — and the absolute
/// paths it carries reach nothing else.
///
/// This is the left-hand side of 02-packages' rule that every Cargo package match exactly one
/// manifest `[[crates]]` entry by normalized manifest path and Cargo package name; a [`Workspace`]
/// is the proof every member made exactly one such match.
#[derive(Debug, Eq, PartialEq)]
pub struct CargoMember {
    manifest_path: RelativePath,
    directory: Option<RelativePath>,
    name: String,
    edges: Vec<DeclaredEdge>,
}
impl CargoMember {
    /// Returns every dependency edge this member declares, in declaration order: the left-hand
    /// side of the edge policy. Crate-internal, because a [`DeclaredEdge`] holds the document's
    /// own shape and nothing outside this crate judges it. Registry dependencies are absent —
    /// they are no edge at all — and an edge appears once per declared entry, so one target
    /// reached twice under two `target` cfgs is two edges, as the document declares it.
    fn edges(&self) -> &[DeclaredEdge] {
        &self.edges
    }
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
/// The canonical repository-owned validation workflow data from S5 D7.
///
/// S5 D7 owns these bytes and S6 D4 consumes them for generated projects while owning their
/// placement. `fetch-depth: 0` guarantees that the pull request's base revision is available to
/// `boxology check --base`. The accepted v0 provisioning deduction is that the runner provides
/// `boxology` on `PATH` and a platform source checkout at the path recorded in the workspace's
/// manifests; the first published release replaces that precondition with versioned installation.
/// AC10's fixture execution remains deferred to the S6 placement and acceptance work.
pub const CHECK_WORKFLOW: &str = r#"# The repository-owned Boxology validation workflow (S5 D7).
#
# This document is Boxology platform data: its content is owned by the
# platform (specs/s5-manifest-and-validation.md D7) and written verbatim
# into generated projects by the installer, which owns only its placement
# (specs/s6-installer-and-generated-project.md D4). It runs the same
# `boxology check` used by local development; there is no hidden CI-only
# validation layer (boxology-details/08-rust-build-topology.md).
#
# V0 precondition: the Boxology platform is consumed from a source checkout
# and nothing is published, so the runner must provide the `boxology`
# binary on PATH and the platform source checkout at the path recorded in
# this workspace's manifests. The first published release replaces this
# precondition with a versioned installation step.

name: check

on:
  pull_request:

permissions:
  contents: read

jobs:
  check:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      # fetch-depth: 0 guarantees the pull request's base revision is
      # locally available to `boxology check --base` (S5 D7).
      - uses: actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0 # v7.0.0
        with:
          fetch-depth: 0
          persist-credentials: false
      - name: Install pinned toolchain
        run: rustup toolchain install
      - name: boxology check
        env:
          BASE_SHA: ${{ github.event.pull_request.base.sha }}
        run: boxology check --base "$BASE_SHA"
"#;
#[cfg(test)]
mod tests {
    use super::*;
    use boxology_manifest::{Kind, LineColumn, Span};
    const OPAQUE: &[u8] = b"schema = 9\nnot toml";
    /// A `cargo metadata` document naming no workspace member: what a listing under test that is
    /// not about crate-role mapping supplies, so it reports only what it is about.
    const EMPTY: &str = r#"{"workspace_root":"/w","workspace_members":[],"packages":[]}"#;
    const EXPECTED_CHECK_WORKFLOW: &str = r#"# The repository-owned Boxology validation workflow (S5 D7).
#
# This document is Boxology platform data: its content is owned by the
# platform (specs/s5-manifest-and-validation.md D7) and written verbatim
# into generated projects by the installer, which owns only its placement
# (specs/s6-installer-and-generated-project.md D4). It runs the same
# `boxology check` used by local development; there is no hidden CI-only
# validation layer (boxology-details/08-rust-build-topology.md).
#
# V0 precondition: the Boxology platform is consumed from a source checkout
# and nothing is published, so the runner must provide the `boxology`
# binary on PATH and the platform source checkout at the path recorded in
# this workspace's manifests. The first published release replaces this
# precondition with a versioned installation step.

name: check

on:
  pull_request:

permissions:
  contents: read

jobs:
  check:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      # fetch-depth: 0 guarantees the pull request's base revision is
      # locally available to `boxology check --base` (S5 D7).
      - uses: actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0 # v7.0.0
        with:
          fetch-depth: 0
          persist-credentials: false
      - name: Install pinned toolchain
        run: rustup toolchain install
      - name: boxology check
        env:
          BASE_SHA: ${{ github.event.pull_request.base.sha }}
        run: boxology check --base "$BASE_SHA"
"#;
    #[test]
    fn check_workflow_matches_the_independent_golden() {
        assert_eq!(
            CHECK_WORKFLOW.as_bytes(),
            EXPECTED_CHECK_WORKFLOW.as_bytes()
        );
    }
    #[test]
    fn check_workflow_has_one_of_each_required_anchor() {
        const ANCHORS: &[&str] = &[
            "        run: boxology check --base \"$BASE_SHA\"",
            "          BASE_SHA: ${{ github.event.pull_request.base.sha }}",
            "          fetch-depth: 0",
            "          persist-credentials: false",
            "    runs-on: ubuntu-latest",
            "on:\n  pull_request:",
            "permissions:\n  contents: read",
            "        run: rustup toolchain install",
        ];
        for workflow in [CHECK_WORKFLOW, EXPECTED_CHECK_WORKFLOW] {
            for anchor in ANCHORS {
                assert_eq!(
                    workflow.match_indices(anchor).count(),
                    1,
                    "anchor {anchor:?} must occur exactly once"
                );
            }
            let uses: Vec<&str> = workflow
                .lines()
                .filter(|line| line.trim_start().starts_with("- uses:"))
                .collect();
            assert_eq!(uses.len(), 1);
            assert_eq!(
                uses[0].trim_start(),
                "- uses: actions/checkout@9c091bb21b7c1c1d1991bb908d89e4e9dddfe3e0 # v7.0.0"
            );
        }
    }
    #[test]
    fn check_workflow_has_exactly_one_trailing_newline() {
        for workflow in [CHECK_WORKFLOW, EXPECTED_CHECK_WORKFLOW] {
            assert!(workflow.ends_with('\n'));
            assert!(!workflow.ends_with("\n\n"));
        }
    }
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
    /// A manifest with `[[crates]]` entries appended: a name, its directory, and the role declared.
    fn crates(base: Vec<u8>, entries: &[(&str, &str, &str)]) -> Vec<u8> {
        let mut text = String::from_utf8(base).expect("test manifests are ASCII");
        for (name, at, role) in entries {
            let head = format!("[[crates]]\ncargo_package = {name:?}\n");
            text.push_str(&head);
            text.push_str(&format!("path = {at:?}\nrole = {role:?}\n"));
        }
        text.into_bytes()
    }
    /// A package manifest of `kind` owning its own manifest and declaring `entries`, plus the
    /// `[composition]` section BXW0022 requires of a composition-kind package.
    fn roled(id: &str, kind: &str, entries: &[(&str, &str, &str)]) -> Vec<u8> {
        let mut base = owning(id, kind, &[MANIFEST], &[]);
        if kind == "composition" {
            base.extend_from_slice(b"[composition]\nboxes = [\"hello\"]\n");
        }
        crates(base, entries)
    }
    fn importing(id: &str, entries: &[(&str, &str, &str)], imports: &[&str]) -> Vec<u8> {
        let mut text = String::from_utf8(roled(id, "box", entries)).expect("ASCII");
        for package in imports {
            text.push_str(&format!(
                "[[imports]]\npackage = {package:?}\ncontract = {package:?}\n"
            ));
        }
        text.into_bytes()
    }
    fn selecting(
        id: &str,
        entries: &[(&str, &str, &str)],
        boxes: &[&str],
        binding: bool,
    ) -> Vec<u8> {
        let named: Vec<String> = boxes.iter().map(|box_id| format!("{box_id:?}")).collect();
        let mut base = owning(id, "composition", &[MANIFEST], &[]);
        base.extend_from_slice(
            format!("[composition]\nboxes = [{}]\n", named.join(", ")).as_bytes(),
        );
        if binding {
            let first = boxes.first().expect("a binding needs one box");
            base.extend_from_slice(
                format!(
                    "[[composition.bindings]]\nbox = {first:?}\n\
                 capability = \"{first}.run\"\ntransport = \"in-process\"\n"
                )
                .as_bytes(),
            );
        }
        crates(base, entries)
    }
    fn successful_edge(
        checked: &Workspace,
        packages: &[&str],
        source: &str,
        source_at: &str,
        target: &str,
        target_at: &str,
    ) {
        let ids: Vec<&str> = checked
            .packages()
            .iter()
            .map(|package| package.id().as_str())
            .collect();
        assert_eq!(ids, packages);
        let source = checked
            .cargo_members()
            .iter()
            .find(|member| member.cargo_package() == source)
            .unwrap();
        let target = checked
            .cargo_members()
            .iter()
            .find(|member| member.cargo_package() == target)
            .unwrap();
        assert_eq!(source.manifest_path(), &path(source_at));
        assert_eq!(target.crate_dir(), Some(&path(target_at)));
        assert_eq!(
            source.edges(),
            &[DeclaredEdge {
                kind: EdgeKind::Normal,
                target: EdgeTarget::InRoot(path(target_at)),
            }]
        );
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
        let mut listed = Vec::new();
        for (at, name) in members {
            listed.push((*at, *name, ""));
        }
        depending(&listed, strangers)
    }
    /// [`metadata`], each member also carrying the raw `dependencies[]` element list it declares —
    /// which every member must spell, empty or not. Strangers stay raw, and none of them spells
    /// the field at all: that is what makes the skip order observable.
    fn depending(members: &[(&str, &str, &str)], strangers: &[&str]) -> String {
        let (mut ids, mut packages) = (Vec::new(), Vec::from(strangers).join(","));
        for (at, name, edges) in members {
            let head = if at.is_empty() {
                String::new()
            } else {
                format!("{at}/")
            };
            let id = format!("path+file:///w/{head}#0.0.0");
            let one = format!("\"name\":{name:?},\"manifest_path\":\"/w/{head}Cargo.toml\"");
            let held = format!("\"dependencies\":[{edges}]");
            packages.push_str(&format!(",{{\"id\":{id:?},{one},{held}}}"));
            ids.push(format!("{id:?}"));
        }
        let listed = ids.join(",");
        let body = packages.trim_start_matches(',');
        format!(r#"{{"workspace_root":"/w","workspace_members":[{listed}],"packages":[{body}]}}"#)
    }
    /// One `dependencies[]` element: its `kind` value verbatim, the absolute `path` a path
    /// dependency carries, and `extra` raw attributes the reader must never consult.
    fn edge(kind: &str, path: Option<&str>, extra: &str) -> String {
        let at = path.map(|at| format!(",\"path\":{at:?}"));
        let more = if extra.is_empty() {
            String::new()
        } else {
            format!(",{extra}")
        };
        let head = at.unwrap_or_default();
        format!("{{\"kind\":{kind}{head}{more}}}")
    }
    /// The one diagnostic `OPAQUE` bytes provoke, as a report entry located at `at`.
    fn rejected(at: &str) -> Entry {
        let held = Manifest::parse(path(at), OPAQUE);
        let mut errors = held.expect_err("opaque bytes are no manifest").into_vec();
        Entry::Manifest(errors.pop().expect("the schema gate reports one"))
    }
    /// Every code this crate *authors*, ascending. A report may also carry BXW0001-BXW0041 through
    /// [`Entry::Manifest`], which is `boxology_manifest`'s own rendering of its own rule table and
    /// is pinned by that crate's own golden; this list is what this crate is accountable for. The
    /// corpus and the golden below are both driven from it, so a code that registers nowhere fails
    /// loudly instead of going unproven.
    const ALL_CODES: &[&str] = &[
        "BXW0042", "BXW0043", "BXW0044", "BXW0045", "BXW0046", "BXW0047", "BXW0048", "BXW0049",
        "BXW0050", "BXW0051", "BXW0052", "BXW0053", "BXW0054", "BXW0055", "BXW0056", "BXW0057",
        "BXW0058", "BXW0059", "BXW0060",
    ];
    /// One minimal workspace per code, ordered as `ALL_CODES` is: each is a shape the suite above
    /// already exercises, reduced to the least input that provokes its code, so the golden below
    /// reads every rule off a finding a real check produced rather than off a constant table.
    fn corpus() -> Vec<(&'static str, WorkspaceInputs)> {
        let twin = || document("twin", "box", &[]);
        let platform = |owned: &[&str]| owning("root", "platform", owned, &[]);
        let solo = |bytes| vec![(MANIFEST, bytes)];
        let twice: [(&str, &[&str]); 2] = [("one", &["g.rs"]), ("two", &["g.rs"])];
        let once: [(&str, &[&str]); 1] = [("gen", &["g.rs"])];
        let nested = owning("p", "box", &[MANIFEST], &[]);
        let escaping = FileEntry::symlink(path("link"), String::from("/etc"));
        // One member at `c/`, and the entries mapping it rightly, unmatchably, and impossibly.
        let one_member = metadata(&[("c", "c")], &[]);
        let one = |bytes, document: &str| mapped(solo(bytes), &[], document);
        let mapping = |at: &str| crates(platform(&[MANIFEST]), &[("c", at, "platform")]);
        let inner: [(&str, &str, &str); 1] = [("c", "c", "box-contract")];
        let boxed = crates(platform(&[MANIFEST]), &inner);
        // Two manifests resolving one entry each onto the same member: what BXW0053 needs.
        let deep = crates(owning("deep", "box", &[MANIFEST], &[]), &inner);
        let twins = vec![(MANIFEST, mapping("sub/c")), ("sub/boxology.toml", deep)];
        // Two crates of one box package, the first declaring a normal edge onto the second: only
        // the source role separates the contract-to-implementation row from the same-package pair
        // no role can permit. Both members map, so the edge finding is the whole report.
        let near = edge("null", Some("/w/t"), "");
        let sibling = |role: &str| {
            let held: [(&str, &str, &str); 2] =
                [("s", "s", role), ("t", "t", "box-implementation")];
            let listed = [("s", "s", near.as_str()), ("t", "t", "")];
            let two = solo(roled("solo", "box", &held));
            mapped(two, &[], &depending(&listed, &[]))
        };
        // Two box packages, the first's implementation reaching the second's: foreign by identity.
        let far = edge("\"dev\"", Some("/w/b/t"), "");
        let sole = |name: &'static str| [(name, name, "box-implementation")];
        let foreign = vec![
            ("a/boxology.toml", roled("a", "box", &sole("s"))),
            ("b/boxology.toml", roled("b", "box", &sole("t"))),
        ];
        let across = depending(&[("a/s", "s", far.as_str()), ("b/t", "t", "")], &[]);
        let contract: [(&str, &str, &str); 1] = [("t", "t", "box-contract")];
        let undeclared = vec![
            ("a/boxology.toml", roled("a", "box", &sole("s"))),
            ("b/boxology.toml", roled("b", "box", &contract)),
        ];
        let to_contract = depending(&[("a/s", "s", far.as_str()), ("b/t", "t", "")], &[]);
        let composition: [(&str, &str, &str); 1] = [("s", "s", "composition")];
        let unselected = vec![
            ("a/boxology.toml", roled("a", "composition", &composition)),
            ("b/boxology.toml", roled("b", "box", &sole("t"))),
        ];
        let non_member = vec![("a/boxology.toml", roled("a", "box", &sole("s")))];
        let missing = edge("null", Some("/w/missing"), "");
        vec![
            (
                "BXW0042",
                workspace(vec![
                    ("a/boxology.toml", twin()),
                    ("b/boxology.toml", twin()),
                ]),
            ),
            (
                "BXW0043",
                workspace(solo(document("root", "platform", &[MANIFEST]))),
            ),
            (
                "BXW0044",
                listing(solo(platform(&[MANIFEST])), &["orphan.rs"]),
            ),
            (
                "BXW0045",
                workspace(vec![
                    (MANIFEST, platform(&["**"])),
                    ("p/boxology.toml", nested),
                ]),
            ),
            (
                "BXW0046",
                listing(solo(deriving(platform(&[MANIFEST]), &twice)), &["g.rs"]),
            ),
            (
                "BXW0047",
                listing(
                    solo(deriving(platform(&[MANIFEST, "g.rs"]), &once)),
                    &["g.rs"],
                ),
            ),
            ("BXW0048", inputs(vec![escaping])),
            (
                "BXW0049",
                listing(solo(platform(&[MANIFEST, LOCKFILE])), &[LOCKFILE]),
            ),
            (
                "BXW0050",
                mapped(solo(platform(&[MANIFEST])), &[], "not json"),
            ),
            ("BXW0051", one(platform(&[MANIFEST]), &one_member)),
            ("BXW0052", one(mapping("c"), EMPTY)),
            (
                "BXW0053",
                mapped(twins, &[], &metadata(&[("sub/c", "c")], &[])),
            ),
            ("BXW0054", one(boxed, &one_member)),
            ("BXW0055", sibling("box-contract")),
            ("BXW0056", mapped(foreign, &[], &across)),
            ("BXW0057", mapped(undeclared, &[], &to_contract)),
            ("BXW0058", mapped(unselected, &[], &across)),
            ("BXW0059", sibling("box-implementation")),
            (
                "BXW0060",
                mapped(
                    non_member,
                    &[],
                    &depending(&[("a/s", "s", missing.as_str())], &[]),
                ),
            ),
        ]
    }
    const EXPECTED: &str = "\
BXW0042 one package identity must be declared by exactly one manifest boxology-details/02-packages.md discovery walk
BXW0043 a fixtures pattern must not claim its own declaring manifest boxology-details/02-packages.md discovery walk
BXW0044 every tracked file must classify under some package boxology-details/02-packages.md discovery walk
BXW0045 at most one package may claim a non-derived path boxology-details/02-packages.md discovery walk
BXW0046 at most one declared derived output may claim a path boxology-details/02-packages.md discovery walk
BXW0047 a declared derived output must not also be claimed as a non-derived path boxology-details/02-packages.md discovery walk
BXW0048 symlink targets must stay inside the workspace root boxology-details/02-packages.md discovery walk
BXW0049 Cargo.lock must be a platform package's declared global derived artifact boxology-details/02-packages.md discovery walk
BXW0050 cargo metadata must be a readable workspace document boxology-details/02-packages.md crate roles
BXW0051 every Cargo workspace member must match one declared crate entry boxology-details/02-packages.md crate roles
BXW0052 every declared crate entry must match one Cargo workspace member specs/s5-manifest-and-validation.md D4
BXW0053 at most one declared crate entry may match a Cargo workspace member boxology-details/02-packages.md crate roles
BXW0054 a declared crate role must be one its package kind can host specs/s5-manifest-and-validation.md D4
BXW0055 a box contract crate must depend on no box implementation boxology-details/08-rust-build-topology.md edge table
BXW0056 a box implementation must depend on no foreign box implementation boxology-details/08-rust-build-topology.md edge table
BXW0057 a box crate's edge to a foreign contract must be a declared import boxology-details/08-rust-build-topology.md edge table
BXW0058 a composition edge must target a selected box specs/s5-manifest-and-validation.md D4
BXW0059 no rule permits an edge between these crate roles at this package scope specs/s5-manifest-and-validation.md D4
BXW0060 a path dependency onto a non-member is allowed only from a platform crate specs/s5-manifest-and-validation.md D4
";
    #[test]
    fn rule_text_and_sources_are_locked() {
        // What each code actually *reports*, not what a table spells: this proves the rule its
        // constant carries is reached by a real finding, which reading the constants cannot.
        let mut rendered = String::new();
        for (code, inputs) in corpus() {
            let Err(report) = inputs.check() else {
                panic!("{code} accepted its own corpus input");
            };
            // *Every* entry must carry the code, not merely one of them: finding one would pass a
            // corpus input that provokes its code plus unrelated ones, leaving "the least input
            // that provokes it" asserted nowhere. It is not "exactly one" either — BXW0042 reports
            // once per carrier of the duplicated identity, and both entries are that same code.
            let mut first = None;
            for entry in &report {
                let Entry::Workspace(carried) = entry else {
                    panic!("{code} reported {report}");
                };
                assert_eq!(carried.code(), code, "{code} reported {report}");
                first = first.or(Some(carried));
            }
            let found = first.expect("a rejection carries at least one entry");
            let line = format!("{code} {} {}\n", found.rule(), found.rule_source());
            rendered.push_str(&line);
        }
        assert_eq!(rendered, EXPECTED);
    }
    #[test]
    fn corpus_covers_every_code() {
        // Comparing the two ordered lists proves both directions at once: no code without a
        // workspace that provokes it, and no workspace for a code this crate does not emit.
        let covered: Vec<&str> = corpus().iter().map(|(code, _)| *code).collect();
        assert_eq!(covered, ALL_CODES);
        assert!(ALL_CODES.windows(2).all(|pair| pair[0] < pair[1]));
    }
    #[test]
    fn all_codes_is_exhaustive() {
        // The rule table's own source text, read at compile time: a code emitted anywhere in the
        // crate but registered nowhere above fails here rather than drifting in unproven. The test
        // module is cut off so this module's own probe literals do not count as emissions.
        //
        // Both halves of that cut are pinned, because either one narrows the scan *silently* and
        // silence here reads exactly like success. Taking the text before the first
        // `#[cfg(test)]` is only the production half if that occurrence is the test module itself:
        // a `#[cfg(test)] use`, a test-only helper module, or a `#[cfg(test)] impl` above it would
        // truncate the scan there and hide every code below. And one file is only the whole crate
        // while the crate is one file: the next slice's BXW0051-BXW0054 are a natural candidate
        // for a second source file, which `include_str!` would not reach.
        let whole = include_str!("lib.rs");
        let (source, rest) = whole
            .split_once("#[cfg(test)]")
            .expect("the test module marker");
        assert!(
            rest.starts_with("\nmod tests"),
            "the cut missed the test module"
        );
        assert!(
            !source.contains("mod "),
            "a second source file is unscanned"
        );
        let mut seen: Vec<&str> = Vec::new();
        for (at, _) in source.match_indices("\"BXW") {
            let code = &source[at + 1..at + 8];
            if !seen.contains(&code) {
                seen.push(code);
            }
        }
        seen.sort_unstable();
        assert_eq!(seen, ALL_CODES);
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
        // The reading alone: this manifest maps none of the four, which is BXW0051's business
        // below, so the members are read off `members` rather than off an accepted `Workspace`.
        let (read, defects) = mapped(held, &[], &document).members();
        assert!(defects.is_empty(), "the document is readable");
        let seen: Vec<(&str, &str, &str)> = read
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
                r#""id":"i","name":"solo-crate","manifest_path":{at:?},"dependencies":[]"#
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
                   "name":"n","manifest_path":"/w/a/Cargo.toml","dependencies":[]}]}"#,
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
            let report = mapped(held, &[], &document)
                .check()
                .expect_err("this manifest maps no member");
            let [Entry::Workspace(found)] = report.as_slice() else {
                panic!("{case:?} reported {report}");
            };
            if readable.is_some() {
                // `/w/crate/Cargo.toml` and `/w/Cargo.toml` normalize; nothing else here does, and
                // reading them is what leaves one *member* — unmapped, which is BXW0051, not this.
                assert_eq!(found.code(), "BXW0051", "{case:?}");
                continue;
            }
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
    /// The other half of the reading: every member carries the edges it *declares*, normalized
    /// under `workspace_root`. The fixture is built so a wrong answer is reachable — the first
    /// entry names its own source member and aliases a *third* member, so a `name`- or
    /// `rename`-matcher attributes it to the wrong target; `crates/foo` is a strict prefix of
    /// member `crates/foo-bar`, and `/w2` a strict prefix collision on the root itself, so a
    /// separator-free normalizer either misattributes or invents `2/crates/foo`; one target sits
    /// two directories deep inside another member's; the same target is declared twice under two
    /// `target` cfgs, so a per-target dedupe loses one; a registry entry carries no `path` and must
    /// vanish; a fifth member occupies the workspace root itself and is both the source of an edge
    /// and the target of one, so reading the bare root as *out of* root would call an in-workspace
    /// member outside the workspace; and two members declare nothing at all, which must be an
    /// empty list and no defect.
    #[test]
    fn declared_edges_are_read_and_normalized() {
        // Attributes the reader must never consult, spelled to make consulting them observable.
        let lying = "\"name\":\"foo-bar\",\"rename\":\"deep\",\"optional\":true,\
                     \"features\":[\"extra\"]";
        let registry = "\"name\":\"serde\",\"source\":\"registry+https://example\"";
        // The two `cfg` variants of one dependency sit *adjacent* and render identically, which is
        // the order cargo emits — it sorts within each dependency table — so a `dedup` anywhere
        // silently halves them. Separated by unrelated entries, that mutation is inert and the
        // green means nothing.
        let listed = [
            edge("null", Some("/w/crates/foo"), lying),
            edge("null", Some("/w/crates/foo"), "\"target\":\"cfg(windows)\""),
            edge("\"build\"", Some("/w/crates/foo/deep"), ""),
            edge("\"dev\"", Some("/w/tools"), "\"optional\":false"),
            edge("null", None, registry),
            edge("null", Some("/w2/crates/foo"), ""),
            edge("\"dev\"", Some("/elsewhere/x"), ""),
            edge("null", Some("/w"), ""),
        ];
        let many = listed.join(",");
        let solo = edge("\"dev\"", Some("/w/crates/foo-bar"), "");
        let members = [
            ("crates/foo-bar", "foo-bar", many.as_str()),
            ("crates/foo", "foo", solo.as_str()),
            ("crates/foo/deep", "deep", ""),
            ("tools", "tools", ""),
            ("", "whole-workspace", solo.as_str()),
        ];
        let held = vec![(MANIFEST, owning("root", "platform", &[MANIFEST], &[]))];
        let document = depending(&members, &[]);
        // The reading alone, as above: this manifest maps none of the four members.
        let (read, defects) = mapped(held, &[], &document).members();
        assert!(defects.is_empty(), "the document is readable");
        let spelled = |member: &CargoMember| {
            let one = |held: &DeclaredEdge| {
                let at = match &held.target {
                    EdgeTarget::InRoot(dir) => dir.as_str(),
                    EdgeTarget::Root => "root",
                    EdgeTarget::OutOfRoot => "outside",
                };
                format!("{} {at}", held.kind.word())
            };
            let each: Vec<String> = member.edges().iter().map(one).collect();
            format!("{} [{}]", member.cargo_package(), each.join(","))
        };
        let seen: Vec<String> = read.iter().map(spelled).collect();
        assert_eq!(
            seen,
            [
                "deep []",
                "foo [dev crates/foo-bar]",
                "foo-bar [normal crates/foo,normal crates/foo,build crates/foo/deep,\
                 dev tools,normal outside,dev outside,normal root]",
                "tools []",
                "whole-workspace [dev crates/foo-bar]",
            ]
        );
    }
    /// BXW0050 codes every defect of a member's `dependencies[]` too, and every case below is the
    /// *same* rendered line: a reading defect is one coded answer whatever spelled it, and no byte
    /// of the document — least of all a dependency's absolute path — reaches the report. The field
    /// is required of every member, and an unknown `kind` is fail-closed, because a dependency the
    /// reader silently drops is an edge the policy silently permits. The stranger beside the member
    /// spells the field not at all, so every readable case here is also the proof that a
    /// `packages[]` element no `workspace_members` entry names is skipped *before* it is read.
    #[test]
    fn unreadable_dependency_declarations_are_coded() {
        let out = r#"{"id":"s","name":"vendor","manifest_path":"/vendor/Cargo.toml"}"#;
        let whole = |field: Option<&str>| {
            let head = r#""id":"i","name":"solo-crate","manifest_path":"/w/c/Cargo.toml""#;
            let held = field.map(|value| format!(",\"dependencies\":{value}"));
            let one = format!("{{{head}{}}}", held.unwrap_or_default());
            format!(
                "{{\"workspace_root\":\"/w\",\"workspace_members\":[\"i\"],\
                 \"packages\":[{one},{out}]}}",
            )
        };
        // `dependencies` values, `-` for the field spelled not at all and `!`-prefixed when the
        // document stays readable — which leaves the one member unmapped, BXW0051 and not this.
        let cases = [
            "-",
            "7",
            "{}",
            r#""[]""#,
            "[7]",
            r#"["x"]"#,
            "[[]]",
            "[{}]",
            r#"[{"kind":7}]"#,
            r#"[{"kind":["dev"]}]"#,
            r#"[{"kind":"future"}]"#,
            r#"[{"kind":"Dev"}]"#,
            r#"[{"kind":""}]"#,
            r#"[{"kind":"normal"}]"#,
            r#"[{"kind":null,"path":7}]"#,
            r#"[{"kind":null,"path":null}]"#,
            r#"[{"kind":null,"path":"/w/../x"}]"#,
            r#"[{"kind":null,"path":"/w/a\\b"}]"#,
            r#"[{"kind":null,"path":"/w/"}]"#,
            r#"[{"kind":null,"path":"/w/a/./b"}]"#,
            "![]",
            r#"![{"kind":null,"path":"/w/c"}]"#,
            r#"![{"kind":"dev"}]"#,
            r#"![{"kind":"build","path":"/w2/c"}]"#,
            r#"![{"kind":null,"path":"/w"}]"#,
        ];
        for case in cases {
            let readable = case.strip_prefix('!');
            let field = readable.unwrap_or(case);
            let document = whole((field != "-").then_some(field));
            let held = vec![(MANIFEST, owning("solo", "platform", &[MANIFEST], &[]))];
            let report = mapped(held, &[], &document)
                .check()
                .expect_err("this manifest maps no member");
            let [Entry::Workspace(found)] = report.as_slice() else {
                panic!("{case:?} reported {report}");
            };
            if readable.is_some() {
                assert_eq!(found.code(), "BXW0051", "{case:?}");
                assert_eq!(found.path(), &path("c/Cargo.toml"), "{case:?}");
                continue;
            }
            assert_eq!(found.code(), "BXW0050", "{case:?}");
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
    /// 02-packages' matching rule, satisfied: every Cargo member matches exactly one `[[crates]]`
    /// entry by the *pair* (normalized directory, Cargo package name). A wrong answer is reachable —
    /// the box package's entries re-anchor at its own root, so an unanchored comparison matches none
    /// of them; and two members carry the identical name `twin` in different directories under
    /// different manifests, so matching by name alone claims each twice and reports BXW0053.
    #[test]
    fn every_cargo_member_matches_one_declared_entry() {
        let claimed: [(&str, &str, &str); 3] = [
            ("tool", "tools", "platform"),
            ("fixture-impl", "fix/crate/impl", "platform"),
            ("twin", "tools/twin", "platform"),
        ];
        let owns: [(&str, &str, &str); 3] = [
            ("deep-impl", "implementation", "box-implementation"),
            ("deep-contract", "generated/contract", "box-contract"),
            ("twin", "twin", "box-implementation"),
        ];
        let root = owning("root", "platform", &[MANIFEST], &["fix/**"]);
        let deep = owning("deep", "box", &[MANIFEST], &[]);
        let held = vec![
            (MANIFEST, crates(root, &claimed)),
            ("pkg/boxology.toml", crates(deep, &owns)),
        ];
        let members = [
            ("tools", "tool"),
            ("fix/crate/impl", "fixture-impl"),
            ("pkg/implementation", "deep-impl"),
            ("pkg/generated/contract", "deep-contract"),
            ("tools/twin", "twin"),
            ("pkg/twin", "twin"),
        ];
        let checked = mapped(held, &[], &metadata(&members, &[]))
            .check()
            .expect("every member matches one entry");
        let named = checked.cargo_members().iter();
        let seen: Vec<&str> = named.map(CargoMember::cargo_package).collect();
        let sorted = "deep-contract deep-impl fixture-impl tool twin twin";
        assert_eq!(seen.join(" "), sorted);
    }
    /// BXW0051 codes a Cargo member no entry maps, BXW0052 an entry no member matches. The six
    /// unmapped members fail for five distinct reasons: `plain` is declared nowhere; the member at
    /// the workspace root and the one at the `deep` package's own root can be spelled by no entry at
    /// all; `right/dir` is named by an entry whose path disagrees and `spelled` is located by an
    /// entry whose name disagrees, so each reports **both** codes — the pair rule failing one half at
    /// a time, which a name-only or a path-only rule would answer with silence; and `fix/crate/impl`
    /// is mapped only by a manifest *inside* the pruned fixture subtree. The `ghost` entry is
    /// declared by the *nested* package, so a BXW0052 is located at its own manifest.
    #[test]
    fn unmapped_members_and_unmatched_entries_are_coded() {
        let entries: [(&str, &str, &str); 2] = [
            ("named", "wrong/dir", "platform"),
            ("other", "spelled", "platform"),
        ];
        let ghost: [(&str, &str, &str); 1] = [("ghost", "nowhere", "box-implementation")];
        let root = owning("root", "platform", &[MANIFEST], &["fix/**"]);
        let pruned = owning("fixture", "box", &[MANIFEST], &[]);
        let opaque: [(&str, &str, &str); 1] = [("fixture-impl", "impl", "box-implementation")];
        let deep = crates(owning("deep", "box", &[MANIFEST], &[]), &ghost);
        let held = vec![
            (MANIFEST, crates(root, &entries)),
            ("fix/crate/boxology.toml", crates(pruned, &opaque)),
            ("pkg/boxology.toml", deep),
        ];
        let members = [
            ("", "at-root"),
            ("plain", "plain"),
            ("pkg", "at-pkg-root"),
            ("right/dir", "named"),
            ("spelled", "elsewhere"),
            ("fix/crate/impl", "fixture-impl"),
        ];
        let report = mapped(held, &[], &metadata(&members, &[]))
            .check()
            .expect_err("six members and three entries match nothing");
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0051 Cargo.toml package= candidates=[]",
                "BXW0051 fix/crate/impl/Cargo.toml package= candidates=[]",
                "BXW0051 pkg/Cargo.toml package= candidates=[]",
                "BXW0051 plain/Cargo.toml package= candidates=[]",
                "BXW0051 right/dir/Cargo.toml package= candidates=[]",
                "BXW0051 spelled/Cargo.toml package= candidates=[]",
                "BXW0052 pkg/boxology.toml package=deep candidates=[nowhere]",
                "BXW0052 boxology.toml package=root candidates=[spelled]",
                "BXW0052 boxology.toml package=root candidates=[wrong/dir]",
            ]
        );
        let [Entry::Workspace(bare), .., Entry::Workspace(entry)] = report.as_slice() else {
            panic!("nine workspace findings: {report}");
        };
        // The rendered lines pin code, path, package, and payload byte for byte; these pin what no
        // rendering shows. The literal texts, not the constants they guard.
        assert_eq!(bare.candidates(), [], "a member names no glob claim");
        let unmapped = "every Cargo workspace member must match one declared crate entry";
        assert_eq!(bare.rule(), unmapped);
        assert_eq!(bare.rule_source(), CRATE_SOURCE);
        let unmatched = "every declared crate entry must match one Cargo workspace member";
        assert_eq!(entry.rule(), unmatched);
        assert_eq!(
            entry.rule_source(),
            "specs/s5-manifest-and-validation.md D4"
        );
    }
    /// BXW0053 codes a member two entries claim. It takes two manifests: BXW0029 refuses a repeated
    /// name or path inside one, and a member carries one name. The root package spells the member's
    /// whole directory and the `sub` package spells its tail, so two anchors resolve onto one member,
    /// and the payload names each claim in walk order — outermost first, not the sorted order.
    #[test]
    fn a_member_two_entries_claim_is_coded() {
        let outer: [(&str, &str, &str); 1] = [("shared", "sub/c", "platform")];
        let inner: [(&str, &str, &str); 1] = [("shared", "c", "box-implementation")];
        let root = crates(owning("zulu", "platform", &[MANIFEST], &[]), &outer);
        let deep = crates(owning("alpha", "box", &[MANIFEST], &[]), &inner);
        let held = vec![(MANIFEST, root), ("sub/boxology.toml", deep)];
        let report = mapped(held, &[], &metadata(&[("sub/c", "shared")], &[]))
            .check()
            .expect_err("two entries claim one member");
        assert_eq!(
            report.to_string(),
            "BXW0053 sub/c/Cargo.toml package= candidates=[zulu boxology.toml sub/c,\
             alpha sub/boxology.toml c]"
        );
        let [Entry::Workspace(found)] = report.as_slice() else {
            panic!("one workspace finding: {report}");
        };
        assert_eq!(found.candidates(), [], "no claim is a glob claim");
        let stated = "at most one declared crate entry may match a Cargo workspace member";
        assert_eq!(found.rule(), stated);
        let source = "boxology-details/02-packages.md crate roles";
        assert_eq!(found.rule_source(), source);
    }
    /// BXW0054's whole table, all twelve cells: the four a package kind can host as well as the
    /// eight it cannot. Every case declares one entry that *does* match the one Cargo member, so
    /// nothing but the role decides it, and an accepted cell is a workspace with no finding at all.
    #[test]
    fn impossible_crate_roles_are_coded() {
        // "<kind> <role>", `!`-prefixed when that kind cannot host that role.
        let cases = "box box-implementation,box box-contract,!box composition,!box platform,\
                     !composition box-implementation,!composition box-contract,\
                     composition composition,!composition platform,\
                     !platform box-implementation,!platform box-contract,!platform composition,\
                     platform platform";
        for case in cases.split(',') {
            let impossible = case.strip_prefix('!');
            let Some((kind, role)) = impossible.unwrap_or(case).split_once(' ') else {
                panic!("malformed case {case:?}");
            };
            let mut base = owning("solo", kind, &[MANIFEST], &[]);
            if kind == "composition" {
                base.extend_from_slice(b"[composition]\nboxes = [\"hello\"]\n");
            }
            let held = vec![(MANIFEST, crates(base, &[("c", "c", role)]))];
            let document = metadata(&[("c", "c")], &[]);
            let Err(report) = mapped(held, &[], &document).check() else {
                assert!(impossible.is_none(), "{case:?} cannot be hosted");
                continue;
            };
            assert!(
                impossible.is_some(),
                "{case:?} is hostable, and reported {report}"
            );
            assert_eq!(
                report.to_string(),
                "BXW0054 boxology.toml package=solo candidates=[c]",
                "{case:?}"
            );
            let [Entry::Workspace(found)] = report.as_slice() else {
                panic!("{case:?} reported {report}");
            };
            assert_eq!(found.candidates(), [], "{case:?}");
            let stated = "a declared crate role must be one its package kind can host";
            assert_eq!(found.rule(), stated, "{case:?}");
            let source = "specs/s5-manifest-and-validation.md D4";
            assert_eq!(found.rule_source(), source, "{case:?}");
        }
        // Located at the *declaring* manifest and reported once per impossible role: the nested
        // package declares two, and the root package — first in walk order — declares none.
        let both: [(&str, &str, &str); 2] = [("x", "a", "composition"), ("y", "b", "platform")];
        let deep = crates(owning("deep", "box", &[MANIFEST], &[]), &both);
        let root = owning("root", "platform", &[MANIFEST], &[]);
        let held = vec![(MANIFEST, root), ("pkg/boxology.toml", deep)];
        let report = mapped(held, &[], &metadata(&[("pkg/a", "x"), ("pkg/b", "y")], &[]))
            .check()
            .expect_err("the nested package declares two impossible roles");
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0054 pkg/boxology.toml package=deep candidates=[a]",
                "BXW0054 pkg/boxology.toml package=deep candidates=[b]",
            ]
        );
    }
    /// A declared role is a property of one manifest, so it is judged with no Cargo member at all
    /// and BXW0054 joins BXW0050. Matching does not: the entry below matches nothing, and a BXW0052
    /// per declared entry is the cascade the whole-document reading prevents. Two lines, no third.
    #[test]
    fn roles_are_judged_beside_an_unreadable_document() {
        let base = owning("solo", "platform", &[MANIFEST], &[]);
        let held = vec![(MANIFEST, crates(base, &[("c", "c", "composition")]))];
        let report = mapped(held, &[], "not json")
            .check()
            .expect_err("both defects are reported");
        assert_eq!(
            report.to_string(),
            "BXW0050 Cargo.toml package= candidates=[]\n\
             BXW0054 boxology.toml package=solo candidates=[c]"
        );
    }
    // Cargo cannot represent dev × optional or dev × feature-activated dependencies, so those two
    // cells are vacuous. Every other applicable declaration form is a real metadata shape below.
    const EDGE_FORMS: &[(&str, &str, &str, &str)] = &[
        ("normal", "null", "normal", ""),
        ("build", "\"build\"", "build", ""),
        ("dev", "\"dev\"", "dev", ""),
        (
            "renamed-normal",
            "null",
            "normal",
            "\"name\":\"t\",\"rename\":\"alias\"",
        ),
        (
            "renamed-build",
            "\"build\"",
            "build",
            "\"name\":\"t\",\"rename\":\"alias\"",
        ),
        (
            "renamed-dev",
            "\"dev\"",
            "dev",
            "\"name\":\"t\",\"rename\":\"alias\"",
        ),
        ("optional-normal", "null", "normal", "\"optional\":true"),
        ("optional-build", "\"build\"", "build", "\"optional\":true"),
        (
            "feature-normal",
            "null",
            "normal",
            "\"optional\":true,\"features\":[\"dep:t\"]",
        ),
        (
            "feature-build",
            "\"build\"",
            "build",
            "\"optional\":true,\"features\":[\"dep:t\"]",
        ),
        (
            "target-normal",
            "null",
            "normal",
            "\"target\":\"cfg(unix)\"",
        ),
        (
            "target-build",
            "\"build\"",
            "build",
            "\"target\":\"cfg(unix)\"",
        ),
        ("target-dev", "\"dev\"", "dev", "\"target\":\"cfg(unix)\""),
    ];
    /// The crate role one case letter spells, and the one package kind that hosts it.
    fn plays(letter: &str) -> (&'static str, &'static str) {
        match letter {
            "i" => ("box", "box-implementation"),
            "c" => ("box", "box-contract"),
            "x" => ("composition", "composition"),
            "p" => ("platform", "platform"),
            other => panic!("unknown role {other:?}"),
        }
    }
    fn policy_inputs(
        source: &str,
        target: &str,
        declared: bool,
        selected: bool,
        edge: &str,
    ) -> WorkspaceInputs {
        let (source_kind, source_role) = plays(source);
        let (target_kind, target_role) = plays(target);
        let source_entry = [("s", "s", source_role)];
        let source_manifest = if source == "x" {
            let boxes = if selected { ["b"] } else { ["other"] };
            selecting("a", &source_entry, &boxes, false)
        } else if declared {
            importing("a", &source_entry, &["b"])
        } else {
            roled("a", source_kind, &source_entry)
        };
        let target_entry = [("t", "t", target_role)];
        mapped(
            vec![
                ("a/boxology.toml", source_manifest),
                ("b/boxology.toml", roled("b", target_kind, &target_entry)),
            ],
            &[],
            &depending(&[("a/s", "s", edge), ("b/t", "t", "")], &[]),
        )
    }
    /// Every judged cell of the role-pair table, the *permitted* ones included: two crates joined
    /// by one plain edge, where nothing differs between cases but the two roles and whether one
    /// package or two own them. A permitted cell is a workspace with no finding at all, so the
    /// table is asserted in both directions rather than read back off the code that answers it.
    /// Same-package cases are spelled only where one kind hosts both roles, which is what makes
    /// "same package" observably different from "same role".
    #[test]
    fn role_pair_edges_are_judged() {
        // "<source role> <target role> <same|foreign>", plus the code a forbidden pair reports.
        let cases = "i i same BXW0059,i i foreign BXW0056,i c same,i c foreign BXW0057,\
                     i x foreign BXW0059,i p foreign,c i same BXW0055,c i foreign BXW0055,\
                     c c same BXW0059,c c foreign BXW0057,c x foreign BXW0059,c p foreign,\
                     x i foreign BXW0058,x c foreign BXW0058,\
                     x x same BXW0059,x x foreign BXW0059,x p foreign,\
                     p i foreign BXW0059,p c foreign BXW0059,p x foreign BXW0059,\
                     p p same,p p foreign";
        for case in cases.split(',') {
            let field: Vec<&str> = case.split(' ').collect();
            let [from, onto, scope, expected @ ..] = field.as_slice() else {
                panic!("malformed case {case:?}");
            };
            let (same, (skind, srole), (tkind, trole)) =
                (*scope == "same", plays(from), plays(onto));
            let at = if same { "a/t" } else { "b/t" };
            let target: [(&str, &str, &str); 1] = [("t", "t", trole)];
            let mut first: Vec<(&str, &str, &str)> = vec![("s", "s", srole)];
            let mut held = Vec::new();
            if same {
                first.push(target[0]);
            } else {
                held.push(("b/boxology.toml", roled("b", tkind, &target)));
            }
            held.insert(0, ("a/boxology.toml", roled("a", skind, &first)));
            for &(form, edge_kind, word, extra) in EDGE_FORMS {
                let one = edge(edge_kind, Some(&format!("/w/{at}")), extra);
                let listed = [("a/s", "s", one.as_str()), (at, "t", "")];
                let checked = mapped(held.clone(), &[], &depending(&listed, &[])).check();
                let owner = if same { "a" } else { "b" };
                let packages: &[&str] = if same { &["a"] } else { &["a", "b"] };
                match (&checked, expected) {
                    (Ok(workspace), []) if form == "normal" => {
                        successful_edge(workspace, packages, "s", "a/s/Cargo.toml", "t", at)
                    }
                    (Ok(_), []) => {}
                    (Err(report), [code]) => assert_eq!(
                        report.to_string(),
                        format!("{code} a/s/Cargo.toml package=a candidates=[{owner} {at} {word}]"),
                        "{case} ({form})"
                    ),
                    _ => panic!("{case} ({form}) answered {checked:?}"),
                }
            }
        }
    }
    /// Every declaration-dependent allowed and forbidden rule is crossed with each metadata form;
    /// the assertion is the policy verdict, not merely retention of an edge in the reader.
    #[test]
    fn declaration_policy_matrix_crosses_edge_forms() {
        let cases = "i c undeclared BXW0057,i c declared -,c c undeclared BXW0057,\
                     c c declared -,x i unselected BXW0058,x i selected -,\
                     x c unselected BXW0058,x c selected -";
        for case in cases.split(',') {
            let [source, target, condition, expected] = case.split(' ').collect::<Vec<_>>()[..]
            else {
                panic!("malformed case {case:?}");
            };
            let declared = condition == "declared";
            let selected = condition == "selected";
            for &(form, edge_kind, word, extra) in EDGE_FORMS {
                let one = edge(edge_kind, Some("/w/b/t"), extra);
                let checked = policy_inputs(source, target, declared, selected, &one).check();
                match (checked, expected) {
                    (Ok(_), "-") => {}
                    (Err(report), code) => assert_eq!(
                        report.to_string(),
                        format!("{code} a/s/Cargo.toml package=a candidates=[b b/t {word}]"),
                        "{case} ({form})"
                    ),
                    (answer, _) => panic!("{case} ({form}) answered {answer:?}"),
                }
            }
        }
        for &(form, edge_kind, word, extra) in EDGE_FORMS {
            let one = edge(edge_kind, Some("/w/missing"), extra);
            let held = vec![(
                "a/boxology.toml",
                roled("a", "box", &[("s", "s", "box-implementation")]),
            )];
            let checked = mapped(held, &[], &depending(&[("a/s", "s", &one)], &[]))
                .check()
                .expect_err("a box edge onto a non-member is forbidden");
            assert_eq!(
                checked.to_string(),
                format!("BXW0060 a/s/Cargo.toml package=a candidates=[missing {word}]"),
                "non-member ({form})"
            );
        }
        for &(form, edge_kind, _, extra) in EDGE_FORMS {
            let one = edge(edge_kind, Some("/outside"), extra);
            let held = vec![(
                MANIFEST,
                roled("root", "platform", &[("s", "s", "platform")]),
            )];
            let checked = mapped(held, &[], &depending(&[("s", "s", &one)], &[]))
                .check()
                .expect("a platform edge onto a non-member is allowed");
            assert_eq!(checked.cargo_members().len(), 1, "platform ({form})");
        }
    }
    /// Location, count and scope, all three provable at once — the shape S5-T2 lacked, where every
    /// fixture was a one-package workspace at the root manifest. Three packages, two of them
    /// *nested*, so a crate directory re-anchors under its own package root and a reading anchored
    /// at the workspace root mislocates all four crates. The violating sources are neither the first
    /// package in walk order nor the first member in the document's order. One source declares
    /// **three** forbidden edges — two of which render identically, being one target reached twice
    /// under two `cfg`s, so the pair is *countable* but not attributable: the only thing separating
    /// them is the `target` string, deliberately never read — and a second source declares a fourth
    /// from its own manifest, so a per-source dedupe, a first-violation-only reading, and a constant
    /// location each lose a line. Beside them sit a legal implementation-to-own-contract edge, a
    /// legal edge onto platform material, and a registry dependency: a rule that reports every edge
    /// fails here. The two box packages' identities invert their Cargo names, so a report ordered by
    /// the member order the findings are produced in comes out backwards, and all three edge kinds
    /// are declared, so a kind-collapsing payload fails.
    ///
    /// The workspace also carries **one unmapped member and one unowned file**, so edge findings
    /// interleave with a BXW0051 and a BXW0044 in one report. That asserts the per-member
    /// anti-cascade instead of only documenting it: suppressing the whole edge phase under any
    /// mapping or classification defect — the simpler alternative — would answer a workspace with
    /// both a missing `[[crates]]` entry and a forbidden edge with only the first.
    #[test]
    fn edge_findings_locate_count_and_scope() {
        let tools: [(&str, &str, &str); 1] = [("a-tools", "tools", "platform")];
        let zulu: [(&str, &str, &str); 2] = [
            ("aa-contract", "contract", "box-contract"),
            ("aa-impl", "impl", "box-implementation"),
        ];
        let alpha: [(&str, &str, &str); 2] = [
            ("zz-contract", "contract", "box-contract"),
            ("zz-impl", "impl", "box-implementation"),
        ];
        let held = vec![
            (MANIFEST, roled("root", "platform", &tools)),
            ("pkg/zulu/boxology.toml", roled("zulu", "box", &zulu)),
            ("pkg/alpha/boxology.toml", roled("alpha", "box", &alpha)),
        ];
        // The two `cfg` variants sit adjacent and render identically, as cargo emits them.
        let unix = "\"target\":\"cfg(unix)\"";
        let contract = [
            edge("\"dev\"", Some("/w/pkg/alpha/impl"), ""),
            edge("\"dev\"", Some("/w/pkg/alpha/impl"), unix),
            edge("null", Some("/w/pkg/zulu/impl"), ""),
        ];
        let implementation = [
            edge("\"build\"", Some("/w/pkg/zulu/impl"), ""),
            edge("null", Some("/w/pkg/alpha/contract"), ""),
            edge("null", Some("/w/tools"), ""),
        ];
        let (three, mixed) = (contract.join(","), implementation.join(","));
        let registry = edge("null", None, "\"name\":\"serde\"");
        let members = [
            ("tools", "a-tools", ""),
            ("orphan", "an-orphan", ""),
            ("pkg/zulu/contract", "aa-contract", three.as_str()),
            ("pkg/zulu/impl", "aa-impl", registry.as_str()),
            ("pkg/alpha/contract", "zz-contract", ""),
            ("pkg/alpha/impl", "zz-impl", mixed.as_str()),
        ];
        let report = mapped(held, &["stray.rs"], &depending(&members, &[]))
            .check()
            .expect_err("four declared edges break the table");
        let at = "pkg/zulu/contract/Cargo.toml package=zulu";
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0051 orphan/Cargo.toml package= candidates=[]",
                "BXW0044 stray.rs package= candidates=[]",
                "BXW0056 pkg/alpha/impl/Cargo.toml package=alpha \
                 candidates=[zulu pkg/zulu/impl build]",
                &format!("BXW0055 {at} candidates=[alpha pkg/alpha/impl dev]"),
                &format!("BXW0055 {at} candidates=[alpha pkg/alpha/impl dev]"),
                &format!("BXW0055 {at} candidates=[zulu pkg/zulu/impl normal]"),
            ]
        );
        let [_, _, Entry::Workspace(across), Entry::Workspace(down), ..] = report.as_slice() else {
            panic!("six workspace findings: {report}");
        };
        // The literal texts, not the constants they guard.
        let foreign = "a box implementation must depend on no foreign box implementation";
        assert_eq!(across.rule(), foreign);
        let forbidden = "a box contract crate must depend on no box implementation";
        assert_eq!(down.rule(), forbidden);
        let source = "boxology-details/08-rust-build-topology.md edge table";
        assert_eq!(across.rule_source(), source);
        assert_eq!(down.rule_source(), source);
        assert_eq!(across.candidates(), [], "an edge names no glob claim");
    }
    #[test]
    fn undeclared_foreign_contract_edges_are_coded() {
        let a = [
            ("ai", "i", "box-implementation"),
            ("ac", "c", "box-contract"),
        ];
        let b = [("bc", "c", "box-contract")];
        let d = [
            ("di", "i", "box-implementation"),
            ("dc", "c", "box-contract"),
        ];
        let held = vec![
            ("a/boxology.toml", importing("a", &a, &["b"])),
            ("b/boxology.toml", roled("b", "box", &b)),
            ("d/boxology.toml", roled("d", "box", &d)),
        ];
        let ai = [
            edge("null", Some("/w/b/c"), ""),
            edge("null", Some("/w/d/c"), ""),
        ]
        .join(",");
        let ac = [
            edge("null", Some("/w/b/c"), ""),
            edge("\"build\"", Some("/w/d/c"), ""),
        ]
        .join(",");
        let di = edge("\"dev\"", Some("/w/a/c"), "");
        let members = [
            ("a/i", "ai", ai.as_str()),
            ("a/c", "ac", ac.as_str()),
            ("b/c", "bc", ""),
            ("d/i", "di", di.as_str()),
            ("d/c", "dc", ""),
        ];
        let report = mapped(held, &[], &depending(&members, &[]))
            .check()
            .unwrap_err();
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0057 a/c/Cargo.toml package=a candidates=[d d/c build]",
                "BXW0057 a/i/Cargo.toml package=a candidates=[d d/c normal]",
                "BXW0057 d/i/Cargo.toml package=d candidates=[a a/c dev]",
            ]
        );
        let Entry::Workspace(first) = &report.as_slice()[0] else {
            panic!()
        };
        assert_eq!(
            (first.rule(), first.rule_source()),
            (DECLARED_TEXT, EDGE_SOURCE)
        );
        let held = vec![
            (
                "a/boxology.toml",
                importing("a", &[("ai", "i", "box-implementation")], &["b"]),
            ),
            (
                "b/boxology.toml",
                roled("b", "box", &[("bc", "c", "box-contract")]),
            ),
        ];
        let one = edge("null", Some("/w/b/c"), "");
        let document = depending(&[("a/i", "ai", &one), ("b/c", "bc", "")], &[]);
        let checked = mapped(held, &[], &document)
            .check()
            .expect("declared foreign I-to-C");
        successful_edge(&checked, &["a", "b"], "ai", "a/i/Cargo.toml", "bc", "b/c");
    }
    #[test]
    fn unselected_composition_edges_are_coded() {
        let x = [("x", "x", "composition")];
        let pair = |prefix| {
            [
                ("i", prefix, "box-implementation"),
                ("c", "c", "box-contract"),
            ]
        };
        let held = vec![
            ("x/boxology.toml", selecting("x", &x, &["a"], false)),
            ("a/boxology.toml", roled("a", "box", &pair("i"))),
            ("d/boxology.toml", roled("d", "box", &pair("i"))),
        ];
        let paths = ["/w/a/i", "/w/a/c", "/w/d/i", "/w/d/c"];
        let edges = paths.map(|at| edge("null", Some(at), "")).join(",");
        let members = [
            ("x/x", "x", edges.as_str()),
            ("a/i", "i", ""),
            ("a/c", "c", ""),
            ("d/i", "i", ""),
            ("d/c", "c", ""),
        ];
        let report = mapped(held, &[], &depending(&members, &[]))
            .check()
            .unwrap_err();
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0058 x/x/Cargo.toml package=x candidates=[d d/c normal]",
                "BXW0058 x/x/Cargo.toml package=x candidates=[d d/i normal]",
            ]
        );
        let held = vec![
            ("x/boxology.toml", selecting("x", &x, &["a", "d"], true)),
            (
                "d/boxology.toml",
                roled("d", "box", &[("i", "i", "box-implementation")]),
            ),
        ];
        let one = edge("null", Some("/w/d/i"), "");
        let members = [("x/x", "x", one.as_str()), ("d/i", "i", "")];
        let checked = mapped(held, &[], &depending(&members, &[]))
            .check()
            .expect("selected edge");
        successful_edge(&checked, &["d", "x"], "x", "x/x/Cargo.toml", "i", "d/i");
    }
    #[test]
    fn non_member_path_edges_are_coded() {
        let p = [("p", "p", "platform"), ("near", "prefix-more", "platform")];
        let b = [("i", "i", "box-implementation")];
        let held = vec![
            (MANIFEST, roled("root", "platform", &p)),
            ("b/boxology.toml", roled("b", "box", &b)),
        ];
        let paths = ["/w", "/w/missing", "/w/missing", "/w/prefix", "/outside"];
        let mut edges: Vec<String> = paths
            .iter()
            .enumerate()
            .map(|(n, at)| {
                edge(
                    "null",
                    Some(at),
                    if n == 2 {
                        "\"target\":\"cfg(unix)\""
                    } else {
                        ""
                    },
                )
            })
            .collect();
        edges.push(edge("null", None, "\"name\":\"registry\""));
        let platform = paths
            .iter()
            .map(|at| edge("\"dev\"", Some(at), ""))
            .collect::<Vec<_>>()
            .join(",");
        let members = [
            ("", "root-member", ""),
            ("p", "p", platform.as_str()),
            ("prefix-more", "near", ""),
            ("b/i", "i", &edges.join(",")),
        ];
        let report = mapped(held, &[], &depending(&members, &[]))
            .check()
            .unwrap_err();
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0051 Cargo.toml package= candidates=[]",
                "BXW0060 b/i/Cargo.toml package=b candidates=[missing normal]",
                "BXW0060 b/i/Cargo.toml package=b candidates=[missing normal]",
                "BXW0060 b/i/Cargo.toml package=b candidates=[outside normal]",
                "BXW0060 b/i/Cargo.toml package=b candidates=[prefix normal]",
            ]
        );
    }
    /// No edge into or out of a member this checker cannot role is judged: the mapping finding
    /// already names the document to change, and a verdict would rest on a role that does not
    /// exist. All three ways of lacking one are here — a member no entry maps (BXW0051), one two
    /// entries claim (BXW0053), and one whose declared role its package kind cannot host (BXW0054)
    /// — and each is the source of an edge that would be forbidden under the role it is denied
    /// *and* the target of one. The member at the workspace root is the permanent fourth: no
    /// `[[crates]].path` spells it, so it is unroled by
    /// construction, so its edge is skipped; the final non-member edges are BXW0060.
    #[test]
    fn unroled_members_produce_no_edge_findings() {
        let outer: [(&str, &str, &str); 2] = [
            ("bad", "bad", "box-contract"),
            ("dup", "pkg/dup", "platform"),
        ];
        let inner: [(&str, &str, &str); 3] = [
            ("bx-c", "contract", "box-contract"),
            ("bx-i", "impl", "box-implementation"),
            ("dup", "dup", "box-implementation"),
        ];
        let held = vec![
            (MANIFEST, roled("root", "platform", &outer)),
            ("pkg/boxology.toml", roled("bx", "box", &inner)),
        ];
        let onto = |at: &str| edge("null", Some(at), "");
        let out = onto("/w/pkg/impl");
        let listed = [
            "/w/orphan",
            "/w/bad",
            "/w/pkg/dup",
            "/w",
            "/w/nowhere",
            "/elsewhere/x",
        ];
        let six: Vec<String> = listed.iter().map(|at| onto(at)).collect();
        let members = [
            ("", "at-root", out.as_str()),
            ("bad", "bad", out.as_str()),
            ("orphan", "orphan", out.as_str()),
            ("pkg/contract", "bx-c", &six.join(",")),
            ("pkg/dup", "dup", out.as_str()),
            ("pkg/impl", "bx-i", ""),
        ];
        let report = mapped(held, &[], &depending(&members, &[]))
            .check()
            .expect_err("three members earn no role, and the root member never can");
        assert_eq!(
            report.to_string().lines().collect::<Vec<_>>(),
            [
                "BXW0051 Cargo.toml package= candidates=[]",
                "BXW0051 orphan/Cargo.toml package= candidates=[]",
                "BXW0053 pkg/dup/Cargo.toml package= \
                 candidates=[root boxology.toml pkg/dup,bx pkg/boxology.toml dup]",
                "BXW0060 pkg/contract/Cargo.toml package=bx candidates=[nowhere normal]",
                "BXW0060 pkg/contract/Cargo.toml package=bx candidates=[outside normal]",
                "BXW0054 boxology.toml package=root candidates=[bad]",
            ]
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
