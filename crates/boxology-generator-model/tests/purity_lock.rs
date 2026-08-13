//! Closed own-source directory, manifest, test-tree, and AST/effect authority for the generator
//! crates (#107A). Transitive dependency purity remains post-V0 under #358.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;
use syn::{Attribute, Meta, UseTree};
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
const PACKAGE_KEYS: &[&str] = &[
    "description",
    "edition",
    "homepage",
    "license",
    "name",
    "publish",
    "readme",
    "repository",
    "rust-version",
    "version",
];
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
            version: "=0.1.0",
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
    path_dep("boxology-schema", "../boxology-schema"),
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
            if matches!(name.as_str(), "LICENSE-MIT" | "LICENSE-APACHE") {
                continue;
            }
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
    if let Some(table) = item.as_table_like()
        && table.get("package").is_some()
    {
        return Err(format!("dep_alias: `{key}` must not set package = ..."));
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
    if package
        .get("version")
        .and_then(Item::as_table_like)
        .and_then(|v| v.get("workspace"))
        .and_then(Item::as_bool)
        != Some(true)
    {
        return Err("package_version: package.version must inherit workspace metadata".into());
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
        "[package]\nname=\"boxology-generator-model\"\nversion.workspace=true\nedition.workspace=true\nrust-version.workspace=true\nlicense.workspace=true\nrepository.workspace=true\nhomepage.workspace=true\nreadme.workspace=true\ndescription=\"Pure generation planning\"\npublish=true\n{extra}"
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
        ("package_keys", model_pkg(&format!("authors=[]\n{deps}"))),
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
        (
            "package_version",
            model_pkg(deps).replace("version.workspace=true", "version=\"1.2.3\""),
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

// --- Closed own-source AST / effect scanner (#107A). Transitive purity remains #358. ---
#[rustfmt::skip]
const ROOTS: &[&str] = &["alloc", "boxology_contract", "boxology_contract_syntax", "boxology_generator_model", "boxology_schema", "core", "crate", "imports", "manifest", "prettyplease", "proc_macro2", "rust", "schema", "self", "serde_json", "sha2", "std", "super", "syn", "toml_edit"];
#[rustfmt::skip]
const PURE_STD: &[&str] = &["any", "borrow", "boxed", "cell", "char", "clone", "cmp", "convert", "default", "error", "fmt", "future", "iter", "marker", "mem", "ops", "option", "pin", "prelude", "ptr", "rc", "result", "slice", "str", "string", "sync", "vec"];
const PURE_COLLECTIONS: &[&str] = &["BTreeMap", "BTreeSet", "btree_map", "btree_set"];
#[rustfmt::skip]
const EFFECT_STD: &[&str] = &["env", "fs", "io", "net", "os", "process", "random", "thread", "time"];
#[rustfmt::skip]
const ALLOW_MACROS: &[&str] = &["copy_getters", "format", "json", "matches", "model_getters", "ref_getters", "unreachable", "vec", "write"];
#[rustfmt::skip]
const DENY_MACROS: &[&str] = &["asm", "concat_bytes", "env", "global_asm", "include", "include_bytes", "include_str", "option_env"];
#[rustfmt::skip]
const MACRO_DEFS: &[(&str, &str)] = &[
    ("ref_getters", r#"($(#[$meta:meta] $name:ident: $return:ty = $field:tt;)*) => {$ ( #[$meta] pub fn $name(&self) -> $return { &self.$field } )*};"#),
    ("copy_getters", r#"($(#[$meta:meta] $name:ident: $return:ty = $field:ident;)*) => {$ ( #[$meta] pub fn $name(&self) -> $return { self.$field } )*};"#),
    ("model_getters", r#"($this:ident; $(#[$meta:meta] $name:ident: $return:ty = $body:expr;)*) => {$ ( #[$meta] pub fn $name(&$this) -> $return { $body } )*};"#),
];
const ALLOW_ATTRS: &[&str] = &["allow", "deny", "derive", "doc", "forbid", "rustfmt::skip"];
#[rustfmt::skip]
const ALLOW_DERIVES: &[&str] = &["Clone", "Copy", "Debug", "Eq", "Ord", "PartialEq", "PartialOrd"];
#[rustfmt::skip]
const ALLOW_LINTS: &[&str] = &["clippy::too_many_arguments", "deprecated", "missing_docs", "unsafe_code"];
#[rustfmt::skip]
const PRIMITIVES: &[&str] = &["bool", "char", "f32", "f64", "i128", "i16", "i32", "i64", "i8", "isize", "str", "u128", "u16", "u32", "u64", "u8", "usize"];

fn scan_source(src: &str) -> Result<(), String> {
    let file = syn::parse_file(src).map_err(|e| format!("parse: {e}"))?;
    let mut sc = Scanner {
        aliases: BTreeMap::new(),
        err: None,
    };
    file.attrs.iter().for_each(|a| sc.attr(a));
    file.items.iter().for_each(|i| sc.visit_item(i));
    sc.err.map_or(Ok(()), Err)
}

struct Scanner {
    aliases: BTreeMap<String, Vec<String>>,
    err: Option<String>,
}

impl Scanner {
    fn fail(&mut self, rule: &str, detail: impl std::fmt::Display) {
        if self.err.is_none() {
            self.err = Some(format!("{rule}: {detail}"));
        }
    }
    fn attr(&mut self, attr: &Attribute) {
        self.meta(&attr.meta);
    }
    fn meta(&mut self, meta: &Meta) {
        if self.err.is_some() {
            return;
        }
        let path = pj(meta.path());
        match path.as_str() {
            "cfg" if exact_cfg_test_meta(meta) => {}
            "cfg" => self.fail("cfg", "non-exact cfg"),
            "cfg_attr" => self.fail("cfg_attr", "cfg_attr forbidden"),
            "path" => self.fail("path", "#[path] forbidden"),
            p if !ALLOW_ATTRS.contains(&p) => self.fail("attr", format!("attribute `{path}`")),
            "derive" => self.meta_paths(meta, ALLOW_DERIVES, "derive"),
            "allow" | "deny" | "forbid" => self.meta_paths(meta, ALLOW_LINTS, "lint"),
            "doc" => {
                if let Meta::NameValue(nv) = meta
                    && !matches!(&nv.value, syn::Expr::Lit(l) if matches!(l.lit, syn::Lit::Str(_)))
                {
                    self.fail("attr", "doc must be a string literal");
                }
            }
            _ => {}
        }
    }
    fn macro_meta(&mut self, meta: &Meta) {
        match pj(meta.path()).as_str() {
            "cfg" => self.fail("cfg", "cfg forbidden in macro invocation"),
            "cfg_attr" => self.fail("cfg_attr", "cfg_attr forbidden in macro invocation"),
            _ => self.meta(meta),
        }
    }
    fn meta_paths(&mut self, meta: &Meta, allow: &[&str], kind: &str) {
        let Meta::List(list) = meta else {
            return self.fail("attr", format!("{kind} form"));
        };
        let Ok(args) = list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            return self.fail("attr", format!("{kind} parse"));
        };
        for meta in args {
            let Meta::Path(p) = meta else {
                return self.fail("attr", format!("{kind} entry"));
            };
            let name = pj(&p);
            if !allow.contains(&name.as_str()) {
                self.fail("attr", format!("{kind} `{name}`"));
            }
        }
    }
    fn skip_test(&mut self, attrs: &[Attribute]) -> bool {
        if attrs.iter().any(exact_cfg_test) {
            return true;
        }
        attrs.iter().for_each(|a| self.attr(a));
        false
    }
    fn mac(&mut self, mac: &syn::Macro) {
        if mac.path.leading_colon.is_some() {
            return self.fail("macro", format!("leading `::` macro `{}`", pj(&mac.path)));
        }
        let segs = ps(&mac.path);
        if let Err(rule) = self.chk_mac(&segs) {
            return self.fail(rule, segs.join("::"));
        }
        self.tokens(mac.tokens.clone());
    }
    fn macro_def(&mut self, name: &str, tokens: &TokenStream) {
        let Some((_, expected)) = MACRO_DEFS.iter().find(|(n, _)| *n == name) else {
            return self.fail("macro", format!("macro_rules `{name}`"));
        };
        self.token_stream(tokens.clone(), true);
        if self.err.is_some() {
            return;
        }
        let expected = expected
            .parse::<TokenStream>()
            .expect("trusted macro definition parses");
        if tokens.to_string() != expected.to_string() {
            self.fail("macro", format!("macro_rules `{name}` shape"));
        }
    }
    fn chk_mac(&self, segs: &[String]) -> Result<(), &'static str> {
        let r = self.resolve(segs);
        let name = r.last().map(String::as_str).unwrap_or("");
        if DENY_MACROS.contains(&name) {
            return Err(match name {
                "env" | "option_env" => "env",
                "include" | "include_str" | "include_bytes" => "include",
                n if n.contains("asm") => "asm",
                _ => "macro",
            });
        }
        // `Token!` is syntax supplied by syn, not an open macro allowlist entry. Requiring its
        // canonical path prevents a renamed macro from borrowing this trusted spelling.
        if name == "Token" {
            return (segs == ["syn", "Token"]).then_some(()).ok_or("macro");
        }
        if segs.len() > 1 {
            self.chk_path(segs).map_err(|_| "macro")?;
        }
        ALLOW_MACROS.contains(&name).then_some(()).ok_or("macro")
    }
    fn chk_path(&self, segs: &[String]) -> Result<(), &'static str> {
        if segs.is_empty() {
            return Ok(());
        }
        let r = self.resolve(segs);
        let head = r[0].as_str();
        if matches!(head, "std" | "core" | "alloc") {
            if r.len() >= 2 {
                let m = r[1].as_str();
                if let Some(rule) = EFFECT_STD.iter().copied().find(|&e| e == m) {
                    return Err(rule);
                }
                if m == "collections" {
                    return if r.len() >= 3 && PURE_COLLECTIONS.contains(&r[2].as_str()) {
                        Ok(())
                    } else {
                        Err("random")
                    };
                }
                if !PURE_STD.contains(&m) {
                    return Err("std");
                }
            }
            return Ok(());
        }
        if ROOTS.contains(&head) {
            return Ok(());
        }
        if r.len() >= 2 && crate_style(head) {
            return Err("root");
        }
        Ok(())
    }
    fn resolve(&self, segs: &[String]) -> Vec<String> {
        match self.aliases.get(&segs[0]) {
            Some(b) => b
                .iter()
                .cloned()
                .chain(segs.iter().skip(1).cloned())
                .collect(),
            None => segs.to_vec(),
        }
    }
    fn path(&mut self, path: &syn::Path) {
        if path.leading_colon.is_some() {
            return self.fail("root", "leading `::` path");
        }
        let segs = ps(path);
        if (segs.len() >= 2 || ROOTS.contains(&segs[0].as_str()))
            && let Err(rule) = self.chk_path(&segs)
        {
            self.fail(rule, segs.join("::"));
        }
    }
    fn use_tree(&mut self, prefix: &[String], tree: &UseTree) {
        match tree {
            UseTree::Path(p) => {
                let mut n = prefix.to_vec();
                n.push(ident(&p.ident));
                self.use_tree(&n, &p.tree);
            }
            UseTree::Name(n) => {
                let mut f = prefix.to_vec();
                f.push(ident(&n.ident));
                self.import(&f, ident(&n.ident));
            }
            UseTree::Rename(n) => {
                let mut f = prefix.to_vec();
                f.push(ident(&n.ident));
                self.import(&f, ident(&n.rename));
            }
            UseTree::Glob(_) => self.fail("use", "glob import"),
            UseTree::Group(g) => g.items.iter().for_each(|t| self.use_tree(prefix, t)),
        }
    }
    fn import(&mut self, full: &[String], bind: String) {
        if let Err(rule) = self.chk_path(full) {
            return self.fail(rule, full.join("::"));
        }
        let resolved = self.resolve(full);
        if ROOTS.contains(&bind.as_str()) {
            return self.fail("alias", format!("binding shadows root `{bind}`"));
        }
        let head = resolved[0].as_str();
        if resolved.len() == 1 && matches!(head, "std" | "core" | "alloc") {
            return self.fail("alias", format!("ambient root alias `{bind}`"));
        }
        if !ROOTS.contains(&head) && crate_style(head) {
            return self.fail("root", full.join("::"));
        }
        self.aliases.insert(bind, resolved);
    }
    fn tokens(&mut self, ts: TokenStream) {
        self.token_stream(ts, false);
    }
    fn token_stream(&mut self, ts: TokenStream, macro_definition: bool) {
        if self.err.is_none() {
            self.toks(&ts.into_iter().collect::<Vec<_>>(), macro_definition);
        }
    }
    fn toks(&mut self, t: &[TokenTree], macro_definition: bool) {
        let mut i = 0;
        while i < t.len() && self.err.is_none() {
            let attribute = match (t.get(i + 1), t.get(i + 2)) {
                (Some(TokenTree::Group(group)), _) => Some((group, 2)),
                (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(group)))
                    if bang.as_char() == '!' =>
                {
                    Some((group, 3))
                }
                _ => None,
            };
            if matches!(&t[i], TokenTree::Punct(p) if p.as_char() == '#')
                && let Some((g, width)) = attribute
                && g.delimiter() == proc_macro2::Delimiter::Bracket
            {
                let attr_tokens = g.stream();
                let attr_parts = attr_tokens.clone().into_iter().collect::<Vec<_>>();
                let is_meta_var = macro_definition
                    && match attr_parts.as_slice() {
                        [TokenTree::Punct(p), TokenTree::Ident(_)] => p.as_char() == '$',
                        [
                            TokenTree::Punct(dollar),
                            TokenTree::Ident(_),
                            TokenTree::Punct(colon),
                            TokenTree::Ident(kind),
                        ] => dollar.as_char() == '$' && colon.as_char() == ':' && *kind == "meta",
                        _ => false,
                    };
                if is_meta_var {
                    self.token_stream(attr_tokens, true);
                } else if let Ok(meta) = syn::parse2::<Meta>(attr_tokens) {
                    self.macro_meta(&meta);
                } else {
                    self.fail("attr", "attribute token parse");
                }
                i += width;
                continue;
            }
            if dcolon(t, i) {
                let (segs, next) = read_path(t, i);
                if next < t.len() && matches!(&t[next], TokenTree::Punct(p) if p.as_char() == '!') {
                    return self.fail("macro", format!("leading `::` macro `{}`", segs.join("::")));
                }
                return self.fail("root", "leading `::` token path");
            }
            match &t[i] {
                TokenTree::Group(g) => {
                    self.token_stream(g.stream(), macro_definition);
                    i += 1;
                }
                TokenTree::Literal(_) | TokenTree::Punct(_) => i += 1,
                TokenTree::Ident(id) => {
                    // Macro bodies are expansion-live but AST-opaque; ban smuggled unsafety.
                    if *id == "unsafe" {
                        return self.fail("unsafe", "token");
                    }
                    if *id == "extern" {
                        return self.fail("extern", "token");
                    }
                    let (segs, next) = read_path(t, i);
                    if next < t.len()
                        && matches!(&t[next], TokenTree::Punct(p) if p.as_char() == '!')
                    {
                        if let Err(rule) = self.chk_mac(&segs) {
                            return self.fail(rule, segs.join("::"));
                        }
                        let body = next + 1;
                        if body < t.len()
                            && let TokenTree::Group(g) = &t[body]
                        {
                            self.token_stream(g.stream(), macro_definition);
                            i = body + 1;
                            continue;
                        }
                        i = body;
                        continue;
                    }
                    if (segs.len() >= 2 || ROOTS.contains(&segs[0].as_str()))
                        && let Err(rule) = self.chk_path(&segs)
                    {
                        return self.fail(rule, segs.join("::"));
                    }
                    i = next;
                }
            }
        }
    }
    fn unsafety(&mut self, s: &syn::Safety, w: &str) {
        if matches!(s, syn::Safety::Unsafe(_)) {
            self.fail("unsafe", w);
        }
    }
}

impl<'ast> Visit<'ast> for Scanner {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if self.err.is_some() || self.skip_test(item_attrs(item)) {
            return;
        }
        match item {
            syn::Item::ExternCrate(e) => {
                let name = ident(&e.ident);
                let bind = e
                    .rename
                    .as_ref()
                    .map(|(_, i)| ident(i))
                    .unwrap_or_else(|| name.clone());
                self.import(&[name], bind);
            }
            syn::Item::Use(u) => {
                if u.leading_colon.is_some() {
                    return self.fail("root", "leading `::` use");
                }
                self.use_tree(&[], &u.tree);
            }
            syn::Item::Macro(m) => {
                if let Some(n) = &m.ident {
                    self.macro_def(&ident(n), &m.mac.tokens);
                    return; // Avoid revisiting `macro_rules` as an invocation.
                }
            }
            syn::Item::ForeignMod(_) => self.fail("extern", "foreign block"),
            syn::Item::Fn(f) => {
                self.unsafety(&f.sig.safety, "fn");
                if f.sig.abi.is_some() {
                    self.fail("extern", "extern fn");
                }
            }
            syn::Item::Static(s) if matches!(s.mutability, syn::StaticMutability::Mut(_)) => {
                self.fail("unsafe", "static mut");
            }
            syn::Item::Impl(i) if i.unsafety.is_some() => self.fail("unsafe", "impl"),
            syn::Item::Trait(t) if t.unsafety.is_some() => self.fail("unsafe", "trait"),
            syn::Item::Verbatim(_) => self.fail("parse", "verbatim item"),
            syn::Item::Mod(m)
                if m.content.is_none() && !ROOTS.contains(&ident(&m.ident).as_str()) =>
            {
                self.fail("root", format!("outline mod `{}`", m.ident));
            }
            _ => {}
        }
        if self.err.is_none() {
            syn::visit::visit_item(self, item);
        }
    }
    fn visit_impl_item(&mut self, n: &'ast syn::ImplItem) {
        if self.err.is_some() || self.skip_test(impl_item_attrs(n)) {
            return;
        }
        if matches!(n, syn::ImplItem::Verbatim(_)) {
            return self.fail("parse", "verbatim impl item");
        }
        if self.err.is_none() {
            syn::visit::visit_impl_item(self, n);
        }
    }
    fn visit_trait_item(&mut self, n: &'ast syn::TraitItem) {
        if self.err.is_some() || self.skip_test(trait_item_attrs(n)) {
            return;
        }
        if matches!(n, syn::TraitItem::Verbatim(_)) {
            return self.fail("parse", "verbatim trait item");
        }
        if self.err.is_none() {
            syn::visit::visit_trait_item(self, n);
        }
    }
    fn visit_impl_item_fn(&mut self, n: &'ast syn::ImplItemFn) {
        if self.err.is_some() || self.skip_test(&n.attrs) {
            return;
        }
        self.unsafety(&n.sig.safety, "impl fn");
        if n.sig.abi.is_some() {
            return self.fail("extern", "extern impl fn");
        }
        syn::visit::visit_impl_item_fn(self, n);
    }
    fn visit_trait_item_fn(&mut self, n: &'ast syn::TraitItemFn) {
        if self.err.is_some() || self.skip_test(&n.attrs) {
            return;
        }
        self.unsafety(&n.sig.safety, "trait fn");
        if n.sig.abi.is_some() {
            return self.fail("extern", "extern trait fn");
        }
        syn::visit::visit_trait_item_fn(self, n);
    }
    fn visit_expr(&mut self, e: &'ast syn::Expr) {
        if matches!(e, syn::Expr::Verbatim(_)) {
            return self.fail("parse", "verbatim expr");
        }
        if self.err.is_none() {
            syn::visit::visit_expr(self, e);
        }
    }
    fn visit_expr_unsafe(&mut self, n: &'ast syn::ExprUnsafe) {
        self.fail("unsafe", "block");
        syn::visit::visit_expr_unsafe(self, n);
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        if self.err.is_none() {
            self.mac(m);
        }
    }
    fn visit_type_fn_ptr(&mut self, n: &'ast syn::TypeFnPtr) {
        if n.unsafety.is_some() {
            return self.fail("unsafe", "bare fn type");
        }
        if n.abi.is_some() {
            return self.fail("extern", "bare fn abi");
        }
        if self.err.is_none() {
            syn::visit::visit_type_fn_ptr(self, n);
        }
    }
    fn visit_path(&mut self, p: &'ast syn::Path) {
        if self.err.is_none() {
            self.path(p);
            syn::visit::visit_path(self, p);
        }
    }
    fn visit_attribute(&mut self, a: &'ast Attribute) {
        if self.err.is_none() {
            self.attr(a);
        }
    }
}

fn item_attrs(i: &syn::Item) -> &[Attribute] {
    match i {
        syn::Item::Const(i) => &i.attrs,
        syn::Item::Enum(i) => &i.attrs,
        syn::Item::ExternCrate(i) => &i.attrs,
        syn::Item::Fn(i) => &i.attrs,
        syn::Item::ForeignMod(i) => &i.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Macro(i) => &i.attrs,
        syn::Item::Mod(i) => &i.attrs,
        syn::Item::Static(i) => &i.attrs,
        syn::Item::Struct(i) => &i.attrs,
        syn::Item::Trait(i) => &i.attrs,
        syn::Item::TraitAlias(i) => &i.attrs,
        syn::Item::Type(i) => &i.attrs,
        syn::Item::Union(i) => &i.attrs,
        syn::Item::Use(i) => &i.attrs,
        _ => &[],
    }
}
fn impl_item_attrs(i: &syn::ImplItem) -> &[Attribute] {
    match i {
        syn::ImplItem::Const(i) => &i.attrs,
        syn::ImplItem::Fn(i) => &i.attrs,
        syn::ImplItem::Type(i) => &i.attrs,
        syn::ImplItem::Macro(i) => &i.attrs,
        syn::ImplItem::Verbatim(_) => &[],
        _ => &[],
    }
}
fn trait_item_attrs(i: &syn::TraitItem) -> &[Attribute] {
    match i {
        syn::TraitItem::Const(i) => &i.attrs,
        syn::TraitItem::Fn(i) => &i.attrs,
        syn::TraitItem::Type(i) => &i.attrs,
        syn::TraitItem::Macro(i) => &i.attrs,
        syn::TraitItem::Verbatim(_) => &[],
        _ => &[],
    }
}
fn exact_cfg_test(a: &Attribute) -> bool {
    exact_cfg_test_meta(&a.meta)
}
fn exact_cfg_test_meta(meta: &Meta) -> bool {
    let Meta::List(l) = meta else {
        return false;
    };
    if !l.path.is_ident("cfg") {
        return false;
    }
    let Ok(args) =
        l.parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
    else {
        return false;
    };
    matches!(args.iter().collect::<Vec<_>>()[..], [Meta::Path(p)] if p.is_ident("test"))
}
fn ps(p: &syn::Path) -> Vec<String> {
    p.segments.iter().map(|s| ident(&s.ident)).collect()
}
fn pj(p: &syn::Path) -> String {
    ps(p).join("::")
}
fn crate_style(n: &str) -> bool {
    !PRIMITIVES.contains(&n)
        && matches!(n.chars().next(), Some('a'..='z'))
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
}
fn ident(i: &proc_macro2::Ident) -> String {
    let spelling = i.to_string();
    spelling.strip_prefix("r#").unwrap_or(&spelling).to_owned()
}
fn dcolon(t: &[TokenTree], i: usize) -> bool {
    matches!(
        (t.get(i), t.get(i + 1)),
        (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
            if a.as_char() == ':' && b.as_char() == ':'
    )
}
fn read_path(t: &[TokenTree], start: usize) -> (Vec<String>, usize) {
    let mut i = start + if dcolon(t, start) { 2 } else { 0 };
    let mut segs = Vec::new();
    while i < t.len() {
        let TokenTree::Ident(id) = &t[i] else {
            break;
        };
        segs.push(ident(id));
        i += 1;
        if !dcolon(t, i) {
            break;
        }
        i += 2;
        if matches!(t.get(i), Some(TokenTree::Punct(p)) if p.as_char() == '<') {
            let mut depth = 0;
            while i < t.len() {
                if let TokenTree::Punct(p) = &t[i] {
                    match p.as_char() {
                        '<' => depth += 1,
                        '>' => {
                            depth -= 1;
                            i += 1;
                            if depth == 0 {
                                break;
                            }
                            continue;
                        }
                        _ => {}
                    }
                }
                i += 1;
            }
            if !dcolon(t, i) {
                break;
            }
            i += 2;
        }
    }
    if segs.is_empty() {
        (vec![String::new()], start + 1)
    } else {
        (segs, i)
    }
}

#[test]
fn production_sources_pass_effect_scan() {
    let root = root();
    for rel in SOURCES {
        let src = fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let src = src
            .replace("include_bytes!(\"../../../LICENSE-APACHE\")", "b\"\"")
            .replace("include_bytes!(\"../../../LICENSE-MIT\")", "b\"\"");
        scan_source(&src).unwrap_or_else(|e| panic!("{rel}: {e}"));
    }
}

#[test]
fn effect_scan_rejects_hostile_corpus() {
    #[rustfmt::skip]
    let rows = [
        ("fs", "fn f() { let _ = std::fs::read(\"x\"); }"),
        ("fs", "use std::fs as x; fn f() { let _ = x::read(\"x\"); }"),
        ("fs", "fn f() { let _ = r#std::fs::read(\"x\"); }"),
        ("alias", "extern crate std as s; fn f() { let _ = s::fs::read(\"x\"); }"),
        ("alias", "use crate as std; fn f() { let _ = ::std::fs::read(\"x\"); }"),
        ("root", "fn f() { let _ = ::std::mem::size_of::<u8>(); }"),
        ("alias", "use std as s; fn f() { let _ = crate::s::fs::read(\"x\"); }"),
        ("fs", "struct S; impl S { model_getters! { self; a: std::fs::File = a; } }"),
        ("fs", "use serde_json::json as Token; fn f() { let _ = Token!({\"x\": std::fs::read(\"x\")}); }"),
        ("root", "fn f() { let _ = undeclared_crate::foo(); }"),
        ("root", "use undeclared_crate;"),
        ("root", "extern crate undeclared_crate;"),
        ("env", "fn f() { let _ = env!(\"PATH\"); }"),
        ("include", "fn f() { let _ = include!(\"x.rs\"); }"),
        ("include", "fn f() { let _ = include_str!(\"x.txt\"); }"),
        ("include", "fn f() { let _ = include_bytes!(\"x.bin\"); }"),
        ("macro", "fn f() { let _ = concat_bytes!(b\"x\"); }"),
        ("env", "fn f() { let _ = option_env!(\"x\"); }"),
        ("macro", "macro_rules! wrap { () => { env!(\"x\") }; }"),
        ("macro", "macro_rules! model_getters { ($a:ident, $b:ident, $c:ident) => { fn f() { let _ = $a::$b::$c(\"x\"); } }; } model_getters!(std, fs, read);"),
        ("include", "fn f() { let _ = format!(\"{}\", json!({\"x\": include_str!(\"x\")})); }"),
        ("asm", "fn f() { asm!(\"\"); }"),
        ("asm", "global_asm!(\"\");"),
        ("path", "#[path = \"x.rs\"] mod m;"),
        ("cfg", "#[cfg(any(test))] fn f() {}"),
        ("cfg", "struct S { #[cfg(feature = \"x\")] f: u8 }"),
        ("cfg_attr", "#[cfg_attr(test, allow(dead_code))] fn f() {}"),
        ("cfg", "struct S; impl S { model_getters! { self; #[cfg(feature = \"hostile\")] a: usize = 0; } }"),
        ("cfg", "struct S; impl S { model_getters! { self; #[cfg(test)] a: usize = 0; } }"),
        ("cfg", "struct S; impl S { model_getters! { self; #[doc = \"x\"] a: () = { #![cfg(test)] () }; } }"),
        ("cfg_attr", "struct S; impl S { model_getters! { self; #[doc = \"x\"] a: () = { #![cfg_attr(test, allow(dead_code))] () }; } }"),
        ("cfg_attr", "struct S; impl S { model_getters! { self; #[cfg_attr(test, doc = \"hostile\")] a: usize = 0; } }"),
        ("attr", "struct S; impl S { model_getters! { self; #[hostile] a: usize = 0; } }"),
        ("cfg_attr", "struct S; impl S { #[cfg_attr(test, allow(deprecated))] fn f() {} }"),
        ("cfg_attr", "trait T { #[cfg_attr(test, allow(deprecated))] fn f(); }"),
        ("cfg_attr", "struct S { #[cfg_attr(test, allow(deprecated))] f: u8 }"),
        ("cfg_attr", "enum E { #[cfg_attr(test, allow(deprecated))] V }"),
        ("fs", "struct S { #[cfg(test)] f: std::fs::File }"),
        ("fs", "enum E { V(#[cfg(test)] std::fs::File) }"),
        ("unsafe", "unsafe fn f() {}"),
        ("unsafe", "fn f() { unsafe { let _ = 1; } }"),
        ("unsafe", "type F = unsafe fn();"),
        ("unsafe", "static mut X: u8 = 0;"),
        ("unsafe", "macro_rules! model_getters { () => { unsafe {} }; }"),
        ("unsafe", "macro_rules! model_getters { () => { format!(\"{}\", { unsafe {} }) }; }"),
        ("extern", "extern \"C\" { fn f(); }"),
        ("extern", "type F = extern \"C\" fn();"),
        ("extern", "macro_rules! model_getters { () => { type F = extern \"C\" fn(); }; }"),
        ("net", "fn f() { let _ = std::net::Ipv4Addr::LOCALHOST; }"),
        ("process", "fn f() { let _ = std::process::id(); }"),
        ("time", "fn f() { let _ = std::time::Instant::now(); }"),
        ("thread", "fn f() { let _ = std::thread::available_parallelism(); }"),
        ("io", "fn f() { let _ = std::io::empty(); }"),
        ("os", "fn f(_: &dyn std::os::unix::ffi::OsStrExt) {}"),
        ("env", "fn f() { let _ = std::env::var(\"x\"); }"),
        ("random", "fn f() { let _ = std::random::random::<u8>(); }"),
        ("random", "use std::collections::HashMap; fn f() { let _: HashMap<u8, u8> = HashMap::new(); }"),
        ("random", "fn f() { let _ = std::collections::hash_map::RandomState::default(); }"),
        ("fs", "fn f() { let _ = format!(\"{}\", std::fs::read(\"x\")); }"),
        ("macro", "use syn::Token as T; type Comma = T![,];"),
        ("macro", "use syn::Token; type Comma = Token![,];"),
        ("macro", "type Comma = ::syn::Token![,];"),
        ("macro", "fn f() { let _ = ::serde_json::json!({\"x\": 1}); }"),
        ("macro", "fn f() { let _ = format!(\"{}\", ::serde_json::json!({\"x\": 1})); }"),
    ];
    for (rule, src) in rows {
        expect_err(scan_source(src), rule);
    }
    scan_source("#[cfg(test)] fn f() { let _ = std::fs::read(\"x\"); }").expect("cfg(test) ok");
    scan_source("struct S; impl S { #[cfg(test)] model_getters! { self; a: std::fs::File = a; } }")
        .expect("cfg(test) associated item subtree is excluded");
    expect_err(
        scan_source(
            "#[cfg(test)] fn t() { let _ = std::fs::read(\"x\"); }\nfn p() { let _ = std::fs::read(\"y\"); }",
        ),
        "fs",
    );
}

#[test]
fn effect_scan_allows_positive_controls() {
    #[rustfmt::skip]
    let rows = [
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)] struct S;",
        "#[allow(deprecated)] fn f() {}",
        "#[doc = \"hi\"] fn f() {}",
        "#[rustfmt::skip] fn f() {}",
        "fn f() { let _ = format!(\"{}\", \"std::fs env! include! unsafe\"); }",
        "fn f() { let _ = b\"std::fs\"; let _ = \"env!\"; let _ = r#\"include!\"#; }",
        "fn f() { let _ = vec![1, 2]; let _ = matches!(0, 0); let _ = unreachable!(); }",
        "fn f() { let _ = format!(\"{}\", format!(\"{}\", 1)); }",
        "use std::collections::{BTreeMap, BTreeSet}; fn f() { let _: BTreeMap<u8, u8> = BTreeMap::new(); let _ = BTreeSet::<u8>::new(); }",
        "use serde_json::json; fn f() { let _ = json!({ \"a\": 1 }); }",
        "use serde_json::{Value, json}; fn f() { let _: Value = json!({ \"a\": [1] }); }",
        "type Metas = syn::punctuated::Punctuated<syn::Meta, syn::Token![,]>;",
        "use syn::parse::Parser as _;",
        "struct S; impl S { model_getters! { self; #[doc = \"d\"] a: &'static str = a; } }",
        "struct S; impl S { ref_getters! { self; a: &'static str = a; } copy_getters! { self; n: usize = n; } }",
        "struct S { #[cfg(test)] f: u8 } enum E { #[doc = \"v\"] V } trait T { #[doc = \"t\"] fn f(); }",
        "enum BoundaryLeaf { A } fn f() { use BoundaryLeaf as Wire; let _ = Wire::A; }",
        "use std::sync as sync_one; use sync_one as sync_two; fn f() { let _ = sync_two::atomic::AtomicUsize::new(0); }",
        "use std::sync::atomic::{AtomicUsize, Ordering}; fn f() { let a = AtomicUsize::new(0); let _ = a.load(Ordering::Relaxed); }",
        "use boxology_contract::BoxId; fn f(x: BoxId) { let _ = x; }",
        "fn f() { let _ = std::str::from_utf8(b\"a\"); }",
        "fn f() { let _ = r#std::str::from_utf8(b\"a\"); }",
        "mod schema; fn f() {}",
        "#[cfg(test)] mod tests { fn evil() { let _ = std::fs::read(\"x\"); env!(\"y\"); } }",
    ];
    for src in rows {
        scan_source(src).unwrap_or_else(|e| panic!("should allow `{src}`: {e}"));
    }
}
