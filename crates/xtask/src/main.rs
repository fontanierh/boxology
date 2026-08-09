use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

mod advisories;
mod budget;
mod classifier_subject;
mod deny;
pub mod determinism;
mod determinism_compare;
mod determinism_cross;
#[cfg(test)]
mod determinism_meta;
mod determinism_publish;
mod determinism_run;
mod determinism_verify;
mod external_test;
mod fixture_projects;
mod generated_project_subject;
mod generator_model_subject;
mod links;
mod records;
#[cfg(test)]
mod scratch_test;
mod skill_audit;
mod workspace_subject;

const EDITOR_FIXTURE: &str = "crates/fixtures/hello/implementation";
const EDITOR_CHECK_ARGS: &[&str] = &[
    "analysis-stats",
    "--disable-build-scripts",
    "--disable-proc-macros",
    "--no-test",
    EDITOR_FIXTURE,
];
const SURFACE_LOCK_SPEC: external_test::ExternalTestSpec = external_test::ExternalTestSpec {
    package: "boxology-workspace",
    target: "surface_lock",
    manifest: "crates/boxology-workspace/Cargo.toml",
    source: "crates/boxology-workspace/tests/surface_lock.rs",
    default_source: "tests/surface_lock.rs",
    tests: &["surface_and_live_evasions_are_locked"],
    source_digest: "851bc809ce185a49fb0ef6b6b5758269c2ae559bd7f363ed1288c86883780eea",
    body_digest: "3daaf29c01df87b82990aaeeaa74b8856f85e1ff8a984992910393b3528b60d9",
};
const CLASSIFIER_SURFACE_LOCK_SPEC: external_test::ExternalTestSpec =
    external_test::ExternalTestSpec {
        package: "boxology-classifier",
        target: "surface_lock",
        manifest: "crates/boxology-classifier/Cargo.toml",
        source: "crates/boxology-classifier/tests/surface_lock.rs",
        default_source: "tests/surface_lock.rs",
        tests: &["surface_and_live_evasions_are_locked"],
        source_digest: "b633faf32525a7b4883e9f7f07c77a0738f199797927240d71abd32618f59dd3",
        body_digest: "b010c6eb43ce00b40f6dd11c3aa63c1f62ad8c2e3196f8e7e1e4ffd10331e65a",
    };
// #107A closed generator source-surface and effect authority. PR-required. Transitive: #358.
const GENERATOR_SOURCE_INVENTORY_TESTS: &[&str] = &[
    "closure_rules_reject_live_hostile_corpus",
    "effect_scan_allows_positive_controls",
    "effect_scan_rejects_hostile_corpus",
    "generator_crate_roots_admit_only_manifest_src_and_tests",
    "generator_manifests_are_closed_and_pin_exact_dependencies",
    "generator_test_trees_are_closed_and_inventoried",
    "production_source_inventory_is_exact",
    "production_sources_pass_effect_scan",
];
const GENERATOR_SOURCE_INVENTORY_LOCK_SPEC: external_test::ExternalTestSpec =
    external_test::ExternalTestSpec {
        package: "boxology-generator-model",
        target: "purity_lock",
        manifest: "crates/boxology-generator-model/Cargo.toml",
        source: "crates/boxology-generator-model/tests/purity_lock.rs",
        default_source: "tests/purity_lock.rs",
        tests: GENERATOR_SOURCE_INVENTORY_TESTS,
        source_digest: "4e032f8b6046a98ed9477057313f659faae539c236dd6285ec75486178f2a5b7",
        body_digest: "703b85e771149ae68064568bfa7a453962d608ae49edc165df2bf4ee96bc26c0",
    };
