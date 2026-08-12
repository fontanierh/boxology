use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::{fs, path::Path};
use toml_edit::{DocumentMut, Item, TableLike};

const REPOSITORY: &str = "https://github.com/fontanierh/boxology";
const APACHE_SHA256: &str = "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30";
const MIT_SHA256: &str = "c6c7cd67105c326b5aa22c9285cf80e54f654e5203891fe1f0c65c567da5dbcb";

#[rustfmt::skip]
const PRODUCTS: &[(&str, &str, &str)] = &[
    ("crates/boxology", "boxology", "Authoring facade"),
    ("crates/boxology-classifier", "boxology-classifier", "Compatibility classifier"),
    ("crates/boxology-cli", "boxology-cli", "Command-line interface"),
    ("crates/boxology-cli-core", "boxology-cli-core", "Reusable command-line boundary"),
    ("crates/boxology-contract", "boxology-contract", "Core identifiers, descriptors, and call types"),
    ("crates/boxology-contract-syntax", "boxology-contract-syntax", "Controlled contract parser and model"),
    ("crates/boxology-generator", "boxology-generator", "Deterministic generator"),
    ("crates/boxology-generator-model", "boxology-generator-model", "Pure generation planning"),
    ("crates/boxology-generator-writer", "boxology-generator-writer", "Filesystem publication"),
    ("crates/boxology-http", "boxology-http", "HTTP transport"),
    ("crates/boxology-init", "boxology-init", "Project initializer"),
    ("crates/boxology-macros", "boxology-macros", "Procedural macros"),
    ("crates/boxology-manifest", "boxology-manifest", "Strict manifest parser and model"),
    ("crates/boxology-runtime", "boxology-runtime", "Runtime composition and invocation"),
    ("crates/boxology-schema", "boxology-schema", "Canonical schema model and serializer"),
    ("crates/boxology-workspace", "boxology-workspace", "Pure workspace validation"),
];
#[rustfmt::skip]
const INTERNAL: &[(&str, &str)] = &[
    ("crates/boxology-classifier/generated/contract", "classifier-contract"),
    ("crates/boxology-classifier/implementation", "classifier-implementation"),
    ("crates/boxology-check/generated/contract", "check-contract"),
    ("crates/boxology-check/implementation", "check-implementation"),
    ("crates/boxology-http-conformance", "boxology-http-conformance"),
    ("crates/fixtures/fixture-tests", "boxology-fixture-tests"),
    ("crates/xtask", "xtask"),
];

#[rustfmt::skip]
pub(crate) fn run(root: &Path) -> u8 {
    match check(root) {
        Ok(()) => 0,
        Err(errors) => { for error in errors { eprintln!("oss-metadata: {error}"); } 1 }
    }
}

#[rustfmt::skip]
fn document(root: &Path, relative: &str) -> Result<DocumentMut, String> {
    fs::read_to_string(root.join(relative)).map_err(|e| format!("read {relative}: {e}"))?
        .parse().map_err(|e| format!("parse {relative}: {e}"))
}

fn string<'a>(table: &'a dyn TableLike, key: &str) -> Option<&'a str> {
    table.get(key)?.as_str()
}

#[rustfmt::skip]
fn inherited(table: &dyn TableLike, key: &str) -> bool {
    table.get(key).and_then(Item::as_table_like)
        .and_then(|value| value.get("workspace")).and_then(Item::as_bool) == Some(true)
}

#[rustfmt::skip]
fn check_readme(readme: &str, errors: &mut Vec<String>) {
    for truth in ["early-stage, source-only framework", "V0 was completed on 2026-08-09",
        "Applications built with Boxology are separate products and are not included in this repository.",
        "version `0.0.0` with `publish = false`", "Install only from a source checkout.",
        "requires Rust 1.97.1", "dual-licensed under [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE)"] {
        if !readme.contains(truth) { errors.push(format!("README.md: missing required truth {truth:?}")); }
    }
    for stale in ["published on crates.io", "cargo install boxology", "committed flagship application", "current product critical path"] {
        if readme.contains(stale) { errors.push(format!("README.md: stale claim {stale:?}")); }
    }
}

