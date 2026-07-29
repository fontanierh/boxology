use std::{
    fs,
    path::{Component, Path},
    process::Command,
};

use toml_edit::{DocumentMut, Item, Table};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ExternalTestSpec {
    pub(crate) package: &'static str,
    pub(crate) target: &'static str,
    pub(crate) manifest: &'static str,
    pub(crate) source: &'static str,
    pub(crate) default_source: &'static str,
    pub(crate) tests: &'static [&'static str],
}

pub(crate) fn require_external_tests(
    root: &Path,
    mut run: impl FnMut(&[&str]) -> Option<(bool, Vec<u8>)>,
    spec: &ExternalTestSpec,
) -> bool {
    if spec.tests.is_empty()
        || !regular_file(root, spec.manifest)
        || !regular_file(root, spec.source)
        || build_script_exists(root, spec.manifest)
    {
        return false;
    }
    let Ok(manifest) = fs::read_to_string(root.join(spec.manifest)) else {
        return false;
    };
    manifest_is_exact(&manifest, spec) && execute(&mut run, spec)
}

pub(crate) fn run_with_cargo(
    root: &Path,
    spec: &ExternalTestSpec,
    run: impl FnMut(&[&str]) -> Option<(bool, Vec<u8>)>,
) -> bool {
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
    let run_args = ["test", "-p", spec.package, "--test", spec.target];
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
    const SPEC: ExternalTestSpec = ExternalTestSpec {
        package: "client",
        target: "client_lock",
        manifest: "Cargo.toml",
        source: "tests/client_lock.rs",
        default_source: "tests/client_lock.rs",
        tests: &["a", "b"],
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
        assert!(passed);
        let calls: Vec<_> = calls.into_iter().map(|call| call.join("\0")).collect();
        assert_eq!(
            calls,
            vec![
                "test\0-p\0boxology-workspace\0--test\0surface_lock\0--\0--list".to_owned(),
                "test\0-p\0boxology-workspace\0--test\0surface_lock".to_owned(),
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
        fs::write(root.join(SPEC.source), "").unwrap();
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
            assert!(!require_external_tests(
                &root,
                |_| {
                    calls += 1;
                    None
                },
                &SPEC
            ));
            assert_eq!(calls, 0);
            fs::remove_file(root.join("build.rs")).unwrap();
            if symlinked {
                fs::remove_file(root.join("real-build.rs")).unwrap();
            }
        }
        let mut calls = 0;
        assert!(require_external_tests(
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
        ));
        assert_eq!(calls, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn production_binder_propagates_false_delegate() {
        let mut calls = 0;
        assert!(!run_with_cargo(
            crate::root().as_path(),
            &crate::SURFACE_LOCK_SPEC,
            |_| {
                calls += 1;
                Some((false, Vec::new()))
            }
        ));
    }

    #[test]
    fn production_binder_is_pinned_once() {
        let source = include_str!("external_test.rs");
        let binder = "require_external_tests(root, run, spec)";
        let production = source.split_once("#[cfg(test)]").unwrap().0;
        assert_eq!(production.match_indices(binder).count(), 1);
    }
}