const BORN_VALID_SPEC: external_test::ExternalTestSpec = external_test::ExternalTestSpec {
    package: "boxology-init",
    target: "born_valid",
    manifest: "crates/boxology-init/Cargo.toml",
    source: "crates/boxology-init/tests/born_valid.rs",
    default_source: "tests/born_valid.rs",
    tests: &["initialized_project_is_born_valid_and_regeneration_is_a_no_op"],
    source_digest: "0768cf6784ed0ec519b952fc63b4a5f64cfe925aa91eff70e5c468799ca6a5f4",
    body_digest: "1a5399cc443c1636c531208924e739258237fb2fa2121bc3806bce8cd4216deb",
};
const EXTERNAL_TEST_SPECS: &[(&str, &external_test::ExternalTestSpec)] = &[
    ("surface-lock", &SURFACE_LOCK_SPEC),
    ("classifier-surface-lock", &CLASSIFIER_SURFACE_LOCK_SPEC),
    (
        "generator-source-inventory",
        &GENERATOR_SOURCE_INVENTORY_LOCK_SPEC,
    ),
    ("born-valid", &BORN_VALID_SPEC),
];
type ExternalTestRunner<'a> = dyn FnMut(&[&str]) -> Option<(bool, Vec<u8>)> + 'a;
type ProductRunner<'a> = dyn FnMut(&[&str]) -> bool + 'a;
type SkillCommand = (&'static str, fn(&Path) -> u8);
const SKILL_COMMANDS: &[SkillCommand] = &[("skill-audit", skill_audit::run)];
const CI_SKILL_AUDITS: &[fn(&Path) -> bool] = &[run_skill_audit_ci];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CiTier {
    PullRequest,
    Hygiene,
    Deep,
}

/// Exact check-name set for `cargo xtask ci-hygiene --base <revision>`.
const HYGIENE_CHECKS: &[&str] = &[
    "audit",
    "fmt",
    "key-order",
    "whitespace",
    "links",
    "records",
    "budget",
];
const PR_CHECKS: &[&str] = &[
    "product-boxology-check",
    "audit",
    "fixture-projects",
    "generated-style-fmt",
    "surface-lock",
    "classifier-surface-lock",
    "generator-source-inventory",
    "born-valid",
    "whitespace",
    "links",
    "records",
    "deny",
    "determinism",
    "budget",
];
const DEEP_CHECKS: &[&str] = &[
    "product-boxology-check",
    "audit",
    "fixture-projects",
    "generated-style-fmt",
    "repo-editor",
    "repo-generator-deep-tests",
    "repo-doc",
    "surface-lock",
    "classifier-surface-lock",
    "generator-source-inventory",
    "born-valid",
    "whitespace",
    "links",
    "records",
    "deny",
    "determinism",
];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    ExitCode::from(dispatch(&args, &root()))
}

