//! Own-source directory/manifest/test-tree closure for the generator crates (#107A slice A).
//! AST/effect scanning is later; transitive purity remains #358.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use toml_edit::{DocumentMut, Item};

const SOURCES: &[&str] = &[
    "crates/boxology-generator-model/src/imports.rs",
    "crates/boxology-generator-model/src/lib.rs",
    "crates/boxology-generator-model/src/manifest.rs",
    "crates/boxology-generator-model/src/rust.rs",
    "crates/boxology-generator/src/lib.rs",
    "crates/boxology-generator/src/schema.rs",
];
const GENERATOR_TESTS: &[&str] = &[
    "crates/boxology-generator/tests/generation_purity.rs",
    "crates/boxology-generator/tests/implementation_conformance.rs",
];
const MODEL_TESTS: &[&str] = &["crates/boxology-generator-model/tests/purity_lock.rs"];
const CRATES: &[&str] = &[
    "crates/boxology-generator",
    "crates/boxology-generator-model",
];
const ROOT_ENTRIES: &[&str] = &["Cargo.toml", "src", "tests"];
const PACKAGE_KEYS: &[&str] = &["edition", "name", "publish", "version"];
const AUTO_FLAGS: &[&str] = &[
    "autolib",
    "autobins",
    "autoexamples",
    "autotests",
    "autobenches",
];
const CUSTOM_TARGETS: &[&str] = &["lib", "bin", "example", "test", "bench"];

#[derive(Clone, Copy)]
enum DepValue {
    Ver(&'static str),
    Path {
        version: &'static str,
        path: &'static str,
    },
    Feat {
        version: &'static str,
        default_features: bool,
        features: &'static [&'static str],
    },
}

#[derive(Clone, Copy)]
struct Dep {
    key: &'static str,
    value: DepValue,
}

const fn path_dep(key: &'static str, path: &'static str) -> Dep {
    Dep {
        key,
        value: DepValue::Path {
            version: "=0.0.0",
            path,
        },
    }
}
const fn ver(key: &'static str, version: &'static str) -> Dep {
    Dep {
        key,
        value: DepValue::Ver(version),
    }
}
const fn feat(
    key: &'static str,
    version: &'static str,
    default_features: bool,
    features: &'static [&'static str],
) -> Dep {
    Dep {
        key,
        value: DepValue::Feat {
            version,
            default_features,
            features,
        },
    }
}

const GEN_DEPS: &[Dep] = &[
    path_dep("boxology-contract", "../boxology-contract"),
    path_dep("boxology-contract-syntax", "../boxology-contract-syntax"),
    path_dep("boxology-generator-model", "../boxology-generator-model"),
    path_dep("boxology-schema", "../boxology-schema"),
    ver("prettyplease", "=0.3.0"),
    ver("serde_json", "=1.0.150"),
    ver("sha2", "=0.10.9"),
    feat("syn", "=3.0.0", false, &["full", "parsing"]),
];
const GEN_DEV: &[Dep] = &[path_dep("boxology-classifier", "../boxology-classifier")];
const MODEL_DEPS: &[Dep] = &[
    path_dep("boxology-contract", "../boxology-contract"),
    path_dep("boxology-contract-syntax", "../boxology-contract-syntax"),
    feat("proc-macro2", "=1.0.107", false, &["span-locations"]),
    ver("serde_json", "=1.0.150"),
    feat("syn", "=3.0.0", false, &["full", "parsing", "visit"]),
    ver("toml_edit", "=0.25.13+spec-1.1.0"),
];
const MODEL_DEV: &[Dep] = &[];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

fn keys(deps: &[Dep]) -> BTreeSet<String> {
    deps.iter().map(|d| d.key.to_owned()).collect()
}

fn collect_rust(root: &Path, dir: &Path, found: &mut BTreeSet<String>) -> Result<(), String> {
    let meta = fs::symlink_metadata(dir).map_err(|e| format!("walk: metadata: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("walk: not a plain directory: {}", dir.display()));
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("walk: read_dir: {e}"))? {
        let path = entry.map_err(|e| format!("walk: entry: {e}"))?.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| format!("walk: entry meta: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "test_symlink: symlink not allowed: {}",
                path.display()
            ));
        }
        if meta.is_dir() {
            collect_rust(root, &path, found)?;
        } else if meta.is_file() {
            if path.extension().is_none_or(|ext| ext != "rs") {
                return Err(format!("walk: non-Rust file: {}", path.display()));
            }
            found.insert(
                path.strip_prefix(root)
                    .map_err(|_| "walk: path escapes root".to_owned())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        } else {
            return Err(format!("walk: unsupported entry: {}", path.display()));
        }
    }
    Ok(())
}

