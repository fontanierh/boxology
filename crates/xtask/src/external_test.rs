use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path},
    process::Command,
};

use proc_macro2::TokenTree;
use quote::ToTokens;
use sha2::{Digest, Sha256};
use syn::visit::Visit;
use syn::{Expr, ExprCall, ExprPath, ItemConst, ItemFn, ItemStatic, Macro};
use toml_edit::{DocumentMut, Item, Table};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExternalTestSpec {
    pub(crate) package: &'static str,
    pub(crate) target: &'static str,
    pub(crate) manifest: &'static str,
    pub(crate) source: &'static str,
    pub(crate) default_source: &'static str,
    pub(crate) tests: &'static [&'static str],
    /// SHA-256 over the exact bytes of the pinned test source file. Checked before
    /// `body_digest` so enforcement helpers outside the test closure stay pinned.
    pub(crate) source_digest: &'static str,
    /// SHA-256 over listed `#[test]` bodies plus the transitive file-level `fn` /
    /// `const` / `static` closure each reaches (bare calls/paths + macro token
    /// idents). Refresh from `observed`.
    pub(crate) body_digest: &'static str,
}

pub(crate) fn require_external_tests(
    root: &Path,
    mut run: impl FnMut(&[&str]) -> Option<(bool, Vec<u8>)>,
    spec: &ExternalTestSpec,
) -> Result<(), String> {
    let pkg = spec.package;
    if spec.tests.is_empty() {
        return Err(format!("{pkg}: empty tests list"));
    }
    if !regular_file(root, spec.manifest) {
        return Err(format!("{pkg}: manifest missing or not a regular file"));
    }
    if !regular_file(root, spec.source) {
        return Err(format!("{pkg}: source missing or not a regular file"));
    }
    if build_script_exists(root, spec.manifest) {
        return Err(format!("{pkg}: build script is forbidden"));
    }
    let Ok(manifest) = fs::read_to_string(root.join(spec.manifest)) else {
        return Err(format!("{pkg}: cannot read manifest"));
    };
    let Ok(source_bytes) = fs::read(root.join(spec.source)) else {
        return Err(format!("{pkg}: cannot read source"));
    };
    let Ok(source) = std::str::from_utf8(&source_bytes).map(str::to_owned) else {
        return Err(format!("{pkg}: source is not UTF-8"));
    };
    if !manifest_is_exact(&manifest, spec) {
        return Err(format!("{pkg}: manifest does not match pinned controls"));
    }
    source_matches_digest(&source_bytes, spec.source_digest)
        .map_err(|error| format!("{pkg}: {error}"))?;
    bodies_match_digest(&source, spec.tests, spec.body_digest)
        .map_err(|error| format!("{pkg}: {error}"))?;
    if !execute(&mut run, spec) {
        return Err(format!("{pkg}: cargo list/run mismatch or cargo failed"));
    }
    Ok(())
}

fn source_matches_digest(bytes: &[u8], expected: &str) -> Result<(), String> {
    let observed = format!("{:x}", Sha256::digest(bytes));
    if observed != expected {
        return Err(format!(
            "source digest mismatch: expected {expected}, observed {observed}"
        ));
    }
    Ok(())
}

/// Digest listed `#[test]` blocks plus each test's transitive file-level closure.
/// Refresh pins by copying `observed`.
fn bodies_match_digest(source: &str, tests: &[&str], expected: &str) -> Result<(), String> {
    let observed = body_digest(source, tests)?;
    if observed != expected {
        return Err(format!(
            "body digest mismatch: expected {expected}, observed {observed}"
        ));
    }
    Ok(())
}