fn dispatch(args: &[String], audit_root: &Path) -> u8 {
    if let Some(code) = registered_skill_command(args, audit_root) {
        return code;
    }
    match args {
        [command, flag] if command == "ci" && flag == "--no-budget" => run_ci(None),
        [command, flag, base]
            if command == "ci"
                && flag == "--base"
                && !base.is_empty()
                && !base.starts_with('-') =>
        {
            run_ci(Some(base))
        }
        [command, flag, base]
            if command == "ci-hygiene"
                && flag == "--base"
                && !base.is_empty()
                && !base.starts_with('-') =>
        {
            run_ci_hygiene(base)
        }
        [command] if command == "ci-fixtures" => run_ci_fixtures(),
        [command, flag, base]
            if command == "budget"
                && flag == "--base"
                && !base.is_empty()
                && !base.starts_with('-') =>
        {
            budget::run(&root(), base)
        }
        [command] if command == "determinism" => determinism_run::local(&root()),
        [command, flag, out]
            if command == "determinism-manifest"
                && flag == "--out"
                && !out.is_empty()
                && !out.starts_with('-') =>
        {
            determinism_run::manifest(&root(), Path::new(out))
        }
        [command, flag, out, mode]
            if command == "determinism-manifest"
                && flag == "--out"
                && !out.is_empty()
                && !out.starts_with('-')
                && mode == "--meta-cross" =>
        {
            determinism_cross::manifest(&root(), Path::new(out))
        }
        [command, rest @ ..] if command == "determinism-compare" => {
            determinism_compare::from_args(rest).unwrap_or_else(|| {
                usage();
                2
            })
        }
        [command, rest @ ..] if command == "determinism-meta-cross" => {
            determinism_cross::from_args(rest).unwrap_or_else(|| {
                usage();
                2
            })
        }
        [command, rest @ ..] if command == "determinism-verify" => {
            determinism_verify::from_args(rest).unwrap_or_else(|| {
                eprintln!("determinism-verify: ERROR: invalid arguments");
                usage();
                2
            })
        }
        [command, rest @ ..] if command == "subject-run" => determinism_run::child_from_args(rest)
            .unwrap_or_else(|| {
                usage();
                2
            }),
        [command] if command == "links" => run_links(),
        [command] if command == "records" => records::run(&root(), None),
        [command, flag, base]
            if command == "records"
                && flag == "--base"
                && !base.is_empty()
                && !base.starts_with('-') =>
        {
            records::run(&root(), Some(base))
        }
        [command] if command == "deny" => deny::run(&root()),
        [command, flag, repo] if command == "advisories" && flag == "--repo" => {
            advisories::run(&root(), repo, None)
        }
        [command, repo_flag, repo, simulate_flag, advisory]
            if command == "advisories"
                && repo_flag == "--repo"
                && simulate_flag == "--simulate" =>
        {
            advisories::run(&root(), repo, Some(advisory))
        }
        [command] if command == "test" => run_test(),
        _ => {
            usage();
            2
        }
    }
}

