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
//! A staged two-phase commit, the shape `xtask`'s determinism publication already uses.
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
//!
//! **It does not prune, a stated gap rather than a policy.** The maintainer has ruled that
//! `generate` *prunes*: a file under a declared `[[derived]].outputs` glob the tree no longer
//! declares is deleted, because S5 D6 step 2 promises `boxology generate --package <id>` repairs a
//! stale artifact, and a repair leaving the orphan never clears the finding. That needs the
//! manifest's globs, a walk bounded to them, and its own commit-ordering argument, so it is the
//! next slice; until then this writer only adds and replaces.
//!
//! **There is no traversal guard: confinement rests on `generate` building paths from literals.**
//! It zips the `OUTPUTS` constants with the emitted bodies, so a tree's paths are those four
//! literals whatever the request declared — `require_exact_outputs` never feeds them — and
//! [`GeneratedTree`] has a private field and no constructor. Nothing here inspects a component for
//! `..`, and `resolve` returns a path above `root` if handed one. **A public `GeneratedTree`
//! constructor must add a component guard in the same change: it is the only thing standing
//! between one and a write outside the destination root.**
//!
//! A [`WriteError`] renders only the tree's own logical paths and `&'static str` this crate chose.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_generator::GeneratedTree;
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
}

impl fmt::Display for WriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Destination => formatter.write_str("destination root is not a directory"),
            Self::Aliased(path) => write!(formatter, "spelled another way on disk: {path}"),
            Self::Ancestor(path) => write!(formatter, "a parent is not a directory: {path}"),
            Self::Occupied(path) => write!(formatter, "not a regular file: {path}"),
            Self::Io(path, error) => write!(formatter, "write {path}: {error}"),
        }
    }
}

impl std::error::Error for WriteError {}

/// Writes every file `tree` declares under `root`, staging the whole tree before committing it.
///
/// Returns the logical paths created or replaced, in the tree's own order; empty means every
/// declared file already held the tree's exact bytes and nothing was modified.
///
/// # Errors
/// Returns [`WriteError`] when `root` is not an existing directory, an existing entry blocks or
/// misspells a declared path, or the filesystem refuses an operation.
pub fn write(root: &Path, tree: &GeneratedTree) -> Result<Vec<String>, WriteError> {
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
    let mut changed = Vec::new();
    for (index, ((path, target, _), temporary)) in plan.iter().zip(&staged).enumerate() {
        if let Err(error) = fs::rename(temporary, target) {
            discard(&staged[index..]);
            return Err(WriteError::Io((*path).to_owned(), error));
        }
        changed.push((*path).to_owned());
    }
    Ok(changed)
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
        assert_eq!(write(&temp.0, &tree).unwrap(), SORTED);
        for file in tree.files() {
            let written = fs::read(temp.0.join(file.path())).unwrap();
            assert_eq!(written, file.bytes(), "{}", file.path());
        }
        let survivor = fs::read_to_string(temp.0.join("generated/stale.json"));
        assert_eq!(survivor.unwrap(), "stale\n", "an undeclared file survives");
        let mut expected = SORTED.to_vec();
        expected.push("generated/stale.json"); // already sorts last: `sc` < `st`
        assert_eq!(listing(&temp.0), expected);
    }

    #[test]
    fn a_repeated_write_touches_only_the_file_whose_bytes_drifted() {
        let temp = Temp::new();
        let tree = tree();
        write(&temp.0, &tree).unwrap();
        let before = stamps(&temp.0);
        let again = write(&temp.0, &tree).unwrap();
        assert_eq!(again, Vec::<String>::new());
        assert_eq!(listing(&temp.0), SORTED, "a skipped file stages no sibling");
        assert_eq!(stamps(&temp.0), before, "an unchanged tree stays untouched");
        fs::write(temp.0.join(SORTED[3]), b"drift\n").unwrap();
        assert_eq!(write(&temp.0, &tree).unwrap(), [SORTED[3]]);
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
        let error = write(&temp.0, &tree()).unwrap_err();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700)).unwrap();
        let blamed = matches!(&error, WriteError::Io(p, _) if p == SORTED[2]);
        assert!(blamed, "{error}");
        assert_eq!(listing(&temp.0), Vec::<String>::new());
    }

    /// Every refusal reachable through the public API, asserting the exact surviving listing —
    /// identical on a case-sensitive and a case-insensitive filesystem.
    #[test]
    #[cfg(unix)]
    fn every_refusal_through_write_leaves_the_destination_untouched() {
        let temp = Temp::new();
        fs::write(temp.0.join("occupied"), b"x").unwrap();
        for name in ["occupied", "absent"] {
            let refused = write(&temp.0.join(name), &tree()).unwrap_err();
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
            let refusal = match write(&temp.0, &tree()) {
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
}