fn assert_inventory(root: &Path, dirs: &[&str], expected: &[&str]) {
    let mut found = BTreeSet::new();
    for dir in dirs {
        collect_rust(root, &root.join(dir), &mut found).unwrap_or_else(|e| panic!("{e}"));
    }
    assert_eq!(found, expected.iter().map(|s| (*s).to_owned()).collect());
    for rel in expected {
        assert!(
            fs::symlink_metadata(root.join(rel)).is_ok_and(|m| m.is_file()),
            "missing locked file: {rel}"
        );
    }
}

fn assert_root_closed(crate_dir: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(crate_dir)
        .map_err(|e| format!("root_entry: missing {}: {e}", crate_dir.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "root_entry: not a plain directory: {}",
            crate_dir.display()
        ));
    }
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(crate_dir).map_err(|e| format!("root_entry: read_dir: {e}"))? {
        let entry = entry.map_err(|e| format!("root_entry: entry: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|e| format!("root_entry: metadata {name}: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err(format!("root_entry: symlink not allowed: {name}"));
        }
        match name.as_str() {
            "Cargo.toml" if meta.is_file() => {}
            "src" | "tests" if meta.is_dir() => {}
            _ => return Err(format!("root_entry: disallowed crate root entry: {name}")),
        }
        names.insert(name);
    }
    let allowed = set(ROOT_ENTRIES);
    if names != allowed {
        return Err(format!("root_entry: expected {allowed:?}, found {names:?}"));
    }
    Ok(())
}

fn tbl_str(table: &dyn toml_edit::TableLike, key: &str) -> Result<String, String> {
    table
        .get(key)
        .and_then(Item::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("dep_value: missing string `{key}`"))
}

fn tbl_bool(table: &dyn toml_edit::TableLike, key: &str) -> Result<bool, String> {
    table
        .get(key)
        .and_then(Item::as_bool)
        .ok_or_else(|| format!("dep_value: missing bool `{key}`"))
}

fn tbl_strs(table: &dyn toml_edit::TableLike, key: &str) -> Result<Vec<String>, String> {
    let array = table
        .get(key)
        .and_then(Item::as_array)
        .ok_or_else(|| format!("dep_value: missing array `{key}`"))?;
    array
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("dep_value: `{key}` entries must be strings"))
        })
        .collect()
}

fn assert_dep_value(key: &str, item: &Item, expected: DepValue) -> Result<(), String> {
    if let Some(table) = item.as_table_like() {
        if table.get("package").is_some() {
            return Err(format!("dep_alias: `{key}` must not set package = ..."));
        }
    }
    match expected {
        DepValue::Ver(version) => match item.as_str() {
            Some(got) if got == version => Ok(()),
            _ => Err(format!("dep_value: `{key}` must be exactly {version:?}")),
        },
        DepValue::Path { version, path } => {
            let table = item
                .as_table_like()
                .ok_or_else(|| format!("dep_value: `{key}` must be a table"))?;
            let got = table
                .iter()
                .map(|(n, _)| n.to_owned())
                .collect::<BTreeSet<_>>();
            let allowed = set(&["path", "version"]);
            if got != allowed {
                return Err(format!("dep_alias: `{key}` keys {got:?} != {allowed:?}"));
            }
            if tbl_str(table, "version")? != version || tbl_str(table, "path")? != path {
                return Err(format!("dep_value: `{key}` path pin mismatch"));
            }
            Ok(())
        }
        DepValue::Feat {
            version,
            default_features,
            features,
        } => {
            let table = item
                .as_table_like()
                .ok_or_else(|| format!("dep_value: `{key}` must be a table"))?;
            let got = table
                .iter()
                .map(|(n, _)| n.to_owned())
                .collect::<BTreeSet<_>>();
            let allowed = set(&["default-features", "features", "version"]);
            if got != allowed {
                return Err(format!("dep_alias: `{key}` keys {got:?} != {allowed:?}"));
            }
            let want: Vec<String> = features.iter().map(|f| (*f).to_owned()).collect();
            if tbl_str(table, "version")? != version
                || tbl_bool(table, "default-features")? != default_features
                || tbl_strs(table, "features")? != want
            {
                return Err(format!("dep_value: `{key}` feature pin mismatch"));
            }
            Ok(())
        }
    }
}

