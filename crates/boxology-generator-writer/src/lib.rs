//! The atomic filesystem writer for a [`GeneratedTree`].
//!
//! `boxology-generator-model` holds pure request data and `boxology-generator` performs pure
//! emission; this crate performs the one effect S2 D1 stage 4 assigns to the caller — "the caller
//! writes the complete generated tree atomically". It is a *sibling* of the generator, not a module
//! inside it, so that crate's no-filesystem obligation is structural: `std::fs` cannot be reached
//! from a crate that does not depend on it, and no test has to police the rule.
//!
//! # What [`write()`] guarantees
//!
//! A staged two-phase commit, the shape `xtask`'s determinism publication already uses, and then a
//! prune of what the tree no longer declares.
//!
//! 1. **No declared path changes until every file is staged.** Bytes go to a temporary sibling in
//!    the file's own final directory, so the commit is a same-directory rename that cannot cross a
//!    filesystem and every content failure lands while the destination holds its prior bytes.
//!    A *staging* failure removes the staged siblings first; empty directories are the only residue.
//! 2. **One declared path is replaced atomically**, by one `rename`: a reader sees the complete old
//!    or the complete new bytes, never a half-rewritten file. Replacement installs a fresh inode,
//!    so a mode a developer changed returns to the umask and hard links and xattrs do not survive.
//! 3. **Cross-file atomicity is not claimed.** A `rename` failing after an earlier one succeeded
//!    leaves a mixed tree; the writer reports that and does not unwind, since unwinding needs the
//!    guarantee it would supply, and it names only the failing path — a caller cannot learn which
//!    files were already committed. Cleanup there is untested: no test forces a rename to fail.
//! 4. **An up-to-date tree is untouched.** A file already holding the tree's bytes is never opened
//!    for writing, so its mtime does not move and [`write()`] omits it: regenerating an unchanged
//!    workspace is a true no-op. Unreadable bytes are not equal, so such a file is rewritten.
//! 5. **A declared path is written under exactly the bytes of its own name.** Components are looked
//!    up by *reading the parent directory*, never by stat-ing the joined path: on APFS a stat for
//!    `generated/schema.json` is answered by an existing `generated/SCHEMA.JSON`, so bytes would
//!    land under a name the tree never declared — `Ok` locally, a missing declared output and an
//!    unowned extra one on Linux CI, with no local signal. A parent listing a rival spelling is
//!    refused on *every* platform, so the Linux/macOS parity S5 D8 and S2 D11 make normative is a
//!    property of this crate, not of the filesystem. Rivalry is ASCII-case, so NFD/NFC is residual.
//! 6. **A stale declared output is pruned, and only under a declared pattern.** After the commit
//!    phase, a file under one of the package's `[[derived]].outputs` patterns that the tree does
//!    not declare is removed: S5 D6 step 2 promises `boxology generate --package <id>` repairs a
//!    stale or tampered artifact, and a repair leaving the orphan never clears the finding. Nothing
//!    a pattern does not match is removed, and the match is [`GlobPattern::matches`] itself — the
//!    single definition of the frozen dialect, never a second copy of it here. Guarantee 3 is
//!    **not** widened: this is a third phase, atomic per file and no further, and a removal failure
//!    leaves every declared file committed and live with some orphans behind, which is this crate's
//!    own pre-pruning behaviour. Re-running repairs it: an up-to-date tree writes nothing and the
//!    removals retry.
//!
//! **Commit-then-prune is the only safe order, and it is safe only because of guarantee 5.** The
//! other order trades guarantee 1 away: it would delete live bytes that a later staging failure
//! never replaces. Committing first means the walk sees a tree the write already touched, so on a
//! case-insensitive filesystem a case-only *rename* of a declared output — the old name still on
//! disk — would take the new bytes into the old-cased file and then delete it as an orphan,
//! losing a declared output outright, because [`GlobPattern::matches`] and the declared-set test
//! both compare bytes. [`WriteError::Aliased`] refuses that whole write before anything is staged,
//! on every platform, so no walked entry can differ from a declared path by ASCII case. Unicode
//! case and NFD/NFC stay residual exactly as in guarantee 5.
//!
//! **The walk is bounded by the patterns, not by the tree.** Each pattern contributes its literal
//! directory prefix, its leading wildcard-free segments, and the walk covers the union of those
//! subtrees and nothing else; only a pattern whose first segment holds a wildcard reaches the
//! destination root itself. Within a subtree it descends into every real directory, including ones
//! no pattern can match below, because [`GlobPattern`] answers about whole file paths and a
//! "could still match under here" predicate would be exactly the second matcher this crate must not
//! mint; the cost is a listing, never a removal. It descends only into entries a parent lists as
//! real directories, so it never follows a symlink and terminates on the finite acyclic
//! real-directory tree, visiting each subtree at most once per pattern.
//!
//! Removals are ordered and reported by path bytes, never by directory order. A candidate is a
//! *regular file* whose root-relative name is a valid [`RelativePath`]: a symlink, a directory, and
//! a name outside that frozen grammar are left in place — the grammar cannot express them, so no
//! pattern can be asked about them — and an emptied directory is not removed, since empty
//! directories remain this crate's only residue (guarantee 1). This call's own staged siblings are
//! all renamed away before the walk starts, so it never deletes them; a *concurrent* writer's are
//! candidates like any other file, which cleans up after a crashed write and is one more reason
//! two `write()` calls into one destination are not supported. They never were.
//!
//! **There is no traversal guard on the write: confinement rests on `generate` building paths from
//! literals.** It zips the `OUTPUTS` constants with the emitted bodies, so a tree's paths are those
//! four literals whatever the request declared — `require_exact_outputs` never feeds them — and
//! [`GeneratedTree`] has a private field and no constructor. Nothing there inspects a component for
//! `..`, and `resolve` returns a path above `root` if handed one. **A public `GeneratedTree`
//! constructor must add a component guard in the same change: it is the only thing standing
//! between one and a write outside the destination root.** Pruning is confined independently: every
//! path it removes is a [`RelativePath`] read out of a directory under `root`, and that grammar
//! rejects `..`, absolute, and drive-prefixed spellings.
//!
//! A [`WriteError`] renders only the tree's own logical paths, [`RelativePath`] spellings this
//! crate validated before echoing, and `&'static str` this crate chose.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_generator::{GeneratedFile, GeneratedTree};
use boxology_manifest::{GlobPattern, RelativePath};
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Why a write was refused before it began, or how the filesystem refused it.
#[derive(Debug)]
pub enum WriteError {
    /// The destination root is not an existing directory.
    Destination,
    /// A declared logical path resolves to an entry its own parent spells some other way.
    Aliased(String),
    /// An existing ancestor of a declared logical path is not a real directory.
    Ancestor(String),
    /// A declared logical path exists as something other than a regular file.
    Occupied(String),
    /// The filesystem refused an operation on a declared logical path.
    Io(String, io::Error),
    /// The filesystem refused to remove a stale file under a declared output pattern.
    Remove(String, io::Error),
}