fn usage() {
    eprintln!(
        "usage: cargo xtask advisories --repo <owner/repo> [--simulate <RUSTSEC-id>]\n       cargo xtask ci (--base <revision> | --no-budget)\n       cargo xtask ci-hygiene --base <revision>\n       cargo xtask ci-fixtures\n       cargo xtask budget --base <revision>\n       cargo xtask deny\n       cargo xtask determinism\n       cargo xtask determinism-manifest --out <directory>\n       cargo xtask determinism-manifest --out <directory> --meta-cross\n       cargo xtask determinism-compare <a> <b>\n       cargo xtask determinism-meta-cross <linux> <macos>\n       cargo xtask determinism-verify <directory> --target <triple> [--require-image]\n       cargo xtask skill-audit\n       cargo xtask links\n       cargo xtask records [--base <revision>]\n       cargo xtask test\n       cargo xtask subject-run <name> --out <directory>  (internal)"
    );
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn external_test_checks(
    root: &Path,
    run: &mut ExternalTestRunner<'_>,
) -> Vec<(&'static str, bool)> {
    EXTERNAL_TEST_SPECS
        .iter()
        .map(|(name, spec)| {
            (
                *name,
                timed(name, || {
                    match external_test::run_with_cargo(root, spec, |args| run(args)) {
                        Ok(()) => true,
                        Err(error) => {
                            eprintln!("{name}: {error}");
                            false
                        }
                    }
                }),
            )
        })
        .collect()
}

/// Missing tier-required check names, then false verdicts.
fn ci_failure_names(tier: CiTier, checks: &[(&'static str, bool)]) -> Vec<&'static str> {
    let mut failed = Vec::new();
    let required = match tier {
        CiTier::Hygiene => HYGIENE_CHECKS,
        CiTier::PullRequest => PR_CHECKS,
        CiTier::Deep => DEEP_CHECKS,
    };
    for &name in required {
        if checks
            .iter()
            .filter(|(candidate, _)| candidate == &name)
            .count()
            != 1
        {
            failed.push(name);
        }
    }
    for &(name, _) in checks {
        if !required.contains(&name) {
            failed.push(name);
        }
    }
    failed.extend(checks.iter().filter_map(|(n, ok)| (!*ok).then_some(*n)));
    failed
}

fn run_ci(base: Option<&str>) -> u8 {
    let deep = base.is_none();
    let tier = if deep {
        CiTier::Deep
    } else {
        CiTier::PullRequest
    };
    let toolchain = timed("toolchain", check_toolchain);
    if let Err(error) = toolchain {
        eprintln!("toolchain: FAIL: {error}");
        eprintln!("summary: FAIL (toolchain)");
        return 1;
    }
    println!("toolchain: PASS");

    println!("test-tier: {}", if deep { "deep" } else { "pull-request" });
    let mut checks = vec![
        (
            "product-boxology-check",
            timed("product-boxology-check", || {
                run_product_check(base, &mut |args| run_cargo(args))
            }),
        ),
        (
            "audit",
            timed("audit", || registered_ci_skill_audits(&root())),
        ),
        (
            "fixture-projects",
            timed("fixture-projects", || fixture_projects::run(&root(), deep)),
        ),
        (
            "generated-style-fmt",
            timed("generated-style-fmt", || {
                fixture_projects::generated_style_fails_fmt(&root())
            }),
        ),
    ];
    if deep {
        checks.extend([
            ("repo-editor", timed("repo-editor", run_editor)),
            (
                "repo-generator-deep-tests",
                timed("repo-generator-deep-tests", || {
                    run_cargo(&[
                        "test",
                        "-p",
                        "boxology-generator",
                        "--all-features",
                        "--",
                        "--ignored",
                        "--test-threads=1",
                    ])
                }),
            ),
            ("repo-doc", timed("repo-doc", run_doc)),
        ]);
    }
    checks.extend(external_test_checks(&root(), &mut |args| {
        external_test::cargo(&root(), args)
    }));
    checks.extend([
        ("whitespace", timed("whitespace", check_tracked_whitespace)),
        ("links", timed("links", || links::check(&root()))),
        (
            "records",
            timed("records", || records::run(&root(), base) == 0),
        ),
        ("deny", timed("deny", || deny::run(&root()) == 0)),
        (
            "determinism",
            timed("determinism", || determinism_run::local(&root()) == 0),
        ),
    ]);
    for &(name, passed) in &checks {
        println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
    }
    match base {
        Some(base) => {
            let code = timed("budget", || budget::run(&root(), base));
            checks.push(("budget", code == 0));
        }
        None => println!("budget: SKIPPED (--no-budget)"),
    }
    summarize_ci(tier, &checks)
}

fn run_ci_hygiene(base: &str) -> u8 {
    println!("test-tier: ci-hygiene");
    let mut checks = vec![
        (
            "audit",
            timed("audit", || registered_ci_skill_audits(&root())),
        ),
        ("fmt", timed("fmt", run_root_fmt)),
        ("key-order", timed("key-order", run_key_order)),
        ("whitespace", timed("whitespace", check_tracked_whitespace)),
        ("links", timed("links", || links::check(&root()))),
        (
            "records",
            timed("records", || records::run(&root(), Some(base)) == 0),
        ),
    ];
    for &(name, passed) in &checks {
        println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
    }
    let code = timed("budget", || budget::run(&root(), base));
    checks.push(("budget", code == 0));
    summarize_ci(CiTier::Hygiene, &checks)
}

fn run_ci_fixtures() -> u8 {
    let checks = [
        ("fixture-projects", fixture_projects::run(&root(), false)),
        (
            "generated-style-fmt",
            fixture_projects::generated_style_fails_fmt(&root()),
        ),
        (
            "golden-generated-project",
            run_cargo(&[
                "test",
                "-p",
                "boxology-init",
                "--locked",
                "--lib",
                "tests::golden_inventory_and_comparison_fail_closed",
                "--",
                "--exact",
            ]),
        ),
    ];
    for (name, passed) in checks {
        println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
    }
    u8::from(checks.iter().any(|(_, passed)| !passed))
}

fn summarize_ci(tier: CiTier, checks: &[(&'static str, bool)]) -> u8 {
    let failed = ci_failure_names(tier, checks);
    if failed.is_empty() {
        println!("summary: PASS");
        0
    } else {
        eprintln!("summary: FAIL ({})", failed.join(", "));
        1
    }
}

fn product_check_args(base: Option<&str>) -> Vec<&str> {
    let mut args = vec![
        "run",
        "--locked",
        "-q",
        "-p",
        "boxology-cli",
        "--bin",
        "boxology",
        "--",
        "check",
    ];
    if let Some(base) = base {
        args.extend(["--base", base]);
    }
    args
}

fn run_product_check(base: Option<&str>, runner: &mut ProductRunner<'_>) -> bool {
    runner(&product_check_args(base))
}

// Production control-plane anchors; protected review must prevent their test-only displacement.
fn registered_skill_command(args: &[String], root: &Path) -> Option<u8> {
    let [command] = args else { return None };
    SKILL_COMMANDS
        .iter()
        .find_map(|(name, run)| (*name == command).then(|| run(root)))
}

fn registered_ci_skill_audits(root: &Path) -> bool {
    !CI_SKILL_AUDITS.is_empty() && CI_SKILL_AUDITS.iter().all(|audit| audit(root))
}

fn run_skill_audit_ci(audit_root: &Path) -> bool {
    skill_audit::run(audit_root) == 0
}

fn timed<T>(name: &str, check: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let result = check();
    println!("{name}: elapsed_ms={}", started.elapsed().as_millis());
    result
}

fn run_links() -> u8 {
    let passed = links::check(&root());
    println!("links: {}", if passed { "PASS" } else { "FAIL" });
    u8::from(!passed)
}

fn check_toolchain() -> Result<(), String> {
    let text = fs::read_to_string(root().join("rust-toolchain.toml"))
        .map_err(|error| format!("cannot read rust-toolchain.toml: {error}"))?;
    let channel = parse_channel(&text)?;
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot run rustc --version: {error}"))?;
    if !output.status.success() {
        return Err(format!("rustc --version exited with {}", output.status));
    }
    compare_rustc_version(channel, &String::from_utf8_lossy(&output.stdout))
}

fn parse_channel(text: &str) -> Result<&str, String> {
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "channel" {
            continue;
        }
        let value = value.trim();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            let channel = &value[1..value.len() - 1];
            if !channel.is_empty() {
                return Ok(channel);
            }
        }
        return Err("malformed channel in rust-toolchain.toml".into());
    }
    Err("missing channel in rust-toolchain.toml".into())
}

fn compare_rustc_version(expected: &str, output: &str) -> Result<(), String> {
    let found = output
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "malformed rustc --version output".to_string())?;
    if found == expected {
        Ok(())
    } else {
        Err(format!("expected rustc {expected}, found rustc {found}"))
    }
}

fn run_root_fmt() -> bool {
    run_cargo(&[
        "test",
        "-p",
        "boxology-cli",
        "--test",
        "runner",
        "real_root_fmt_selection_excludes_standalone_fixture_and_passes",
        "--",
        "--exact",
    ])
}

fn run_key_order() -> bool {
    run_cargo(&[
        "test",
        "-p",
        "boxology-schema",
        "--features",
        "preserve-order",
    ])
}

fn run_editor() -> bool {
    match Command::new("rust-analyzer")
        .args(EDITOR_CHECK_ARGS)
        .current_dir(root())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("rust-analyzer analysis-stats exited with {status}");
            false
        }
        Err(error) => {
            eprintln!("cannot run pinned rust-analyzer component: {error}");
            false
        }
    }
}

fn run_test() -> u8 {
    if run_cargo(&["test", "--workspace", "--all-features"]) {
        0
    } else {
        1
    }
}

fn run_doc() -> bool {
    Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .current_dir(root())
        .env("RUSTDOCFLAGS", "-D warnings")
        .status()
        .is_ok_and(|status| status.success())
}

fn run_cargo(args: &[&str]) -> bool {
    Command::new("cargo")
        .args(args)
        .current_dir(root())
        .status()
        .is_ok_and(|status| status.success())
}

fn check_tracked_whitespace() -> bool {
    let output = match Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root())
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!("git ls-files failed with {}", output.status);
            return false;
        }
        Err(error) => {
            eprintln!("cannot run git ls-files: {error}");
            return false;
        }
    };
    let mut passed = true;
    for name in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let relative = String::from_utf8_lossy(name);
        let path = root().join(relative.as_ref());
        // Tracked symlinks (`.claude` -> `.agents`) are aliases; their targets
        // are tracked and checked as their own entries.
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            eprintln!("cannot read {}", relative);
            passed = false;
            continue;
        };
        for line in trailing_whitespace_lines(&bytes) {
            eprintln!("{}:{line}: trailing space or tab", relative);
            passed = false;
        }
    }
    passed
}