fn body_digest(source: &str, tests: &[&str]) -> Result<String, String> {
    if tests.is_empty() {
        return Err("empty tests list".to_string());
    }
    let file = syn::parse_file(source).map_err(|_| "source parse failed".to_string())?;
    let index = index_file_level_defs(&file);
    let mut hasher = Sha256::new();
    for name in tests {
        let Some(defs) = index.get(*name) else {
            return Err(format!("missing listed test `{name}`"));
        };
        let item = match defs.as_slice() {
            [FileDef::Fn(item)] if is_bare_test(item) => *item,
            _ => {
                return Err(format!("listed name `{name}` is not a unique #[test]"));
            }
        };
        hash_part(
            &mut hasher,
            "test",
            name,
            &item.block.to_token_stream().to_string(),
        );
        let mut closure = transitive_closure(&index, &item.block)?;
        closure.sort_unstable_by_key(|def| (def.kind_tag(), def.name()));
        for def in closure {
            let name = def.name();
            hash_part(&mut hasher, def.kind_tag(), &name, &def.tokens());
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_part(hasher: &mut Sha256, kind: &str, name: &str, tokens: &str) {
    hasher.update(kind.as_bytes());
    hasher.update(b"\0");
    hasher.update(name.as_bytes());
    hasher.update(b"\0");
    hasher.update(tokens.as_bytes());
    hasher.update(b"\0");
}

fn is_bare_test(item: &ItemFn) -> bool {
    item.attrs
        .iter()
        .any(|attribute| attribute.path().is_ident("test"))
}

fn bare_call_ident(call: &ExprCall) -> Option<String> {
    let Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    bare_path_name(path)
}

fn bare_path_name(path: &ExprPath) -> Option<String> {
    if path.qself.is_none() && path.path.segments.len() == 1 {
        Some(path.path.segments[0].ident.to_string())
    } else {
        None
    }
}

fn index_file_level_defs(file: &syn::File) -> HashMap<String, Vec<FileDef<'_>>> {
    let mut by_name = HashMap::<String, Vec<FileDef<'_>>>::new();
    for item in &file.items {
        let (name, def) = match item {
            syn::Item::Fn(item) => (item.sig.ident.to_string(), FileDef::Fn(item)),
            syn::Item::Const(item) => (item.ident.to_string(), FileDef::Const(item)),
            syn::Item::Static(item) => (item.ident.to_string(), FileDef::Static(item)),
            _ => continue,
        };
        by_name.entry(name).or_default().push(def);
    }
    by_name
}

fn transitive_closure<'ast>(
    index: &HashMap<String, Vec<FileDef<'ast>>>,
    seed: &'ast syn::Block,
) -> Result<Vec<FileDef<'ast>>, String> {
    let mut refs = RefCollector::default();
    refs.visit_block(seed);
    let mut worklist = refs.names;
    let mut visited = HashSet::new();
    let mut closure = Vec::new();
    while let Some(name) = worklist.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(defs) = index.get(&name) else {
            continue;
        };
        if defs.len() != 1 {
            return Err(format!("ambiguous referenced definition `{name}`"));
        }
        let def = defs[0];
        let mut next = RefCollector::default();
        match def {
            FileDef::Fn(item) => next.visit_block(&item.block),
            FileDef::Const(item) => next.visit_expr(&item.expr),
            FileDef::Static(item) => next.visit_expr(&item.expr),
        }
        worklist.extend(next.names);
        closure.push(def);
    }
    Ok(closure)
}