/// What one [`write()`] changed under the destination root.
///
/// Two lists rather than one tagged list of changed paths: they are produced by different phases
/// under different guarantees, a report renders them under different verbs, and no path can appear
/// in both. Both empty means the destination already held exactly the declared tree and nothing
/// stale under a declared pattern, so [`write()`] modified nothing at all.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Changes {
    /// Declared logical paths created or replaced, in the tree's own path-byte order.
    pub written: Vec<String>,
    /// Stale paths removed, in path-byte order; each matched a declared output pattern.
    pub removed: Vec<String>,
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Destination => formatter.write_str("destination root is not a directory"),
            Self::Aliased(path) => write!(formatter, "spelled another way on disk: {path}"),
            Self::Ancestor(path) => write!(formatter, "a parent is not a directory: {path}"),
            Self::Occupied(path) => write!(formatter, "not a regular file: {path}"),
            Self::Io(path, error) => write!(formatter, "write {path}: {error}"),
            Self::Remove(path, error) => write!(formatter, "remove {path}: {error}"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Writes every file `tree` declares under `root`, staging the whole tree before committing it,
/// then removes files under an `outputs` pattern that `tree` does not declare.
///
/// `outputs` is the package's declared `[[derived]].outputs` patterns, anchored at `root` like the
/// manifest that declared them; an empty slice prunes nothing. Pruning runs on every accepted
/// write, including one that changed no bytes, since a stale file outlives an up-to-date tree. The
/// crate documentation states the phase order, its reason, and what a failure leaves.
///
/// # Errors
/// Returns [`WriteError`] when `root` is not an existing directory, an existing entry blocks or
/// misspells a declared path, or the filesystem refuses an operation.
pub fn write(
    root: &Path,
    tree: &GeneratedTree,
    outputs: &[GlobPattern],
) -> Result<Changes, WriteError> {
    if !fs::metadata(root).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(WriteError::Destination);
    }
    let mut plan = Vec::new();
    for file in tree.files() {
        let target = resolve(root, file.path())?;
        if fs::read(&target).is_ok_and(|bytes| bytes == file.bytes()) {
            continue;
        }
        plan.push((file.path(), target, file.bytes()));
    }
    let mut staged = Vec::new();
    for (path, target, bytes) in &plan {
        match stage(target, bytes) {
            Ok(temporary) => staged.push(temporary),
            Err(error) => {
                discard(&staged);
                return Err(WriteError::Io((*path).to_owned(), error));
            }
        }
    }
    let mut written = Vec::new();
    for (index, ((path, target, _), temporary)) in plan.iter().zip(&staged).enumerate() {
        if let Err(error) = fs::rename(temporary, target) {
            discard(&staged[index..]);
            return Err(WriteError::Io((*path).to_owned(), error));
        }
        written.push((*path).to_owned());
    }
    let removed = prune(root, tree, outputs)?;
    Ok(Changes { written, removed })
}