#[rustfmt::skip]
fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let root_doc = document(root, "Cargo.toml").map_err(|e| vec![e])?;
    let Some(workspace) = root_doc.get("workspace").and_then(Item::as_table_like)
        else { return Err(vec!["Cargo.toml: missing [workspace]".into()]); };
    let Some(package) = workspace.get("package").and_then(Item::as_table_like)
        else { return Err(vec!["Cargo.toml: missing [workspace.package]".into()]); };
    let expected = [("version", "0.0.0"), ("edition", "2024"), ("rust-version", "1.97.1"),
        ("license", "MIT OR Apache-2.0"), ("repository", REPOSITORY),
        ("homepage", REPOSITORY), ("readme", "README.md")];
    let keys: BTreeSet<_> = package.iter().map(|(key, _)| key).collect();
    let expected_keys: BTreeSet<_> = expected.iter().map(|(key, _)| *key).collect();
    if keys != expected_keys { errors.push("Cargo.toml: [workspace.package] keys are not exact".into()); }
    for (key, value) in expected {
        if string(package, key) != Some(value) { errors.push(format!("Cargo.toml: workspace {key} must be {value:?}")); }
    }
    let members: BTreeSet<_> = workspace.get("members").and_then(Item::as_array)
        .map(|items| items.iter().filter_map(|item| item.as_str()).collect())
        .unwrap_or_default();
    let classified: BTreeSet<_> = PRODUCTS.iter().map(|(path, _, _)| *path)
        .chain(INTERNAL.iter().map(|(path, _)| *path))
        .collect();
    if members != classified { errors.push("Cargo.toml: workspace members are not exactly classified as product or internal".into()); }

    for &(path, name, description) in PRODUCTS {
        match document(root, &format!("{path}/Cargo.toml")) {
            Err(error) => errors.push(error),
            Ok(doc) => {
                let Some(pkg) = doc.get("package").and_then(Item::as_table_like)
                    else { errors.push(format!("{path}: missing [package]")); continue; };
                let keys: BTreeSet<_> = pkg.iter().map(|(key, _)| key).collect();
                let expected: BTreeSet<_> = ["name", "description", "version", "edition", "rust-version", "license", "repository", "homepage", "readme", "publish"].into_iter().collect();
                if keys != expected { errors.push(format!("{path}: [package] keys are not exact")); }
                if string(pkg, "name") != Some(name) { errors.push(format!("{path}: package name must be {name}")); }
                if string(pkg, "description") != Some(description) { errors.push(format!("{path}: description must be {description:?}")); }
                for key in ["version", "edition", "rust-version", "license", "repository", "homepage", "readme"] {
                    if !inherited(pkg, key) { errors.push(format!("{path}: {key} must inherit workspace metadata")); }
                }
                if pkg.get("publish").and_then(Item::as_bool) != Some(false) { errors.push(format!("{path}: publish must be false")); }
                if pkg.get("authors").is_some() { errors.push(format!("{path}: authors are forbidden")); }
            }
        }
    }
    for &(path, name) in INTERNAL {
        match document(root, &format!("{path}/Cargo.toml")) {
            Err(error) => errors.push(error),
            Ok(doc) => {
                let Some(pkg) = doc.get("package").and_then(Item::as_table_like)
                    else { errors.push(format!("{path}: missing [package]")); continue; };
                if string(pkg, "name") != Some(name) { errors.push(format!("{path}: internal package name must be {name}")); }
                if string(pkg, "version") != Some("0.0.0") { errors.push(format!("{path}: internal version must be 0.0.0")); }
                if pkg.get("publish").and_then(Item::as_bool) != Some(false) { errors.push(format!("{path}: internal publish must be false")); }
            }
        }
    }
    for (file, digest) in [("LICENSE-APACHE", APACHE_SHA256), ("LICENSE-MIT", MIT_SHA256)] {
        match fs::read(root.join(file)) {
            Ok(bytes) if format!("{:x}", Sha256::digest(&bytes)) == digest => {}
            Ok(_) => errors.push(format!("{file}: bytes are not canonical")),
            Err(error) => errors.push(format!("read {file}: {error}")),
        }
    }
    match fs::read_to_string(root.join("README.md")) {
        Err(error) => errors.push(format!("read README.md: {error}")),
        Ok(readme) => check_readme(&readme, &mut errors),
    }
    errors.is_empty().then_some(()).ok_or(errors)
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*; use std::path::PathBuf;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("boxology-oss-metadata-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path); fs::create_dir(&path).unwrap();
            let live = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            for file in ["Cargo.toml", "README.md", "LICENSE-MIT", "LICENSE-APACHE"]
                .into_iter().chain(PRODUCTS.iter().map(|(p, _, _)| *p)).chain(INTERNAL.iter().map(|(p, _)| *p)) {
                let relative = if file.starts_with("crates/") { format!("{file}/Cargo.toml") } else { file.into() };
                let target = path.join(&relative); fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::copy(live.join(&relative), target).unwrap();
            }
            Self(path)
        }
        fn mutate(&self, file: &str, from: &str, to: &str) {
            let path = self.0.join(file); let bytes = fs::read_to_string(&path).unwrap();
            assert!(bytes.contains(from)); fs::write(path, bytes.replacen(from, to, 1)).unwrap();
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test]
    fn live_metadata_passes_and_required_mutations_fail_closed() {
        assert_eq!(run(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")), 0);
        for (file, from, to) in [
            ("crates/boxology/Cargo.toml", "publish = false", "publish = true"),
            ("crates/boxology/Cargo.toml", "publish = false", "publish = false\nlicense-file = \"LICENSE\""),
            ("Cargo.toml", "MIT OR Apache-2.0", "MIT"),
            ("crates/boxology/Cargo.toml", "Authoring facade", "Facade"),
            ("Cargo.toml", REPOSITORY, "https://example.invalid/boxology"),
            ("Cargo.toml", "\"crates/xtask\",", "\"crates/xtask\",\n    \"crates/unknown\","),
            ("crates/xtask/Cargo.toml", "publish = false", "publish = true"),
            ("crates/xtask/Cargo.toml", "version = \"0.0.0\"", "version = \"1.2.3\""),
            ("LICENSE-MIT", "Henry Fontanier", "H. Fontanier"),
            ("README.md", "Install only from a source checkout.", "Boxology is published on crates.io."),
        ] {
            let fixture = Fixture::new(); fixture.mutate(file, from, to);
            assert_eq!(run(&fixture.0), 1, "mutation survived: {file} {from}");
        }
    }
}