fn assert_deps(label: &str, item: Option<&Item>, pins: &[Dep]) -> Result<(), String> {
    if pins.is_empty() {
        return match item {
            None => Ok(()),
            Some(_) => Err(format!("{label}: table must be absent")),
        };
    }
    let table = item
        .and_then(Item::as_table_like)
        .ok_or_else(|| format!("{label}: missing dependency table"))?;
    let found: BTreeSet<String> = table
        .iter()
        .filter(|(_, item)| !item.is_none())
        .map(|(key, _)| key.to_owned())
        .collect();
    let expected = keys(pins);
    if found != expected {
        return Err(format!(
            "unexpected_deps: {label} expected {expected:?}, found {found:?}"
        ));
    }
    for pin in pins {
        let item = table
            .get(pin.key)
            .ok_or_else(|| format!("unexpected_deps: missing `{}`", pin.key))?;
        assert_dep_value(pin.key, item, pin.value)?;
    }
    Ok(())
}

fn assert_manifest_closed(
    manifest: &str,
    package_name: &str,
    deps: &[Dep],
    dev_deps: &[Dep],
) -> Result<(), String> {
    let doc = manifest
        .parse::<DocumentMut>()
        .map_err(|e| format!("manifest_parse: {e}"))?;
    if doc.get("target").is_some() {
        return Err("target_deps: target-specific dependency tables are forbidden".into());
    }
    if doc.get("build-dependencies").is_some() {
        return Err("build_dependencies: [build-dependencies] is forbidden".into());
    }
    for target in CUSTOM_TARGETS {
        if doc.get(target).is_some() {
            return Err(format!("custom_target: [{target}]/[[{target}]] forbidden"));
        }
    }
    let mut allowed_top = set(&["dependencies", "package"]);
    if !dev_deps.is_empty() {
        allowed_top.insert("dev-dependencies".into());
    }
    let top: BTreeSet<String> = doc
        .iter()
        .filter(|(_, item)| !item.is_none())
        .map(|(key, _)| key.to_owned())
        .collect();
    if top != allowed_top {
        return Err(format!(
            "manifest_top: expected {allowed_top:?}, found {top:?}"
        ));
    }

    let package = doc
        .get("package")
        .and_then(Item::as_table)
        .ok_or_else(|| "manifest_package: missing [package]".to_owned())?;
    if package.contains_key("build") {
        return Err("package_build: package.build is forbidden".into());
    }
    if package.contains_key("links") {
        return Err("package_links: package.links is forbidden".into());
    }
    for flag in AUTO_FLAGS {
        if package.contains_key(flag) {
            return Err(format!("auto_target: package.{flag} override forbidden"));
        }
    }
    let package_keys: BTreeSet<String> = package
        .iter()
        .filter(|(_, item)| !item.is_none())
        .map(|(key, _)| key.to_owned())
        .collect();
    let expected_package = set(PACKAGE_KEYS);
    if package_keys != expected_package {
        return Err(format!(
            "package_keys: expected {expected_package:?}, found {package_keys:?}"
        ));
    }
    if package.get("name").and_then(Item::as_str) != Some(package_name) {
        return Err(format!("package_name: expected {package_name}"));
    }

    assert_deps("dependencies", doc.get("dependencies"), deps)?;
    assert_deps("dev-dependencies", doc.get("dev-dependencies"), dev_deps)?;
    Ok(())
}

fn assert_tests_closed(root: &Path, crate_rel: &str, expected: &[&str]) -> Result<(), String> {
    let tests_dir = root.join(crate_rel).join("tests");
    let mut found = BTreeSet::new();
    collect_rust(root, &tests_dir, &mut found).map_err(|e| {
        if e.starts_with("test_symlink:") {
            e
        } else {
            format!("test_tree: {e}")
        }
    })?;
    let want = expected
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<BTreeSet<_>>();
    if found != want {
        return Err(format!(
            "test_inventory: expected {want:?}, found {found:?}"
        ));
    }
    Ok(())
}

fn expect_err(result: Result<(), String>, rule: &str) {
    let err = result.expect_err(&format!("`{rule}` must fail"));
    assert!(err.contains(rule), "`{rule}` missing from error: {err}");
}

fn temp(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "boxology-purity-{label}-{}-{nanos}",
        std::process::id()
    ))
}

#[test]
fn production_source_inventory_is_exact() {
    assert_inventory(
        &root(),
        &[
            "crates/boxology-generator-model/src",
            "crates/boxology-generator/src",
        ],
        SOURCES,
    );
}