fn collect_idents_from_tokens(tokens: proc_macro2::TokenStream, out: &mut Vec<String>) {
    for tree in tokens {
        match tree {
            TokenTree::Ident(ident) => out.push(ident.to_string()),
            TokenTree::Group(group) => collect_idents_from_tokens(group.stream(), out),
            TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

#[derive(Clone, Copy)]
enum FileDef<'ast> {
    Fn(&'ast ItemFn),
    Const(&'ast ItemConst),
    Static(&'ast ItemStatic),
}

impl<'ast> FileDef<'ast> {
    fn kind_tag(self) -> &'static str {
        match self {
            Self::Fn(_) => "fn",
            Self::Const(_) => "const",
            Self::Static(_) => "static",
        }
    }

    fn name(self) -> String {
        match self {
            Self::Fn(item) => item.sig.ident.to_string(),
            Self::Const(item) => item.ident.to_string(),
            Self::Static(item) => item.ident.to_string(),
        }
    }

    fn tokens(self) -> String {
        match self {
            Self::Fn(item) => item.block.to_token_stream().to_string(),
            Self::Const(item) => item.expr.to_token_stream().to_string(),
            Self::Static(item) => item.expr.to_token_stream().to_string(),
        }
    }
}

#[derive(Default)]
struct RefCollector {
    names: Vec<String>,
}

impl<'ast> Visit<'ast> for RefCollector {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Some(ident) = bare_call_ident(node) {
            self.names.push(ident);
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        if let Some(ident) = bare_path_name(node) {
            self.names.push(ident);
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        collect_idents_from_tokens(mac.tokens.clone(), &mut self.names);
    }
}

pub(crate) fn run_with_cargo(
    root: &Path,
    spec: &ExternalTestSpec,
    run: impl FnMut(&[&str]) -> Option<(bool, Vec<u8>)>,
) -> Result<(), String> {
    require_external_tests(root, run, spec)
}

pub(crate) fn cargo(root: &Path, args: &[&str]) -> Option<(bool, Vec<u8>)> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    Some((output.status.success(), output.stdout))
}

fn build_script_exists(root: &Path, manifest: &str) -> bool {
    let Some(directory) = Path::new(manifest).parent() else {
        return false;
    };
    fs::symlink_metadata(root.join(directory).join("build.rs")).is_ok()
}

fn regular_file(root: &Path, relative: &str) -> bool {
    let components: Vec<_> = Path::new(relative).components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return false;
    }
    let Ok(root_metadata) = fs::symlink_metadata(root) else {
        return false;
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return false;
    }
    let mut path = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        path.push(component);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if metadata.file_type().is_symlink()
            || (index + 1 == components.len() && !metadata.is_file())
            || (index + 1 != components.len() && !metadata.is_dir())
        {
            return false;
        }
    }
    true
}

fn manifest_is_exact(text: &str, spec: &ExternalTestSpec) -> bool {
    let Ok(document) = text.parse::<DocumentMut>() else {
        return false;
    };
    let Some(package) = document.get("package").and_then(Item::as_table) else {
        return false;
    };
    if package.get("name").and_then(Item::as_str) != Some(spec.package)
        || package.get("build").is_some()
    {
        return false;
    }
    for flag in [
        "autolib",
        "autobins",
        "autoexamples",
        "autotests",
        "autobenches",
    ] {
        if package.get(flag).map_or(Some(true), Item::as_bool) != Some(true) {
            return false;
        }
    }
    if ["lib", "bin", "example", "bench"]
        .iter()
        .any(|target| document.get(target).is_some())
    {
        return false;
    }
    match document.get("test") {
        None => true,
        Some(item) => {
            let Some(tables) = item.as_array_of_tables() else {
                return false;
            };
            tables.len() == 1
                && tables
                    .get(0)
                    .is_some_and(|table| test_table_is_exact(table, spec))
        }
    }
}

fn test_table_is_exact(table: &Table, spec: &ExternalTestSpec) -> bool {
    if table
        .iter()
        .any(|(key, _)| !["name", "test", "harness", "path"].contains(&key))
    {
        return false;
    }
    table.get("name").and_then(Item::as_str) == Some(spec.target)
        && table.get("test").map_or(Some(true), Item::as_bool) == Some(true)
        && table.get("harness").map_or(Some(true), Item::as_bool) == Some(true)
        && table
            .get("path")
            .map_or(Some(spec.default_source), Item::as_str)
            == Some(spec.default_source)
        && table.get("required-features").is_none()
}

fn execute(
    mut run: impl FnMut(&[&str]) -> Option<(bool, Vec<u8>)>,
    spec: &ExternalTestSpec,
) -> bool {
    if spec.tests.is_empty() {
        return false;
    }
    let list_args = [
        "test",
        "-p",
        spec.package,
        "--test",
        spec.target,
        "--",
        "--list",
    ];
    // Keep multi-test list/run name order exact under libtest parallelism.
    let mut run_args = list_args;
    *run_args.last_mut().expect("list args") = "--test-threads=1";
    match (run(&list_args), run(&run_args)) {
        (Some((true, listed)), Some((true, executed))) => {
            exact(names(&listed, "", ": test"), spec.tests)
                && exact(names(&executed, "test ", " ... ok"), spec.tests)
        }
        _ => false,
    }
}

fn names(output: &[u8], prefix: &str, suffix: &str) -> Option<Vec<String>> {
    let text = std::str::from_utf8(output).ok()?;
    let mut names = Vec::new();
    for line in text.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix(prefix) else {
            continue;
        };
        if let Some(name) = rest.strip_suffix(suffix) {
            if name.is_empty() {
                return None;
            }
            names.push(name.to_owned());
        } else if prefix == "test " && rest.contains(" ... ") {
            return None;
        }
    }
    Some(names)
}

fn exact(actual: Option<Vec<String>>, expected: &[&str]) -> bool {
    let Some(actual) = actual else { return false };
    !expected.is_empty()
        && actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const BASE_MANIFEST: &str =
        "[package]\nname = \"client\"\nversion = \"0.0.0\"\nedition = \"2024\"\n";
    /// Digest of `a`/`b` bodies that each contain `assert!(true)`.
    const LIVE_AB_DIGEST: &str = "e6aaeaa844c2d0820ecd01aba8720523fdeb997a9f95de2e3d50d6b314e76a71";
    const LIVE_AB_SOURCE: &str =
        "#[test] fn a() { assert!(true); }\n#[test] fn b() { assert!(true); }\n";
    const LIVE_AB_SOURCE_DIGEST: &str =
        "d0e1301f6df009789412c4459288fa4be7358d107a5908b848a0e7d0b8359381";
    const SPEC: ExternalTestSpec = ExternalTestSpec {
        package: "client",
        target: "client_lock",
        manifest: "Cargo.toml",
        source: "tests/client_lock.rs",
        default_source: "tests/client_lock.rs",
        tests: &["a", "b"],
        source_digest: LIVE_AB_SOURCE_DIGEST,
        body_digest: LIVE_AB_DIGEST,
    };
    const ONE: &[&str] = &["a"];
    const EMPTY: &[&str] = &[];

    fn manifest(extra: &str) -> String {
        format!("{BASE_MANIFEST}{extra}")
    }

    fn package(extra: &str) -> String {
        format!("[package]\nname = \"client\"\nversion = \"0.0.0\"\nedition = \"2024\"\n{extra}")
    }

    fn execute_case(
        expected: &'static [&'static str],
        listed: (bool, &str),
        executed: (bool, &str),
    ) -> bool {
        let spec = ExternalTestSpec {
            tests: expected,
            source_digest: LIVE_AB_SOURCE_DIGEST,
            ..SPEC
        };
        let mut calls = 0;
        execute(
            |args| {
                calls += 1;
                let (status, output) = if args.last().copied() == Some("--list") {
                    listed
                } else {
                    executed
                };
                Some((status, output.as_bytes().to_vec()))
            },
            &spec,
        ) && calls == 2
    }

    #[test]
    fn manifest_controls_accept_live_default_and_explicit_forms() {
        let live =
            fs::read_to_string(crate::root().join("crates/boxology-workspace/Cargo.toml")).unwrap();
        let live_spec = crate::SURFACE_LOCK_SPEC;
        assert!(manifest_is_exact(&live, &live_spec));
        assert!(manifest_is_exact(BASE_MANIFEST, &SPEC));
        for fields in [
            "name = \"client_lock\"\n",
            "name = \"client_lock\"\ntest = true\nharness = true\n",
            "name = \"client_lock\"\npath = \"tests/client_lock.rs\"\n",
        ] {
            assert!(manifest_is_exact(
                &manifest(&format!("[[test]]\n{fields}")),
                &SPEC
            ));
        }
    }

    #[test]
    fn manifest_mutants_forbid_flags_targets_and_malformed_tables() {
        for flag in [
            "autolib",
            "autobins",
            "autoexamples",
            "autotests",
            "autobenches",
        ] {
            assert!(!manifest_is_exact(
                &package(&format!("{flag} = false\n")),
                &SPEC
            ));
            assert!(!manifest_is_exact(
                &package(&format!("{flag} = \"true\"\n")),
                &SPEC
            ));
        }
        assert!(!manifest_is_exact(
            &package("build = \"build.rs\"\n"),
            &SPEC
        ));
        for target in ["lib", "bin", "example", "bench"] {
            assert!(!manifest_is_exact(
                &manifest(&format!("[{target}]\npath = \"src/escape.rs\"\n")),
                &SPEC
            ));
            assert!(!manifest_is_exact(
                &manifest(&format!("[[{target}]]\nname = \"escape\"\n")),
                &SPEC
            ));
        }
        for mutant in [
            "[[test]]\nname = \"wrong\"\n",
            "[[test]]\nname = 7\n",
            "[[test]]\ntest = true\n",
            "[[test]]\nname = \"client_lock\"\ntest = false\n",
            "[[test]]\nname = \"client_lock\"\ntest = \"true\"\n",
            "[[test]]\nname = \"client_lock\"\nharness = false\n",
            "[[test]]\nname = \"client_lock\"\nharness = []\n",
            "[[test]]\nname = \"client_lock\"\npath = 7\n",
            "[[test]]\nname = \"client_lock\"\npath = \"tests/other.rs\"\n",
            "[[test]]\nname = \"client_lock\"\nrequired-features = []\n",
            "[[test]]\nname = \"client_lock\"\nextra = true\n",
            "[[test]]\nname = \"client_lock\"\n[[test]]\nname = \"client_lock\"\n",
            "[test]\nname = \"client_lock\"\n",
        ] {
            assert!(
                !manifest_is_exact(&manifest(mutant), &SPEC),
                "accepted: {mutant}"
            );
        }
        for mutant in [
            "[package]\nversion = \"0.0.0\"\n",
            "[package]\nname = \"wrong\"\n",
            "[package\nname = \"client\"\n",
        ] {
            assert!(!manifest_is_exact(mutant, &SPEC), "accepted: {mutant}");
        }
    }

    #[test]
    fn recording_runner_proves_list_then_run_exact_argv() {
        let root = crate::root();
        let spec = crate::SURFACE_LOCK_SPEC;
        let mut calls: Vec<Vec<String>> = Vec::new();
        let passed = require_external_tests(
            &root,
            |args| {
                calls.push(args.iter().map(ToString::to_string).collect());
                match args.last().copied() {
                    Some("--list") => Some((
                        true,
                        b"surface_and_live_evasions_are_locked: test\n".to_vec(),
                    )),
                    _ => Some((
                        true,
                        b"test surface_and_live_evasions_are_locked ... ok\n".to_vec(),
                    )),
                }
            },
            &spec,
        );
        assert!(passed.is_ok(), "{passed:?}");
        let calls: Vec<_> = calls.into_iter().map(|call| call.join("\0")).collect();
        assert_eq!(
            calls,
            vec![
                "test\0-p\0boxology-workspace\0--test\0surface_lock\0--\0--list".to_owned(),
                "test\0-p\0boxology-workspace\0--test\0surface_lock\0--\0--test-threads=1"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn execution_mutants_cannot_pass() {
        let listed = "a: test\nb: test\n";
        let executed = "test a ... ok\ntest b ... ok\n";
        assert!(execute_case(SPEC.tests, (true, listed), (true, executed)));
        #[rustfmt::skip]
        let cases = [(false, listed, true, executed), (true, "", true, executed),
            (true, "wrong: test\nb: test\n", true, "test wrong ... ok\ntest b ... ok\n"),
            (true, "a: test\nb: test\nc: test\n", true, "test a ... ok\ntest b ... ok\ntest c ... ok\n"),
            (true, "a: test\n", true, "test a ... ok\n"), (true, listed, true, "test a ... ignored\ntest b ... ok\n"),
            (true, "a: test\na: test\nb: test\n", true, "test a ... ok\ntest a ... ok\ntest b ... ok\n"),
            (true, "b: test\na: test\n", true, "test b ... ok\ntest a ... ok\n"), (true, listed, false, executed)];
        for case in cases {
            assert!(!execute_case(
                SPEC.tests,
                (case.0, case.1),
                (case.2, case.3)
            ));
        }
        assert!(!execute_case(ONE, (true, listed), (true, executed)));
        assert!(!execute_case(EMPTY, (true, ""), (true, "")));
        let spec = ExternalTestSpec {
            tests: SPEC.tests,
            ..SPEC
        };
        assert!(!execute(|_| None, &spec));
    }

    #[cfg(unix)]
    #[test]
    fn regular_paths_and_build_scripts_are_checked_on_disk() {
        use std::os::unix::fs::symlink;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("xtask-external-test-{unique}"));
        fs::create_dir_all(root.join("real/tests")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("Cargo.toml"), BASE_MANIFEST).unwrap();
        fs::write(root.join(SPEC.source), LIVE_AB_SOURCE).unwrap();
        fs::write(root.join("real/tests/test.rs"), "").unwrap();
        symlink(root.join("real"), root.join("link-dir")).unwrap();
        symlink(root.join("real/tests/test.rs"), root.join("link-file")).unwrap();
        assert!(regular_file(&root, "real/tests/test.rs"));
        assert!(!regular_file(&root, "link-dir/tests/test.rs"));
        assert!(!regular_file(&root, "link-file"));
        for symlinked in [false, true] {
            if symlinked {
                fs::write(root.join("real-build.rs"), "").unwrap();
                symlink(root.join("real-build.rs"), root.join("build.rs")).unwrap();
            } else {
                fs::write(root.join("build.rs"), "").unwrap();
            }
            let mut calls = 0;
            assert!(
                require_external_tests(
                    &root,
                    |_| {
                        calls += 1;
                        None
                    },
                    &SPEC
                )
                .is_err()
            );
            assert_eq!(calls, 0);
            fs::remove_file(root.join("build.rs")).unwrap();
            if symlinked {
                fs::remove_file(root.join("real-build.rs")).unwrap();
            }
        }
        let mut calls = 0;
        assert!(
            require_external_tests(
                &root,
                |args| {
                    calls += 1;
                    let output: &[u8] = if args.last().copied() == Some("--list") {
                        b"a: test\nb: test\n"
                    } else {
                        b"test a ... ok\ntest b ... ok\n"
                    };
                    Some((true, output.to_vec()))
                },
                &SPEC
            )
            .is_ok()
        );
        assert_eq!(calls, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn production_binder_propagates_false_delegate() {
        let mut calls = 0;
        assert!(
            run_with_cargo(crate::root().as_path(), &crate::SURFACE_LOCK_SPEC, |_| {
                calls += 1;
                Some((false, Vec::new()))
            })
            .is_err()
        );
    }

    #[test]
    fn production_binder_and_cargo_runner_are_pinned_once() {
        let production = include_str!("external_test.rs")
            .split_once("#[cfg(test)]")
            .unwrap()
            .0;
        assert_eq!(
            production
                .match_indices("require_external_tests(root, run, spec)")
                .count(),
            1
        );
        let result = cargo(
            &crate::root(),
            &["boxology-441-deliberately-invalid-subcommand"],
        );
        assert!(matches!(result, Some((false, _))));
    }

    #[test]
    fn body_digest_rejects_vacuous_and_substituted_bodies() {
        let expected =
            body_digest("#[test] fn subject() { assert!(true); }\n", &["subject"]).unwrap();
        for (source, label) in [
            ("#[test] fn subject() {}\n", "empty body"),
            (
                "#[test] fn subject() {\n// verify the inventory\n}\n",
                "comment-only body",
            ),
            (
                "#[test] fn subject() { let _ = helper(); }\nfn helper() {}\n",
                "discard-only body",
            ),
            (
                "#[test] fn subject() { println!(\"inventory checked\"); }\n",
                "println-only body",
            ),
            (
                "#[test] fn subject() { Some(()).unwrap(); }\n",
                "unwrap-only body",
            ),
            (
                "#[test] fn subject() { assert!(true); assert!(true); }\n",
                "assert!(true) substitution",
            ),
        ] {
            let err = bodies_match_digest(source, &["subject"], &expected).unwrap_err();
            assert!(
                err.contains("body digest mismatch")
                    && err.contains(&format!("expected {expected}"))
                    && err.contains("observed "),
                "mutation survived: {label}; err={err}"
            );
        }
    }

    #[test]
    fn body_digest_includes_macro_token_helpers() {
        let source = concat!(
            "#[test] fn subject() { assert!(helper()); assert_eq!(helper(), true); }\n",
            "fn helper() { true }\n",
        );
        let expected = body_digest(source, &["subject"]).unwrap();
        assert!(bodies_match_digest(source, &["subject"], &expected).is_ok());
        let err = bodies_match_digest(
            concat!(
                "#[test] fn subject() { assert!(helper()); assert_eq!(helper(), true); }\n",
                "fn helper() { false }\n",
            ),
            &["subject"],
            &expected,
        )
        .unwrap_err();
        assert!(
            err.contains("body digest mismatch"),
            "mutation survived: helper only referenced in assert!/assert_eq! tokens; err={err}"
        );
    }

    #[test]
    fn body_digest_includes_transitive_helpers() {
        let source = concat!(
            "#[test] fn subject() { helper(); }\n",
            "fn helper() { deeper(); }\n",
            "fn deeper() { assert!(true); }\n",
        );
        let expected = body_digest(source, &["subject"]).unwrap();
        assert!(bodies_match_digest(source, &["subject"], &expected).is_ok());
        assert!(
            bodies_match_digest(
                concat!(
                    "#[test] fn subject() { helper(); }\n",
                    "fn helper() { deeper(); }\n",
                    "fn deeper() { assert!(false); }\n",
                ),
                &["subject"],
                &expected
            )
            .is_err(),
            "mutation survived: deeper helper body must change the digest"
        );
        assert!(
            bodies_match_digest(
                concat!(
                    "#[test] fn subject() { helper(); }\n",
                    "fn helper() { deeper(); }\n",
                    "fn deeper() {}\n",
                ),
                &["subject"],
                &expected
            )
            .is_err(),
            "mutation survived: emptying deeper helper must change the digest"
        );
    }

    #[test]
    fn body_digest_terminates_reference_cycles() {
        let source = concat!(
            "#[test] fn subject() { a(); }\n",
            "fn a() { b(); }\n",
            "fn b() { a(); }\n",
        );
        let expected = body_digest(source, &["subject"]).unwrap();
        assert!(bodies_match_digest(source, &["subject"], &expected).is_ok());
        assert_eq!(body_digest(source, &["subject"]).unwrap(), expected);
        assert!(
            bodies_match_digest(
                concat!(
                    "#[test] fn subject() { a(); }\n",
                    "fn a() { b(); }\n",
                    "fn b() { a(); assert!(true); }\n",
                ),
                &["subject"],
                &expected
            )
            .is_err(),
            "mutation survived: cycle member body must remain hashed"
        );
    }

    #[test]
    fn body_digest_includes_referenced_const_chains() {
        let source = concat!(
            "const A: i32 = B;\n",
            "const B: i32 = 1;\n",
            "fn helper() { let _ = A; }\n",
            "#[test] fn subject() { helper(); }\n",
        );
        let expected = body_digest(source, &["subject"]).unwrap();
        assert!(bodies_match_digest(source, &["subject"], &expected).is_ok());
        for (mutant, label) in [
            (
                concat!(
                    "const A: i32 = B;\n",
                    "const B: i32 = 2;\n",
                    "fn helper() { let _ = A; }\n",
                    "#[test] fn subject() { helper(); }\n",
                ),
                "leaf const",
            ),
            (
                concat!(
                    "const A: i32 = 1;\n",
                    "const B: i32 = 1;\n",
                    "fn helper() { let _ = A; }\n",
                    "#[test] fn subject() { helper(); }\n",
                ),
                "const chain edge",
            ),
            (
                concat!(
                    "const A: i32 = B;\n",
                    "const B: i32 = 1;\n",
                    "fn helper() { let _ = B; }\n",
                    "#[test] fn subject() { helper(); }\n",
                ),
                "helper body",
            ),
        ] {
            let err = bodies_match_digest(mutant, &["subject"], &expected).unwrap_err();
            assert!(
                err.contains("body digest mismatch"),
                "mutation survived: {label}; err={err}"
            );
        }
    }

    #[test]
    fn body_digest_includes_referenced_static_initializers() {
        let source = concat!(
            "static S: i32 = 1;\n",
            "#[test] fn subject() { assert_eq!(S, 1); }\n",
        );
        let expected = body_digest(source, &["subject"]).unwrap();
        assert!(bodies_match_digest(source, &["subject"], &expected).is_ok());
        let err = bodies_match_digest(
            concat!(
                "static S: i32 = 2;\n",
                "#[test] fn subject() { assert_eq!(S, 1); }\n",
            ),
            &["subject"],
            &expected,
        )
        .unwrap_err();
        assert!(
            err.contains("body digest mismatch"),
            "mutation survived: static initializer; err={err}"
        );
    }

    #[test]
    fn body_digest_ignores_unreferenced_file_level_consts() {
        let source = concat!(
            "const USED: i32 = 1;\n",
            "const UNUSED: i32 = 2;\n",
            "#[test] fn subject() { assert_eq!(USED, 1); }\n",
        );
        let expected = body_digest(source, &["subject"]).unwrap();
        assert!(
            bodies_match_digest(
                concat!(
                    "const USED: i32 = 1;\n",
                    "const UNUSED: i32 = 99;\n",
                    "#[test] fn subject() { assert_eq!(USED, 1); }\n",
                ),
                &["subject"],
                &expected
            )
            .is_ok(),
            "mutation survived: unused const must not affect digest"
        );
        assert!(
            bodies_match_digest(
                concat!(
                    "const USED: i32 = 3;\n",
                    "const UNUSED: i32 = 2;\n",
                    "#[test] fn subject() { assert_eq!(USED, 1); }\n",
                ),
                &["subject"],
                &expected
            )
            .is_err(),
            "mutation survived: referenced const must affect digest"
        );
    }

    #[test]
    fn duplicate_names_and_non_test_anchors_fail_closed() {
        assert!(body_digest(
            "#[test] fn subject() { assert!(true); }\n#[test] fn subject() { assert!(true); }\n",
            &["subject"],
        )
        .is_err());
        assert!(body_digest("fn subject() { assert!(true); }\n", &["subject"]).is_err());
        assert!(body_digest("not rust", &["subject"]).is_err());
        let empty = body_digest("not rust", &[]).unwrap_err();
        assert!(
            empty.contains("empty tests list"),
            "empty tests must fail before parse; err={empty}"
        );
        assert!(body_digest("#[test] fn other() { assert!(true); }\n", &["subject"]).is_err());
        assert!(
            body_digest(
                concat!(
                    "#[test] fn subject() { helper(); }\n",
                    "fn helper() { assert!(true); }\n",
                    "fn other() { fn visit() {}\n}\n",
                    "fn another() { fn visit() {}\n}\n",
                ),
                &["subject"],
            )
            .is_ok()
        );
        assert!(
            body_digest(
                concat!(
                    "#[test] fn subject() { helper(); }\n",
                    "fn helper() { assert!(true); }\n",
                    "fn helper() { assert!(true); }\n",
                ),
                &["subject"],
            )
            .is_err(),
            "mutation survived: ambiguous referenced fn must fail closed"
        );
        assert!(
            body_digest(
                concat!(
                    "const helper: i32 = 1;\n",
                    "fn helper() { assert!(true); }\n",
                    "#[test] fn subject() { helper(); }\n",
                ),
                &["subject"],
            )
            .is_err(),
            "mutation survived: ambiguous fn/const definition must fail closed"
        );
    }

    #[test]
    fn live_consumer_sources_match_pinned_body_digests() {
        let root = crate::root();
        for spec in [
            &crate::SURFACE_LOCK_SPEC,
            &crate::CLASSIFIER_SURFACE_LOCK_SPEC,
            &crate::GENERATOR_SOURCE_INVENTORY_LOCK_SPEC,
            &crate::BORN_VALID_SPEC,
        ] {
            let source_bytes = fs::read(root.join(spec.source)).unwrap();
            if let Err(error) = source_matches_digest(&source_bytes, spec.source_digest) {
                panic!("anchor: live consumer source {}: {error}", spec.package);
            }
            let source = std::str::from_utf8(&source_bytes).unwrap();
            if let Err(error) = bodies_match_digest(source, spec.tests, spec.body_digest) {
                panic!("anchor: live consumer {}: {error}", spec.package);
            }
            let wrong = "0".repeat(64);
            let err = bodies_match_digest(source, spec.tests, &wrong).unwrap_err();
            assert!(
                err.contains("body digest mismatch")
                    && err.contains(&format!("expected {wrong}"))
                    && err.contains("observed "),
                "anchor: {} pin names expected vs observed digests; err={err}",
                spec.package
            );
        }
    }

    #[test]
    fn source_digest_rejects_one_byte_change() {
        let digest = format!("{:x}", Sha256::digest(LIVE_AB_SOURCE.as_bytes()));
        assert_eq!(digest, LIVE_AB_SOURCE_DIGEST);
        source_matches_digest(LIVE_AB_SOURCE.as_bytes(), &digest).unwrap();
        let mut mutant = LIVE_AB_SOURCE.as_bytes().to_vec();
        mutant[0] ^= 0x01;
        let err = source_matches_digest(&mutant, &digest).unwrap_err();
        assert!(
            err.contains("source digest mismatch")
                && err.contains(&format!("expected {digest}"))
                && err.contains("observed "),
            "mutation survived: one-byte source change; err={err}"
        );
    }

    #[test]
    fn vacuous_listed_bodies_fail_the_gate_conjunction() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("xtask-vacuity-wiring-{unique}"));
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("Cargo.toml"), BASE_MANIFEST).unwrap();
        let vacuous = "#[test] fn a() {}\n#[test] fn b() {}\n";
        fs::write(root.join(SPEC.source), vacuous).unwrap();
        let source_digest =
            Box::leak(format!("{:x}", Sha256::digest(vacuous.as_bytes())).into_boxed_str());
        let spec = ExternalTestSpec {
            source_digest,
            ..SPEC
        };
        let mut calls = 0;
        let passed = require_external_tests(
            &root,
            |args| {
                calls += 1;
                let output: &[u8] = if args.last().copied() == Some("--list") {
                    b"a: test\nb: test\n"
                } else {
                    b"test a ... ok\ntest b ... ok\n"
                };
                Some((true, output.to_vec()))
            },
            &spec,
        );
        fs::remove_dir_all(root).unwrap();
        let err = passed.unwrap_err();
        assert!(
            err.contains("body digest mismatch"),
            "mutation survived: vacuous body must fail require_external_tests; err={err}"
        );
        assert_eq!(calls, 0, "anchor: body digest short-circuits execute");
    }
}