/// Removes every file under an `outputs` pattern that `tree` does not declare (guarantee 6).
///
/// Reports the failing path and stops on the first refusal, leaving the committed tree live and
/// the remaining orphans in place; it never unwinds a removal, which it could not do anyway.
fn prune(
    root: &Path,
    tree: &GeneratedTree,
    outputs: &[GlobPattern],
) -> Result<Vec<String>, WriteError> {
    let declared: Vec<&str> = tree.files().iter().map(GeneratedFile::path).collect();
    let mut removed = Vec::new();
    for candidate in candidates(root, outputs) {
        let path = candidate.as_str();
        let matched = |pattern: &GlobPattern| pattern.matches(&candidate);
        if declared.contains(&path) || !outputs.iter().any(matched) {
            continue;
        }
        if let Err(error) = fs::remove_file(root.join(path)) {
            return Err(WriteError::Remove(path.to_owned(), error));
        }
        removed.push(path.to_owned());
    }
    Ok(removed)
}

/// Every regular file under some pattern's literal prefix, as sorted deduplicated logical paths.
///
/// Deduplicated because overlapping patterns share a subtree, and a path offered twice would be
/// removed twice — the second attempt failing on a file this call itself deleted.
fn candidates(root: &Path, outputs: &[GlobPattern]) -> Vec<RelativePath> {
    let mut pending: Vec<String> = outputs.iter().map(literal_prefix).collect();
    let mut found = Vec::new();
    while let Some(prefix) = pending.pop() {
        let Ok(entries) = fs::read_dir(root.join(&prefix)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let logical = format!("{prefix}{name}");
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(format!("{logical}/")),
                Ok(kind) if kind.is_file() => found.extend(RelativePath::new(logical)),
                _ => {}
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The pattern's leading wildcard-free directory segments with a trailing `/`, empty for `root`.
///
/// Every file a pattern can match lies under this directory: `*` never crosses `/`, so the first
/// segment holding one bounds the descent, and the last segment names the file, not a directory.
fn literal_prefix(pattern: &GlobPattern) -> String {
    let mut prefix = String::new();
    let mut segments = pattern.as_str().split('/').peekable();
    while let Some(segment) = segments.next() {
        if segments.peek().is_none() || segment.contains('*') {
            break;
        }
        prefix.push_str(segment);
        prefix.push('/');
    }
    prefix
}

/// Resolves one logical path against `root`, spelling every component exactly (guarantee 5).
fn resolve(root: &Path, logical: &str) -> Result<PathBuf, WriteError> {
    let mut path = root.to_path_buf();
    let mut components = logical.split('/').peekable();
    while let Some(component) = components.next() {
        let last = components.peek().is_none();
        let found = lookup(&path, component);
        path.push(component);
        let Ok(found) = found else {
            return Err(WriteError::Aliased(logical.to_owned()));
        };
        let Some(kind) = found else {
            continue;
        };
        // `DirEntry::file_type` does not follow links, so a symlink is neither a file nor a
        // directory here and fails the same test an occupied path fails.
        let ok = if last { kind.is_file() } else { kind.is_dir() };
        if !ok {
            let path = logical.to_owned();
            return Err(if last {
                WriteError::Occupied(path)
            } else {
                WriteError::Ancestor(path)
            });
        }
    }
    Ok(path)
}

/// The type of the entry `parent` spells exactly `name`; `Err` when it lists only a rival spelling.
fn lookup(parent: &Path, name: &str) -> Result<Option<fs::FileType>, ()> {
    let Ok(entries) = fs::read_dir(parent) else {
        return Ok(None);
    };
    let mut rival = false;
    for entry in entries.flatten() {
        let found = entry.file_name();
        if found == *name {
            return Ok(entry.file_type().ok());
        }
        let other = found.to_str();
        rival |= other.is_some_and(|other| other.eq_ignore_ascii_case(name));
    }
    if rival { Err(()) } else { Ok(None) }
}

/// Materializes `bytes` as a temporary sibling of `target`, returning the staged path.
fn stage(target: &Path, bytes: &[u8]) -> Result<PathBuf, io::Error> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".boxology-write-{}-{unique}", std::process::id()));
    fs::write(&temporary, bytes)?;
    Ok(temporary)
}