fn trailing_whitespace_lines(bytes: &[u8]) -> Vec<usize> {
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Vec::new();
    }
    bytes
        .split(|byte| *byte == b'\n')
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            matches!(line.last(), Some(b' ' | b'\t')).then_some(index + 1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolchain_parser_and_comparison_are_exact() {
        assert_eq!(
            parse_channel("[toolchain]\nchannel = \"1.97.1\""),
            Ok("1.97.1")
        );
        assert!(compare_rustc_version("1.97.1", "rustc 1.97.1 (hash date)").is_ok());
        assert!(compare_rustc_version("1.97.1", "rustc 1.97.0 (hash date)").is_err());
        assert!(compare_rustc_version("1.97.1", "not rustc").is_err());
        assert!(parse_channel("channel = 1.97.1").is_err());
        assert!(parse_channel("[toolchain]").is_err());
    }

    #[test]
    fn editor_check_is_a_fixed_reproducible_batch_probe() {
        assert_eq!(
            EDITOR_CHECK_ARGS,
            &[
                "analysis-stats",
                "--disable-build-scripts",
                "--disable-proc-macros",
                "--no-test",
                EDITOR_FIXTURE,
            ]
        );
        assert!(!EDITOR_CHECK_ARGS.contains(&"--randomize"));
        assert!(!EDITOR_CHECK_ARGS.contains(&"--parallel"));
        assert!(!EDITOR_CHECK_ARGS.contains(&"--with-deps"));
        assert!(root().join(EDITOR_FIXTURE).join("Cargo.toml").is_file());
    }

    #[test]
    fn external_test_specs_are_registered_once_by_identity() {
        let expected = [
            ("surface-lock", &SURFACE_LOCK_SPEC),
            ("classifier-surface-lock", &CLASSIFIER_SURFACE_LOCK_SPEC),
            (
                "generator-source-inventory",
                &GENERATOR_SOURCE_INVENTORY_LOCK_SPEC,
            ),
            ("born-valid", &BORN_VALID_SPEC),
        ];
        assert_eq!(EXTERNAL_TEST_SPECS, expected);
        for (index, (name, spec)) in expected.iter().enumerate() {
            assert_eq!(
                EXTERNAL_TEST_SPECS[index].0, *name,
                "mutation survived: report order {name}"
            );
            let count = EXTERNAL_TEST_SPECS
                .iter()
                .filter(|(n, s)| *n == *name && *s == *spec)
                .count();
            assert_eq!(count, 1, "mutation survived: {name}");
        }
    }

    #[test]
    fn external_test_checks_consult_and_propagate() {
        let mut consultations: Vec<Vec<String>> = Vec::new();
        let results = external_test_checks(&root(), &mut |args| {
            consultations.push(args.iter().map(|arg| (*arg).to_owned()).collect());
            let package = args
                .iter()
                .position(|arg| *arg == "-p")
                .and_then(|index| args.get(index + 1))
                .copied()
                .unwrap_or("");
            let tests: &[&str] = match package {
                "boxology-workspace" | "boxology-classifier" => {
                    &["surface_and_live_evasions_are_locked"]
                }
                "boxology-generator-model" => GENERATOR_SOURCE_INVENTORY_TESTS,
                "boxology-init" => BORN_VALID_SPEC.tests,
                other => panic!("unexpected package: {other}"),
            };
            if args.last().copied() == Some("--list") {
                let listed = tests
                    .iter()
                    .map(|name| format!("{name}: test\n"))
                    .collect::<String>();
                Some((true, listed.into_bytes()))
            } else {
                let executed = tests
                    .iter()
                    .map(|name| format!("test {name} ... ok\n"))
                    .collect::<String>();
                Some((true, executed.into_bytes()))
            }
        });
        assert_eq!(results.len(), 4, "mutation survived: results.len()");
        assert!(
            results.iter().all(|(_, passed)| *passed),
            "mutation survived: live consumer consultation"
        );
        let expected_argv: &[&[&str]] = &[
            &[
                "test",
                "-p",
                "boxology-workspace",
                "--test",
                "surface_lock",
                "--",
                "--list",
            ],
            &[
                "test",
                "-p",
                "boxology-workspace",
                "--test",
                "surface_lock",
                "--",
                "--test-threads=1",
            ],
            &[
                "test",
                "-p",
                "boxology-classifier",
                "--test",
                "surface_lock",
                "--",
                "--list",
            ],
            &[
                "test",
                "-p",
                "boxology-classifier",
                "--test",
                "surface_lock",
                "--",
                "--test-threads=1",
            ],
            &[
                "test",
                "-p",
                "boxology-generator-model",
                "--test",
                "purity_lock",
                "--",
                "--list",
            ],
            &[
                "test",
                "-p",
                "boxology-generator-model",
                "--test",
                "purity_lock",
                "--",
                "--test-threads=1",
            ],
            &[
                "test",
                "-p",
                "boxology-init",
                "--test",
                "born_valid",
                "--",
                "--list",
            ],
            &[
                "test",
                "-p",
                "boxology-init",
                "--test",
                "born_valid",
                "--",
                "--test-threads=1",
            ],
        ];
        // Two argv vectors per spec: list then run.
        assert_eq!(
            consultations.len(),
            expected_argv.len(),
            "mutation survived: consultation count"
        );
        for (got, expected) in consultations.iter().zip(expected_argv.iter()) {
            assert_eq!(
                got.iter().map(String::as_str).collect::<Vec<_>>(),
                expected.to_vec(),
                "mutation survived: argv"
            );
        }
        let false_results = external_test_checks(&root(), &mut |_| Some((false, Vec::new())));
        assert_eq!(
            false_results.len(),
            4,
            "mutation survived: false-propagation length"
        );
        assert!(
            false_results.iter().all(|(_, passed)| !*passed),
            "mutation survived: false-propagation"
        );
    }

    #[test]
    fn tiers_require_exactly_one_named_product_and_every_retained_gate() {
        for (tier, names) in [
            (CiTier::Hygiene, HYGIENE_CHECKS),
            (CiTier::PullRequest, PR_CHECKS),
            (CiTier::Deep, DEEP_CHECKS),
        ] {
            let passed: Vec<_> = names.iter().map(|&name| (name, true)).collect();
            assert!(ci_failure_names(tier, &passed).is_empty(), "{tier:?}");
            let mut missing = passed.clone();
            missing.pop();
            assert_eq!(
                ci_failure_names(tier, &missing),
                vec![names[names.len() - 1]]
            );
        }

        let mut duplicate: Vec<_> = PR_CHECKS.iter().map(|&name| (name, true)).collect();
        duplicate.push(("product-boxology-check", true));
        assert_eq!(
            ci_failure_names(CiTier::PullRequest, &duplicate),
            vec!["product-boxology-check"]
        );
        let mut failed: Vec<_> = DEEP_CHECKS.iter().map(|&name| (name, true)).collect();
        failed[0].1 = false;
        assert_eq!(
            ci_failure_names(CiTier::Deep, &failed),
            vec!["product-boxology-check"]
        );
    }

    #[test]
    fn product_check_argv_and_failure_propagation_are_exact() {
        assert_eq!(
            product_check_args(Some("base-sha")),
            [
                "run",
                "--locked",
                "-q",
                "-p",
                "boxology-cli",
                "--bin",
                "boxology",
                "--",
                "check",
                "--base",
                "base-sha"
            ]
        );
        assert_eq!(
            product_check_args(None),
            [
                "run",
                "--locked",
                "-q",
                "-p",
                "boxology-cli",
                "--bin",
                "boxology",
                "--",
                "check"
            ]
        );
        let mut calls = 0;
        assert!(!run_product_check(Some("base"), &mut |_| {
            calls += 1;
            false
        }));
        assert_eq!(calls, 1);
    }

    #[test]
    fn ci_hygiene_dispatch_rejects_missing_empty_and_flag_shaped_bases() {
        let root = root();
        for args in [
            vec!["ci-hygiene".to_owned()],
            vec!["ci-hygiene".to_owned(), "--base".to_owned()],
            vec!["ci-hygiene".to_owned(), "--base".to_owned(), String::new()],
            vec![
                "ci-hygiene".to_owned(),
                "--base".to_owned(),
                "--no-budget".to_owned(),
            ],
            vec!["ci-hygiene".to_owned(), "HEAD".to_owned()],
        ] {
            assert_eq!(dispatch(&args, &root), 2, "{args:?}");
        }
    }

    #[test]
    fn workflows_keep_product_dispatch_only_and_preserve_scoped_pr_gates() {
        let workflow = include_str!("../../../.github/workflows/pr.yml");
        let scope = workflow
            .split_once("- name: Select test scope")
            .expect("Select test scope step")
            .1
            .split_once("- name: Test repository invariants")
            .expect("end of Select test scope")
            .0;
        assert!(
            scope.contains(r"\.md$"),
            "Markdown-only diffs must clear the Rust/product lane"
        );
        assert!(
            scope.contains("crates/fixtures/")
                && scope.contains("goldens/generated-project/")
                && scope.contains("ops/process-reaper/"),
            "opaque fixtures and process-reaper retain broad path gates"
        );
        assert!(!workflow.contains("--bin boxology -- check"));
        assert!(!workflow.contains("cargo xtask ci --base"));
        assert!(workflow.contains("cargo xtask ci-fixtures"));
        assert!(workflow.contains("steps.scope.outputs.run_reaper == 'true'"));
        assert!(workflow.contains("cargo test -p xtask --locked"));
        assert!(workflow.contains("- name: Test changed crates"));
        assert!(workflow.contains("- name: Check workspace build graph"));
        assert!(
            scope.find("crates/fixtures/fixture-tests/*) ;;").unwrap()
                < scope
                    .find("crates/fixtures/*|goldens/generated-project/*")
                    .unwrap()
        );
        assert_eq!(
            include_str!("main.rs")
                .matches("golden_inventory_and_comparison_fail_closed")
                .count(),
            2
        );

        let deep = include_str!("../../../.github/workflows/deep-validation.yml");
        assert_eq!(deep.matches("cargo xtask ci --no-budget").count(), 1);
        assert!(!deep.contains("--bin boxology -- check"));
    }

    #[test]
    fn tracked_whitespace_check_passes_with_committed_symlink() {
        assert!(
            fs::symlink_metadata(root().join(".claude"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(check_tracked_whitespace());
    }

    #[test]
    fn whitespace_detection_handles_text_and_binary() {
        assert_eq!(
            trailing_whitespace_lines(b"clean\ntext\n"),
            Vec::<usize>::new()
        );
        assert_eq!(trailing_whitespace_lines(b"space \ntab\t\n"), vec![1, 2]);
        assert_eq!(
            trailing_whitespace_lines(b"space \0\n"),
            Vec::<usize>::new()
        );
    }
}
