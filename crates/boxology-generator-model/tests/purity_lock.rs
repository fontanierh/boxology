//! Source-file inventory lock for the generator crates.
//!
//! This lock pins the exact set of files under the two production `src/` trees. It does NOT cover:
//!
//! 1. **`boxology-generator`'s manifest is unguarded.** The registered CI spec names only
//!    `crates/boxology-generator-model/Cargo.toml`, so manifest/build-script checks never inspect
//!    `crates/boxology-generator/Cargo.toml` — even though this lock pins that crate's `src` tree.
//!    A `build.rs` or a `[lib] path = ...` escape outside `src/` is invisible here; those pins
//!    belong to the deferred dependency-graph slice.
//! 2. **`#[path]` escapes the walk.** `#[path = "../hidden.rs"] mod hidden;` compiles and is
//!    invisible to a filesystem walk. Rejecting `#[path]` belongs to the deferred source-closure /
//!    AST slice.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const SOURCES: [&str; 6] = [
    "crates/boxology-generator-model/src/imports.rs",
    "crates/boxology-generator-model/src/lib.rs",
    "crates/boxology-generator-model/src/manifest.rs",
    "crates/boxology-generator-model/src/rust.rs",
    "crates/boxology-generator/src/lib.rs",
    "crates/boxology-generator/src/schema.rs",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn collect_sources(root: &Path, directory: &Path, found: &mut BTreeSet<String>) {
    let metadata = fs::symlink_metadata(directory).expect("source directory metadata");
    // symlink_metadata never reports is_dir() for a symlink, so this rejects symlink roots too.
    assert!(
        metadata.is_dir(),
        "locked source path is not a directory: {}",
        directory.display()
    );
    for entry in fs::read_dir(directory).expect("source directory") {
        let path = entry.expect("source entry").path();
        let metadata = fs::symlink_metadata(&path).expect("source entry metadata");
        assert!(
            !metadata.file_type().is_symlink(),
            "symlink under a locked source directory: {}",
            path.display()
        );
        if metadata.is_dir() {
            collect_sources(root, &path, found);
        } else {
            assert!(
                path.extension().is_some_and(|extension| extension == "rs"),
                "non-Rust file under a locked source directory: {}",
                path.display()
            );
            found.insert(
                path.strip_prefix(root)
                    .expect("source is in workspace")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

#[test]
fn production_source_inventory_is_exact() {
    let root = root();
    let mut found = BTreeSet::new();
    for directory in [
        root.join("crates/boxology-generator-model/src"),
        root.join("crates/boxology-generator/src"),
    ] {
        collect_sources(&root, &directory, &mut found);
    }
    assert_eq!(
        found,
        SOURCES.iter().map(|name| (*name).to_owned()).collect()
    );
    for relative in SOURCES {
        assert!(
            fs::symlink_metadata(root.join(relative)).is_ok_and(|metadata| metadata.is_file()),
            "missing production source: {relative}"
        );
    }
}