/// Removes staged siblings that will never be committed, reporting the original failure instead.
fn discard(staged: &[PathBuf]) {
    for temporary in staged {
        let _ = fs::remove_file(temporary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::BoxId;
    use boxology_generator::{OUTPUTS, generate};
    use boxology_generator_model::GenerationRequest;
    use boxology_manifest::{LineColumn, Span};
    use std::time::SystemTime;

    const SOURCE: &str = r#"boxology::contract! {
    #[error] pub enum GreetError { EmptyName }
    #[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, GreetError>;
}
"#;
    /// The declared output set in the order a tree carries it: sorted by path bytes.
    const SORTED: [&str; 4] = [
        "generated/adapter/adapter.rs",
        "generated/contract/Cargo.toml",
        "generated/contract/src/lib.rs",
        "generated/schema.json",
    ];
    /// A package's declared output patterns: two overlapping on `generated/contract`, one whose
    /// wildcard is not in its last segment, one literal, and one whose subtree is absent.
    const DECLARED: [&str; 5] = [
        "generated/**",
        "generated/contract/*.toml",
        "vendor/*/old.json",
        "cache/old.bin",
        "docs/**",
    ];
    static TEMP: AtomicU64 = AtomicU64::new(0);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let unique = TEMP.fetch_add(1, Ordering::Relaxed);
            let name = format!("{}-{unique}", std::process::id());
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/generator-writer-tests")
                .join(name);
            // `Drop` cannot remove a directory a test made read-only, so a failed run can leave
            // this exact name behind. Start from nothing: a repeat at the same pid is unpoisoned.
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tree() -> GeneratedTree {
        let manifest = b"schema = 1\nid = \"hello\"\nkind = \"box\"\n".to_vec();
        let request = GenerationRequest::new(
            BoxId::new("hello").unwrap(),
            "src/lib.rs".into(),
            vec![
                ("boxology.toml".into(), manifest),
                ("src/lib.rs".into(), SOURCE.as_bytes().to_vec()),
            ],
            vec![],
            OUTPUTS.iter().map(|path| (*path).into()).collect(),
        )
        .unwrap();
        generate(&request).unwrap()
    }

    fn globs(patterns: &[&str]) -> Vec<GlobPattern> {
        let here = RelativePath::new("boxology.toml").unwrap();
        let point = Span::new(LineColumn::new(1, 1), LineColumn::new(1, 1));
        let parse = |pattern: &&str| GlobPattern::parse(pattern, &here, point).unwrap();
        patterns.iter().map(parse).collect()
    }

    /// Every regular file under `root`, as sorted root-relative slash-joined paths.
    fn listing(root: &Path) -> Vec<String> {
        let mut found = Vec::new();
        walk(root, "", &mut found);
        found.sort();
        found
    }

    fn walk(directory: &Path, prefix: &str, found: &mut Vec<String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let logical = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &format!("{logical}/"), found);
            } else {
                found.push(logical);
            }
        }
    }

    fn stamps(root: &Path) -> Vec<SystemTime> {
        let stamp = |path| fs::metadata(root.join(path)).unwrap().modified().unwrap();
        SORTED.into_iter().map(stamp).collect()
    }

    #[test]
    fn a_write_creates_the_declared_tree_and_leaves_every_other_file_alone() {
        let mut declared = OUTPUTS.to_vec();
        declared.sort_unstable();
        assert_eq!(declared, SORTED, "the public output set is the written set");
        let temp = Temp::new();
        let tree = tree();
        fs::create_dir_all(temp.0.join("generated/contract/src")).unwrap();
        fs::write(temp.0.join("generated/stale.json"), "stale\n").unwrap();
        let changes = write(&temp.0, &tree, &globs(&["generated/contract/**"])).unwrap();
        assert_eq!(changes.written, SORTED);
        assert_eq!(changes.removed, Vec::<String>::new());
        for file in tree.files() {
            let written = fs::read(temp.0.join(file.path())).unwrap();
            assert_eq!(written, file.bytes(), "{}", file.path());
        }
        let survivor = fs::read_to_string(temp.0.join("generated/stale.json"));
        assert_eq!(survivor.unwrap(), "stale\n", "outside every pattern");
        let mut expected = SORTED.to_vec();
        expected.push("generated/stale.json"); // already sorts last: `sc` < `st`
        assert_eq!(listing(&temp.0), expected);
    }

    #[test]
    fn a_repeated_write_touches_only_the_file_whose_bytes_drifted() {
        let temp = Temp::new();
        let tree = tree();
        let declared = globs(&DECLARED);
        write(&temp.0, &tree, &declared).unwrap();
        let before = stamps(&temp.0);
        let again = write(&temp.0, &tree, &declared).unwrap();
        assert_eq!(again, Changes::default());
        assert_eq!(listing(&temp.0), SORTED, "a skipped file stages no sibling");
        assert_eq!(stamps(&temp.0), before, "an unchanged tree stays untouched");
        fs::write(temp.0.join(SORTED[3]), b"drift\n").unwrap();
        let drifted = write(&temp.0, &tree, &declared).unwrap();
        assert_eq!(drifted.written, [SORTED[3]]);
        assert_eq!(drifted.removed, Vec::<String>::new());
        let after = stamps(&temp.0);
        assert_eq!(after[..3], before[..3], "only drift may be rewritten");
        let schema = tree.files().iter().find(|f| f.path() == SORTED[3]).unwrap();
        assert_eq!(fs::read(temp.0.join(SORTED[3])).unwrap(), schema.bytes());
    }

    #[test]
    #[cfg(unix)]
    fn a_staging_failure_leaves_no_declared_or_staged_file_behind() {
        use std::os::unix::fs::PermissionsExt;
        let temp = Temp::new();
        let blocked = temp.0.join("generated/contract/src");
        fs::create_dir_all(&blocked).unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o500)).unwrap();
        let probe = fs::write(blocked.join("probe"), b"x");
        assert!(probe.is_err(), "read-only directory must reject writes");
        let error = write(&temp.0, &tree(), &globs(&DECLARED)).unwrap_err();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
        let blamed = matches!(&error, WriteError::Io(p, _) if p == SORTED[2]);
        assert!(blamed, "{error}");
        assert_eq!(listing(&temp.0), Vec::<String>::new());
    }

    /// Every refusal reachable through the public API, asserting the exact surviving listing —
    /// identical on a case-sensitive and a case-insensitive filesystem. Each blocking seed is
    /// under a declared pattern and undeclared by the tree, so the surviving listing is also what
    /// pins that a refused write prunes nothing: the `SCHEMA.JSON` seed is the case-only rename
    /// that commit-then-prune would otherwise write into and then delete.
    #[test]
    #[cfg(unix)]
    fn every_refusal_through_write_leaves_the_destination_untouched() {
        let temp = Temp::new();
        fs::write(temp.0.join("occupied"), b"x").unwrap();
        for name in ["occupied", "absent"] {
            let refused = write(&temp.0.join(name), &tree(), &globs(&DECLARED)).unwrap_err();
            assert!(matches!(refused, WriteError::Destination), "{name}");
        }
        assert_eq!(listing(&temp.0), ["occupied"]);
        let secret = temp.0.join("occupied");
        for (seed, tag, blocked) in [
            ("GENERATED", "aliased", SORTED[0]),
            ("generated/SCHEMA.JSON", "aliased", SORTED[3]),
            ("generated/schema.json", "occupied", SORTED[3]),
            ("generated/contract", "ancestor", SORTED[1]),
        ] {
            let temp = Temp::new();
            let path = temp.0.join(seed);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if seed.ends_with("schema.json") {
                std::os::unix::fs::symlink(&secret, &path).unwrap();
            } else {
                fs::write(&path, b"blocking\n").unwrap();
            }
            let refusal = match write(&temp.0, &tree(), &globs(&DECLARED)) {
                Err(WriteError::Aliased(seen)) => format!("aliased {seen}"),
                Err(WriteError::Occupied(seen)) => format!("occupied {seen}"),
                Err(WriteError::Ancestor(seen)) => format!("ancestor {seen}"),
                other => format!("{other:?}"),
            };
            assert_eq!(refusal, format!("{tag} {blocked}"), "{seed}");
            assert_eq!(listing(&temp.0), [seed], "{seed}");
        }
        assert_eq!(fs::read(&secret).unwrap(), b"x");
    }

    /// The exact destination listing after pruning, over every stale shape a declared pattern
    /// reaches: an orphan beside a declared output, one in a directory the tree no longer
    /// populates, one two patterns match at once, one under a mid-segment wildcard, and one a
    /// pattern names literally. A file under a pattern's prefix that no pattern matches, a file no
    /// prefix reaches, and a name the frozen path grammar cannot express all survive; and pruning
    /// happens whether or not the same call wrote anything.
    #[test]
    fn a_write_prunes_stale_files_under_a_declared_pattern_and_nothing_else() {
        let temp = Temp::new();
        let tree = tree();
        let stale = [
            "cache/old.bin",
            "generated/contract/old.toml",
            "generated/legacy/old.rs",
            "generated/stale.json",
            "vendor/pkg/old.json",
        ];
        let kept = ["keep.txt", "vendor/notes.txt"];
        for path in stale.iter().chain(&kept) {
            let target = temp.0.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, "stale\n").unwrap();
        }
        // A backslash is a legal POSIX filename byte and a Windows separator; only here can a name
        // outside `RelativePath`'s grammar be created, and `generated/**` would match its bytes.
        #[cfg(unix)]
        fs::write(temp.0.join("generated/we\\ird.json"), "stale\n").unwrap();
        let changes = write(&temp.0, &tree, &globs(&DECLARED)).unwrap();
        assert_eq!(changes.written, SORTED);
        assert_eq!(changes.removed, stale, "removed in path-byte order");
        let mut expected = SORTED.to_vec();
        expected.extend(kept);
        #[cfg(unix)]
        expected.push("generated/we\\ird.json");
        expected.sort_unstable();
        assert_eq!(listing(&temp.0), expected);
        let emptied = fs::read_dir(temp.0.join("generated/legacy"));
        assert_eq!(emptied.unwrap().count(), 0, "the directory survives");
        fs::write(temp.0.join(stale[3]), "again\n").unwrap();
        let again = write(&temp.0, &tree, &globs(&DECLARED)).unwrap();
        assert_eq!(again.written, Vec::<String>::new());
        assert_eq!(again.removed, [stale[3]], "an unchanged tree prunes");
    }

    /// The commit phase lands before the walk, and a removal the filesystem refuses is reported
    /// rather than unwound: every declared file is live, the orphan that could not be removed
    /// survives, and so does the symlink the walk considered — and skipped — one step earlier.
    #[test]
    #[cfg(unix)]
    fn a_prune_failure_reports_it_and_leaves_the_committed_tree_live() {
        use std::os::unix::fs::PermissionsExt;
        let temp = Temp::new();
        let sealed = temp.0.join("generated/legacy");
        fs::create_dir_all(&sealed).unwrap();
        fs::write(sealed.join("old.rs"), "old\n").unwrap();
        fs::write(temp.0.join("keep.txt"), "keep\n").unwrap();
        let link = temp.0.join("generated/anchor.json"); // sorts before `generated/legacy/`
        std::os::unix::fs::symlink("../keep.txt", &link).unwrap();
        fs::set_permissions(&sealed, fs::Permissions::from_mode(0o500)).unwrap();
        let probe = fs::remove_file(sealed.join("old.rs"));
        assert!(probe.is_err(), "read-only directory must reject removals");
        let error = write(&temp.0, &tree(), &globs(&DECLARED)).unwrap_err();
        fs::set_permissions(&sealed, fs::Permissions::from_mode(0o700)).unwrap();
        let blamed = matches!(&error, WriteError::Remove(p, _) if p == "generated/legacy/old.rs");
        assert!(blamed, "{error}");
        let mut expected = SORTED.to_vec();
        expected.extend(["generated/anchor.json", "generated/legacy/old.rs"]);
        expected.push("keep.txt");
        expected.sort_unstable();
        assert_eq!(listing(&temp.0), expected);
        let target = fs::read_to_string(temp.0.join("keep.txt"));
        assert_eq!(target.unwrap(), "keep\n", "the symlink target is intact");
    }
}
