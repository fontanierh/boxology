//! The atomic filesystem writer for a [`GeneratedTree`].
//!
//! `boxology-generator-model` holds pure request data and `boxology-generator` performs pure
//! emission; this crate performs the one effect S2 D1 stage 4 assigns to the caller — per-file
//! staged commit plus per-file prune of the generated tree. It is a *sibling* of the generator, not
//! a module inside it, so that crate's no-filesystem obligation is structural: `std::fs` cannot be
//! reached from a crate that does not depend on it, and no test has to police the rule.
//!
//! # What [`write()`] guarantees
//!
//! A staged two-phase commit, the shape `xtask`'s determinism publication already uses, and then a
//! prune of what the tree no longer declares.
//!
//! 1. **No declared path changes until every file is staged.** Bytes go to a temporary sibling in
//!    the file's own final directory, so the commit is a same-directory rename that cannot cross a
//!    filesystem and every content failure lands while the destination holds its prior bytes.
//!    Staging allocates an exclusive same-directory name (`create_new`) and never opens an existing
//!    path, so a planted symlink or leftover sibling cannot be truncated or followed. On a *staging*
//!    failure, previously completed staged siblings are best-effort discarded and the current partial
//!    sibling is best-effort removed; the original staging/write error is preserved even when cleanup
//!    itself is refused. No declared path has changed, but an undeclared `.boxology-write-*` sibling
//!    may remain if cleanup is refused. Exclusive allocation means a re-run never adopts or truncates
//!    that residue, and a successful re-run's prune removes matching undeclared residue under a
//!    declared outputs pattern. Empty directories created for staging may also remain.
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
//!    unowned extra one on Linux CI, with no local signal. Lookup scans the full parent listing and
//!    refuses on *every* platform when any distinct ASCII-case-equivalent spelling appears — alone
//!    or beside the exact name — so an exact+rival pair on a case-sensitive filesystem is refused
//!    rather than written then pruned. A parent-listing, entry, or file-type failure refuses the
//!    write before staging rather than treating the component as absent. The Linux/macOS parity S5
//!    D8 and S2 D11 make normative is a property of this crate, not of the filesystem. Rivalry is
//!    ASCII-case, so NFD/NFC is residual.
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
//! real-directory tree, visiting each subtree at most once per pattern. A missing prefix directory
//! is skipped; any other directory-enumeration failure fails closed as [`WriteError::Walk`] and
//! leaves the committed tree live.
//!
//! Removals are ordered and reported by path bytes, never by directory order. A candidate is a
//! *regular file* whose root-relative name is a valid [`RelativePath`]: a symlink, a directory, and
//! a name outside that frozen grammar are left in place — the grammar cannot express them, so no
//! pattern can be asked about them — and an emptied directory is not removed. This call's own staged
//! siblings are all renamed away before the walk starts when staging and commit succeed, so it never
//! deletes them; a concurrent or prior writer's leftover `.boxology-write-*` siblings are candidates
//! like any other undeclared file under a declared pattern, which is how a successful re-run converges
//! refused staging cleanup (guarantee 1). Two `write()` calls into one destination are not supported.
//! They never were.
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
    fmt, fs,
    fs::OpenOptions,
    io::{self, Write},
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
    /// Pre-write parent-listing, entry, or file-type lookup failed for a declared logical path.
    Inspect(String, io::Error),
    /// The filesystem refused a stage or rename operation on a declared logical path.
    Io(String, io::Error),
    /// The filesystem refused to remove a stale file under a declared output pattern.
    Remove(String, io::Error),
    /// Prune candidate discovery could not enumerate a directory under a declared outputs prefix.
    Walk(String, io::Error),
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
            Self::Inspect(path, error) => write!(formatter, "inspect {path}: {error}"),
            Self::Io(path, error) => write!(formatter, "write {path}: {error}"),
            Self::Remove(path, error) => write!(formatter, "remove {path}: {error}"),
            Self::Walk(path, error) => write!(formatter, "walk {path}: {error}"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Writes every file `tree` declares under `root`, staging all changed bytes before any declared
/// path changes, then removes files under an `outputs` pattern that `tree` does not declare.
///
/// `outputs` is the package's declared `[[derived]].outputs` patterns, anchored at `root` like the
/// manifest that declared them; an empty slice prunes nothing. Pruning runs on every accepted
/// write, including one that changed no bytes, since a stale file outlives an up-to-date tree.
///
/// # Errors
/// Returns [`WriteError`] when `root` is not an existing directory, an existing entry blocks or
/// misspells a declared path, pre-write or prune enumeration fails closed, or the filesystem
/// refuses an operation.
pub fn write(
    root: &Path,
    tree: &GeneratedTree,
    outputs: &[GlobPattern],
) -> Result<Changes, WriteError> {
    write_with(root, tree, outputs, os_list_dir, os_stage_write)
}

fn write_with(
    root: &Path,
    tree: &GeneratedTree,
    outputs: &[GlobPattern],
    list_dir: ListDirFn,
    stage_write: StageWriteFn,
) -> Result<Changes, WriteError> {
    if !fs::metadata(root).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(WriteError::Destination);
    }
    let mut plan = Vec::new();
    for file in tree.files() {
        let target = resolve(root, file.path(), list_dir)?;
        if fs::read(&target).is_ok_and(|bytes| bytes == file.bytes()) {
            continue;
        }
        plan.push((file.path(), target, file.bytes()));
    }
    let mut staged = Vec::new();
    for (path, target, bytes) in &plan {
        match stage(target, bytes, stage_write) {
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
    let removed = prune_with(root, tree, outputs, list_dir)?;
    Ok(Changes { written, removed })
}

/// Removes every file under an `outputs` pattern that `tree` does not declare (guarantee 6).
///
/// Reports the failing path and stops on the first refusal, leaving the committed tree live and
/// the remaining orphans in place; it never unwinds a removal, which it could not do anyway.
fn prune_with(
    root: &Path,
    tree: &GeneratedTree,
    outputs: &[GlobPattern],
    list_dir: ListDirFn,
) -> Result<Vec<String>, WriteError> {
    let declared: Vec<&str> = tree.files().iter().map(GeneratedFile::path).collect();
    let mut removed = Vec::new();
    for candidate in candidates(root, outputs, list_dir)? {
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

/// Directory entry kind as reported by a parent listing without following symlinks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EntryKind {
    Dir,
    File,
    Other,
}

/// One UTF-8 directory entry name and its unfollowed kind.
type ListedEntry = (String, EntryKind);

/// Enumerates one directory for pre-write lookup and prune candidate discovery.
type ListDirFn = fn(&Path) -> io::Result<Vec<ListedEntry>>;

/// Writes staged bytes after exclusive sibling creation.
type StageWriteFn = fn(&mut fs::File, &[u8]) -> io::Result<()>;

/// Production staged-byte write.
fn os_stage_write(file: &mut fs::File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)
}

/// Production directory listing: `NotFound` is handled by callers; other errors propagate.
fn os_list_dir(path: &Path) -> io::Result<Vec<ListedEntry>> {
    let mut listed = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let kind = entry.file_type()?;
        let kind = if kind.is_dir() {
            EntryKind::Dir
        } else if kind.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        listed.push((name, kind));
    }
    Ok(listed)
}

/// Every regular file under some pattern's literal prefix, as sorted deduplicated logical paths.
///
/// Deduplicated because overlapping patterns share a subtree, and a path offered twice would be
/// removed twice — the second attempt failing on a file this call itself deleted. A missing prefix
/// directory is skipped; any other enumeration failure fails closed.
fn candidates(
    root: &Path,
    outputs: &[GlobPattern],
    list_dir: ListDirFn,
) -> Result<Vec<RelativePath>, WriteError> {
    let mut pending: Vec<String> = outputs.iter().map(literal_prefix).collect();
    let mut found = Vec::new();
    while let Some(prefix) = pending.pop() {
        let walk_path = if prefix.is_empty() {
            ".".to_owned()
        } else {
            prefix.trim_end_matches('/').to_owned()
        };
        let listed = match list_dir(&root.join(&prefix)) {
            Ok(listed) => listed,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(WriteError::Walk(walk_path, error)),
        };
        for (name, kind) in listed {
            let logical = format!("{prefix}{name}");
            match kind {
                EntryKind::Dir => pending.push(format!("{logical}/")),
                EntryKind::File => found.extend(RelativePath::new(logical)),
                EntryKind::Other => {}
            }
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
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
fn resolve(root: &Path, logical: &str, list_dir: ListDirFn) -> Result<PathBuf, WriteError> {
    let mut path = root.to_path_buf();
    let mut components = logical.split('/').peekable();
    while let Some(component) = components.next() {
        let last = components.peek().is_none();
        let found = match lookup(&path, component, list_dir) {
            Ok(Ok(found)) => found,
            Ok(Err(())) => return Err(WriteError::Aliased(logical.to_owned())),
            Err(error) => return Err(WriteError::Inspect(logical.to_owned(), error)),
        };
        path.push(component);
        let Some(kind) = found else {
            continue;
        };
        // Listing does not follow links, so a symlink is `Other` and fails the same test an
        // occupied path fails.
        let ok = matches!(
            (last, kind),
            (true, EntryKind::File) | (false, EntryKind::Dir)
        );
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

/// Pure per-listing decision for one path component name.
///
/// Scans every entry. `Ok(true)` means the exact spelling is present and no distinct ASCII-case
/// rival is. `Ok(false)` means the exact spelling is absent and no rival is. `Err(())` means any
/// distinct ASCII-case-equivalent spelling appeared, whether or not the exact name did too.
fn lookup_decision<'a, I>(name: &str, entries: I) -> Result<bool, ()>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut exact = false;
    let mut rival = false;
    for entry in entries {
        if entry == name {
            exact = true;
        } else if entry.eq_ignore_ascii_case(name) {
            rival = true;
        }
    }
    if rival { Err(()) } else { Ok(exact) }
}

/// The kind of the entry `parent` spells exactly `name`.
///
/// `Ok(Ok(kind))` is the scan-complete presence result. `Ok(Err(()))` is a rival spelling.
/// `Err` is a directory, entry, or file-type failure; a missing parent is presence-`None` so a
/// greenfield intermediate component can still be created.
fn lookup(
    parent: &Path,
    name: &str,
    list_dir: ListDirFn,
) -> Result<Result<Option<EntryKind>, ()>, io::Error> {
    let listed = match list_dir(parent) {
        Ok(listed) => listed,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Ok(None)),
        Err(error) => return Err(error),
    };
    match lookup_decision(name, listed.iter().map(|(entry, _)| entry.as_str())) {
        Ok(true) => {
            let kind = listed
                .into_iter()
                .find_map(|(entry, kind)| (entry == name).then_some(kind));
            Ok(Ok(kind))
        }
        Ok(false) => Ok(Ok(None)),
        Err(()) => Ok(Err(())),
    }
}

/// Upper bound on exclusive staging-name allocation attempts for one file.
const STAGE_NAME_ATTEMPTS: u32 = 64;

/// Materializes `bytes` as a temporary sibling of `target`, returning the staged path.
///
/// Never opens an existing path: each attempt uses `create_new`, and `AlreadyExists` advances the
/// counter and retries up to [`STAGE_NAME_ATTEMPTS`]. A write failure after exclusive creation
/// best-effort removes that sibling and always propagates the original write error; a refused
/// cleanup never replaces it and may leave an undeclared `.boxology-write-*` residue. Other open
/// errors propagate immediately.
fn stage(target: &Path, bytes: &[u8], stage_write: StageWriteFn) -> Result<PathBuf, io::Error> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let pid = std::process::id();
    for _ in 0..STAGE_NAME_ATTEMPTS {
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".boxology-write-{pid}-{unique}"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(mut file) => match stage_write(&mut file, bytes) {
                Ok(()) => return Ok(temporary),
                Err(error) => {
                    drop(file);
                    let _ = fs::remove_file(&temporary);
                    return Err(error);
                }
            },
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "staging name allocation exhausted",
    ))
}

/// Best-effort removal of staged siblings that will never be committed; callers keep the original error.
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

    /// Exclusive staging never opens an existing path: a band of upcoming predictable staging
    /// names planted as symlinks to an external canary is skipped, the write lands on a fresh
    /// regular sibling, declared outputs are regular files with the tree's bytes, and the canary
    /// is untouched. The band is re-planted from the live counter so parallel-test drift cannot
    /// miss the collision window, then the counter is rewound into that band so the retry path
    /// is actually exercised.
    #[test]
    #[cfg(unix)]
    fn exclusive_staging_skips_planted_symlinks_and_preserves_canary() {
        let temp = Temp::new();
        let canary = temp.0.join("canary-outside");
        fs::write(&canary, b"canary-intact\n").unwrap();
        let parents = [
            "generated",
            "generated/adapter",
            "generated/contract",
            "generated/contract/src",
        ];
        for parent in parents {
            fs::create_dir_all(temp.0.join(parent)).unwrap();
        }
        let pid = std::process::id();
        let plant = |base: u64| {
            for parent in parents {
                for n in 0..32u64 {
                    let name = format!(".boxology-write-{pid}-{}", base + n);
                    let link = temp.0.join(parent).join(name);
                    let _ = std::os::unix::fs::symlink(&canary, &link);
                }
            }
        };
        let start = NEXT.load(Ordering::Relaxed);
        plant(start);
        // Absorb counter drift from parallel tests in this process, then force the retry path.
        let now = NEXT.load(Ordering::Relaxed);
        if now != start {
            plant(now);
        }
        NEXT.store(start, Ordering::Relaxed);
        let tree = tree();
        let changes = write(&temp.0, &tree, &globs(&DECLARED)).unwrap();
        assert_eq!(changes.written, SORTED);
        for file in tree.files() {
            let path = temp.0.join(file.path());
            let meta = fs::symlink_metadata(&path).unwrap();
            assert!(
                meta.file_type().is_file(),
                "{} must be a regular file",
                file.path()
            );
            assert_eq!(fs::read(&path).unwrap(), file.bytes(), "{}", file.path());
        }
        assert_eq!(fs::read(&canary).unwrap(), b"canary-intact\n");
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

    /// Scan-complete rival refusal is order-independent: an exact spelling plus a distinct
    /// ASCII-case rival is refused in either listing order, and a lone exact or absent name is not.
    #[test]
    fn lookup_decision_is_scan_complete_and_order_independent() {
        assert_eq!(
            lookup_decision("schema.json", ["schema.json", "SCHEMA.JSON"]),
            Err(())
        );
        assert_eq!(
            lookup_decision("schema.json", ["SCHEMA.JSON", "schema.json"]),
            Err(())
        );
        assert_eq!(lookup_decision("schema.json", ["SCHEMA.JSON"]), Err(()));
        assert_eq!(lookup_decision("schema.json", ["schema.json"]), Ok(true));
        assert_eq!(lookup_decision("schema.json", ["other.json"]), Ok(false));
        assert_eq!(lookup_decision("schema.json", []), Ok(false));
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

    /// Injected prune-enumeration failure fails closed as `Walk` after commit: the sealed subtree
    /// still holds its stale artifact, every declared file is live, and absent prefixes still skip.
    /// The injectable seam keeps this value-positive on macOS even when root bypasses mode bits.
    #[test]
    fn injected_prune_walk_failure_leaves_the_committed_tree_live() {
        let temp = Temp::new();
        let tree = tree();
        fs::create_dir_all(temp.0.join("generated/legacy")).unwrap();
        fs::write(temp.0.join("generated/legacy/old.rs"), "old\n").unwrap();
        fn sealed_legacy(path: &Path) -> io::Result<Vec<ListedEntry>> {
            if path.ends_with("legacy") {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "sealed subtree",
                ));
            }
            os_list_dir(path)
        }
        let error = write_with(
            &temp.0,
            &tree,
            &globs(&DECLARED),
            sealed_legacy,
            os_stage_write,
        )
        .unwrap_err();
        let blamed = matches!(
            &error,
            WriteError::Walk(path, err)
                if path == "generated/legacy"
                    && err.kind() == io::ErrorKind::PermissionDenied
        );
        assert!(blamed, "{error}");
        for file in tree.files() {
            assert_eq!(
                fs::read(temp.0.join(file.path())).unwrap(),
                file.bytes(),
                "{}",
                file.path()
            );
        }
        assert_eq!(
            fs::read(temp.0.join("generated/legacy/old.rs")).unwrap(),
            b"old\n"
        );
        // Absent prefix (`docs/**` in DECLARED) still skips under the same seam.
        let again = candidates(&temp.0, &globs(&["docs/**"]), sealed_legacy).unwrap();
        assert!(again.is_empty());
    }

    /// Exclusive create then a forced partial write failure best-effort removes that sibling,
    /// returns the injected write error unchanged, and leaves every declared destination
    /// byte-identical.
    #[test]
    fn injected_stage_write_failure_removes_sibling_and_keeps_destinations() {
        let temp = Temp::new();
        let tree = tree();
        fs::create_dir_all(temp.0.join("generated")).unwrap();
        let canary = temp.0.join("generated/schema.json");
        fs::write(&canary, b"canary-intact\n").unwrap();
        fn partial_then_fail(file: &mut fs::File, _bytes: &[u8]) -> io::Result<()> {
            file.write_all(b"partial")?;
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "injected stage write failure",
            ))
        }
        let error = write_with(
            &temp.0,
            &tree,
            &globs(&DECLARED),
            os_list_dir,
            partial_then_fail,
        )
        .unwrap_err();
        let blamed = matches!(
            &error,
            WriteError::Io(path, err)
                if path == SORTED[0]
                    && err.kind() == io::ErrorKind::Interrupted
                    && err.to_string().contains("injected stage write failure")
        );
        assert!(blamed, "{error}");
        let files = listing(&temp.0);
        assert!(
            files.iter().all(|path| !path.contains(".boxology-write-")),
            "staged sibling remained: {files:?}"
        );
        assert_eq!(files, ["generated/schema.json"]);
        assert_eq!(fs::read(&canary).unwrap(), b"canary-intact\n");
    }

    /// Pre-write parent listing must be scan-complete: an enumeration error refuses before any
    /// staging sibling or destination byte changes, rather than treating the component as absent.
    #[test]
    fn injected_lookup_enumeration_failure_refuses_before_any_change() {
        let temp = Temp::new();
        let tree = tree();
        fs::create_dir_all(temp.0.join("generated")).unwrap();
        let canary = temp.0.join("generated/schema.json");
        fs::write(&canary, b"canary-intact\n").unwrap();
        fn hidden_rival_scan(path: &Path) -> io::Result<Vec<ListedEntry>> {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "hidden rival enumeration",
            ))
        }
        let error = write_with(
            &temp.0,
            &tree,
            &globs(&DECLARED),
            hidden_rival_scan,
            os_stage_write,
        )
        .unwrap_err();
        let blamed = matches!(
            &error,
            WriteError::Inspect(path, err)
                if path == SORTED[0]
                    && err.kind() == io::ErrorKind::PermissionDenied
                    && err.to_string().contains("hidden rival enumeration")
        );
        assert!(blamed, "{error}");
        assert_eq!(
            error.to_string(),
            format!("inspect {}: hidden rival enumeration", SORTED[0])
        );
        let files = listing(&temp.0);
        assert!(
            files.iter().all(|path| !path.contains(".boxology-write-")),
            "staged sibling remained: {files:?}"
        );
        assert_eq!(files, ["generated/schema.json"]);
        assert_eq!(fs::read(&canary).unwrap(), b"canary-intact\n");
    }
}
