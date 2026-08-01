use boxology_init::confined_destination;
use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Staged-write failure; the CLI maps it onto the staged-write diagnostic.
#[derive(Debug, Eq, PartialEq)]
pub struct WriteFailure {
    /// Logical path or top-level entry named in the diagnostic payload.
    pub path: String,
}

/// Writes `files` into `target` via same-filesystem staging and ordered renames.
pub fn write_tree(target: &Path, files: &[(&str, &[u8])]) -> Result<(), WriteFailure> {
    write_tree_with(target, files, |from, to| fs::rename(from, to))
}

/// Like [`write_tree`] with an injectable commit `rename`.
pub fn write_tree_with(
    target: &Path,
    files: &[(&str, &[u8])],
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), WriteFailure> {
    for &(logical, _) in files {
        confined_destination(target, logical).map_err(|_| fail(logical))?;
    }
    let staging = target.join(format!(
        ".boxology-init-staging-{}-{}",
        std::process::id(),
        STAGING_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(error) = stage_files(&staging, files) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    for entry in &commit_order(files) {
        rename(&staging.join(entry), &target.join(entry)).map_err(|_| fail(entry))?;
    }
    fs::remove_dir_all(&staging).map_err(|_| {
        fail(
            staging
                .file_name()
                .expect("staging path has a file name")
                .to_str()
                .expect("staging file name is UTF-8"),
        )
    })
}

fn fail(path: &str) -> WriteFailure {
    WriteFailure {
        path: path.to_owned(),
    }
}

fn stage_files(staging: &Path, files: &[(&str, &[u8])]) -> Result<(), WriteFailure> {
    fs::create_dir(staging)
        .map_err(|_| fail(files.first().map(|(p, _)| *p).unwrap_or_default()))?;
    for &(logical, bytes) in files {
        let dest = confined_destination(staging, logical).expect("paths were pre-checked");
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|_| fail(logical))?;
        }
        fs::write(&dest, bytes).map_err(|_| fail(logical))?;
    }
    Ok(())
}

fn commit_order(files: &[(&str, &[u8])]) -> Vec<String> {
    let mut entries = Vec::new();
    for &(path, _) in files {
        let top = path.split('/').next().expect("split yields a segment");
        if !entries.iter().any(|entry: &String| entry == top) {
            entries.push(top.to_owned());
        }
    }
    entries.sort();
    let position = entries
        .iter()
        .position(|entry| entry == "boxology.toml")
        .expect("generated tree must contain boxology.toml exactly once");
    let sentinel = entries.remove(position);
    entries.push(sentinel);
    entries
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    use std::{path::PathBuf, sync::atomic::AtomicUsize};

    const SAMPLE: [(&str, &[u8]); 6] = [
        (".gitignore", b"g"), ("Cargo.toml", b"c"), ("boxology-generator.toml", b"bg"),
        ("boxology.toml", b"sentinel"), ("ping/x", b"p"), ("rust-toolchain.toml", b"r"),
    ];
    const ORDER: [&str; 6] = [
        ".gitignore", "Cargo.toml", "boxology-generator.toml", "ping", "rust-toolchain.toml", "boxology.toml",
    ];

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "boxology-init-write-{}-{}", std::process::id(), TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn listing(&self) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(&self.0).unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            names
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn commit_order_rotates_sentinel_last() {
        assert_eq!(commit_order(&SAMPLE), ORDER);
    }

    #[test]
    fn write_tree_roundtrip_lists_top_level_entries() {
        let temp = Temp::new();
        write_tree(&temp.0, &SAMPLE).unwrap();
        let mut expected = ORDER.to_vec();
        expected.sort();
        assert_eq!(temp.listing(), expected);
        assert_eq!(fs::read(temp.0.join("ping/x")).unwrap(), b"p");
    }

    #[test]
    fn confined_escape_pair_is_rejected_and_target_untouched() {
        let temp = Temp::new();
        fs::write(temp.0.join("keep"), b"k").unwrap();
        assert_eq!(write_tree(&temp.0, &[("../escape", b"x")]).unwrap_err().path, "../escape");
        assert_eq!(temp.listing(), ["keep"]);
    }

    #[test]
    fn interruption_after_fourth_rename_leaves_no_sentinel() {
        let temp = Temp::new();
        let calls = AtomicUsize::new(0);
        let error = write_tree_with(&temp.0, &SAMPLE, |from, to| {
            if calls.fetch_add(1, Ordering::Relaxed) >= 4 { return Err(io::Error::other("injected")); }
            fs::rename(from, to)
        }).unwrap_err();
        assert_eq!(error.path, "rust-toolchain.toml");
        let listing = temp.listing();
        assert!(listing.iter().any(|n| n == "ping"));
        assert!(!listing.iter().any(|n| n == "boxology.toml"));
        assert!(!listing.iter().any(|n| n == "rust-toolchain.toml"));
        let staging = listing.iter().find(|n| n.starts_with(".boxology-init-staging-")).unwrap();
        assert_eq!(temp.0.join(staging).parent(), Some(temp.0.as_path()));
    }

    #[test]
    fn staging_failure_removes_staging_residue() {
        let temp = Temp::new();
        let staging = temp.0.join(format!(
            ".boxology-init-staging-{}-{}",
            std::process::id(),
            STAGING_COUNTER.load(Ordering::Relaxed)
        ));
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join("ping"), b"blocker").unwrap();
        assert!(
            fs::create_dir(&staging).is_err(),
            "pre-seeded staging path must reject create_dir"
        );
        let error = write_tree(&temp.0, &SAMPLE).unwrap_err();
        assert_eq!(error.path, SAMPLE[0].0);
        assert!(
            temp.listing().iter().all(|n| !n.starts_with(".boxology-init-staging-")),
            "{:?}",
            temp.listing()
        );
    }
}