#[test]
fn generator_test_trees_are_closed_and_inventoried() {
    let root = root();
    assert_tests_closed(&root, "crates/boxology-generator", GENERATOR_TESTS)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_tests_closed(&root, "crates/boxology-generator-model", MODEL_TESTS)
        .unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn generator_crate_roots_admit_only_manifest_src_and_tests() {
    let root = root();
    for rel in CRATES {
        assert_root_closed(&root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    }
}

#[test]
fn generator_manifests_are_closed_and_pin_exact_dependencies() {
    let root = root();
    for (rel, name, deps, dev) in [
        (
            "crates/boxology-generator/Cargo.toml",
            "boxology-generator",
            GEN_DEPS,
            GEN_DEV,
        ),
        (
            "crates/boxology-generator-model/Cargo.toml",
            "boxology-generator-model",
            MODEL_DEPS,
            MODEL_DEV,
        ),
    ] {
        let text = fs::read_to_string(root.join(rel)).expect("manifest");
        assert_manifest_closed(&text, name, deps, dev).unwrap_or_else(|e| panic!("{rel}: {e}"));
    }
}

fn model_pkg(extra: &str) -> String {
    format!(
        "[package]\nname=\"boxology-generator-model\"\nversion=\"0.0.0\"\nedition=\"2024\"\npublish=false\n{extra}"
    )
}

#[test]
fn closure_rules_reject_live_hostile_corpus() {
    let deps = "[dependencies]\nserde_json=\"=1.0.150\"\n";
    let one = [ver("serde_json", "=1.0.150")];
    let mans = [
        (
            "package_build",
            model_pkg(&format!("build=\"build.rs\"\n{deps}")),
        ),
        ("package_links", model_pkg(&format!("links=\"n\"\n{deps}"))),
        (
            "package_keys",
            model_pkg(&format!("readme=\"r.md\"\n{deps}")),
        ),
        (
            "custom_target",
            model_pkg(&format!("[[example]]\nname=\"x\"\npath=\"e.rs\"\n{deps}")),
        ),
        (
            "custom_target",
            model_pkg(&format!(
                "[[test]]\nname=\"x\"\npath=\"tests/x.rs\"\n{deps}"
            )),
        ),
        (
            "auto_target",
            model_pkg(&format!("autotests=false\n{deps}")),
        ),
        (
            "build_dependencies",
            model_pkg(&format!("{deps}[build-dependencies]\ncc=\"1\"\n")),
        ),
        (
            "target_deps",
            model_pkg(&format!(
                "{deps}[target.'cfg(unix)'.dependencies]\nlibc=\"0.2\"\n"
            )),
        ),
        (
            "dep_alias",
            model_pkg(
                "[dependencies]\nserde_json={ version=\"=1.0.150\", package=\"other-serde\" }\n",
            ),
        ),
        (
            "unexpected_deps",
            model_pkg("[dependencies]\nserde_json=\"=1.0.150\"\nevil=\"1\"\n"),
        ),
        (
            "dep_value",
            model_pkg("[dependencies]\nserde_json=\"=9.9.9\"\n"),
        ),
    ];
    for (rule, manifest) in &mans {
        expect_err(
            assert_manifest_closed(manifest, "boxology-generator-model", &one, &[]),
            rule,
        );
    }
    assert_manifest_closed(&model_pkg(deps), "boxology-generator-model", &one, &[])
        .expect("clean manifest");

    let tmp = temp("root");
    fs::create_dir_all(tmp.join("src")).unwrap();
    fs::create_dir_all(tmp.join("tests")).unwrap();
    fs::write(tmp.join("Cargo.toml"), model_pkg(deps)).unwrap();
    assert_root_closed(&tmp).expect("clean root");
    fs::write(tmp.join("build.rs"), "fn main() {}").unwrap();
    expect_err(assert_root_closed(&tmp), "root_entry");
    fs::remove_dir_all(&tmp).unwrap();

    let tests_tmp = temp("tests");
    let tests_dir = tests_tmp.join("crate/tests");
    fs::create_dir_all(&tests_dir).unwrap();
    fs::write(tests_dir.join("only.rs"), "// ok\n").unwrap();
    let expected = ["crate/tests/only.rs"];
    assert_tests_closed(&tests_tmp, "crate", &expected).expect("clean tests");
    fs::write(tests_dir.join("extra.rs"), "// bad\n").unwrap();
    expect_err(
        assert_tests_closed(&tests_tmp, "crate", &expected),
        "test_inventory",
    );
    fs::remove_file(tests_dir.join("extra.rs")).unwrap();
    #[cfg(unix)]
    {
        let link = tests_dir.join("shadow.rs");
        std::os::unix::fs::symlink("only.rs", &link).unwrap();
        expect_err(
            assert_tests_closed(&tests_tmp, "crate", &expected),
            "test_symlink",
        );
        fs::remove_file(&link).unwrap();
    }
    fs::remove_dir_all(&tests_tmp).unwrap();
}
