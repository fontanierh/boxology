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

// Bootstrap registries. S7 replaces both with manifest-derived classification (S0 D10).
// Fixture-project fmt runs in the fixture-projects gate until T5.
const OWNED_FMT_PACKAGES: &[&str] = &[
    "boxology-contract",
    "boxology-contract-syntax",
    "boxology",
    "boxology-macros",
    "boxology-fixture-tests",
    "boxology-generator-model",
    "boxology-generator-writer",
    "boxology-generator",
    "boxology-init",
    "boxology-manifest",
    "boxology-schema",
    "boxology-classifier",
    "boxology-workspace",
    "boxology-cli",
    "boxology-http",
    "boxology-http-conformance",
    "boxology-runtime",
    "boxology-telegram",
    "xtask",
];
const FMT_EXCLUDED_PACKAGES: &[&str] = &["greeter-contract", "hello-contract", "ping-contract"];
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
// #107A generator source-surface closure lock; coarse fail-closed scan, precision PR pending. PR-required. Transitive: #358.
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
        source_digest: "3359fcb86ff1890a0c358d391b8225a355a5995f97ab40bb1e11845e2e75c297",
        body_digest: "41e7ab2344e9f8e3d860bdec77784288fb9bbbbea9666535d90f1e963dbfae45",
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
];
type ExternalTestRunner<'a> = dyn FnMut(&[&str]) -> Option<(bool, Vec<u8>)> + 'a;
type SkillCommand = (&'static str, fn(&Path) -> u8);
const SKILL_COMMANDS: &[SkillCommand] = &[("skill-audit", skill_audit::run)];
const CI_SKILL_AUDITS: &[fn(&Path) -> bool] = &[run_skill_audit_ci];
const CAPSTONE_PACKAGES: &[&str] = &[
    "boxology-init",
    "boxology-generator",
    "boxology-workspace",
    "xtask",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CiTier {
    PullRequest,
    Hygiene,
    Deep,
    Capstone,
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
        [command] if command == "ci-capstone" => run_ci_capstone(),
        [command] if command == "ci-born-valid" => run_ci_born_valid(),
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
        "usage: cargo xtask advisories --repo <owner/repo> [--simulate <RUSTSEC-id>]\n       cargo xtask ci (--base <revision> | --no-budget)\n       cargo xtask ci-hygiene --base <revision>\n       cargo xtask ci-capstone\n       cargo xtask ci-born-valid\n       cargo xtask budget --base <revision>\n       cargo xtask deny\n       cargo xtask determinism\n       cargo xtask determinism-manifest --out <directory>\n       cargo xtask determinism-manifest --out <directory> --meta-cross\n       cargo xtask determinism-compare <a> <b>\n       cargo xtask determinism-meta-cross <linux> <macos>\n       cargo xtask determinism-verify <directory> --target <triple> [--require-image]\n       cargo xtask skill-audit\n       cargo xtask links\n       cargo xtask records [--base <revision>]\n       cargo xtask test\n       cargo xtask subject-run <name> --out <directory>  (internal)"
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
    match tier {
        CiTier::Hygiene => {
            for &required in HYGIENE_CHECKS {
                if !checks.iter().any(|(name, _)| *name == required) {
                    failed.push(required);
                }
            }
            for &(name, _) in checks {
                if !HYGIENE_CHECKS.contains(&name) {
                    failed.push(name);
                }
            }
        }
        CiTier::Capstone => {
            if !checks.iter().any(|(name, _)| *name == "fixture-projects") {
                failed.push("fixture-projects");
            }
            failed.extend(
                EXTERNAL_TEST_SPECS.iter().filter_map(|(name, _)| {
                    (!checks.iter().any(|(n, _)| n == name)).then_some(*name)
                }),
            );
        }
        CiTier::Deep => {
            failed.extend(
                EXTERNAL_TEST_SPECS.iter().filter_map(|(name, _)| {
                    (!checks.iter().any(|(n, _)| n == name)).then_some(*name)
                }),
            );
        }
        CiTier::PullRequest => {
            if !checks
                .iter()
                .any(|(n, _)| *n == "generator-source-inventory")
            {
                failed.push("generator-source-inventory");
            }
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

    if !deep {
        println!(
            "test-tier: PR requires generator-source-inventory; delegates boxology-init, \
             boxology-generator, boxology-workspace, xtask, fixture-projects, and other external gates to macos-capstone"
        );
    }
    let mut checks = vec![
        (
            "audit",
            timed("audit", || registered_ci_skill_audits(&root())),
        ),
        ("fmt", timed("fmt", run_fmt)),
        (
            "test",
            timed("test", || run_cargo(workspace_test_args(deep))),
        ),
    ];
    if deep {
        checks.push((
            "fixture-projects",
            timed("fixture-projects", || fixture_projects::run(&root(), true)),
        ));
        checks.extend([
            (
                "clippy",
                timed("clippy", || {
                    run_cargo(&[
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "--all-features",
                        "--",
                        "-D",
                        "warnings",
                    ])
                }),
            ),
            ("editor", timed("editor", run_editor)),
            (
                "generator-deep-tests",
                timed("generator-deep-tests", || {
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
            ("doc", timed("doc", run_doc)),
        ]);
        checks.extend(external_test_checks(&root(), &mut |args| {
            external_test::cargo(&root(), args)
        }));
    } else {
        // PR-tier lock survives package autotest disable via external_test controls.
        checks.push((
            "generator-source-inventory",
            timed("generator-source-inventory", || {
                external_test::run_with_cargo(
                    &root(),
                    &GENERATOR_SOURCE_INVENTORY_LOCK_SPEC,
                    |args| external_test::cargo(&root(), args),
                )
                .map_err(|error| eprintln!("generator-source-inventory: {error}"))
                .is_ok()
            }),
        ));
    }
    checks.extend([
        ("key-order", timed("key-order", run_key_order)),
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
        ("fmt", timed("fmt", run_fmt)),
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

fn run_ci_capstone() -> u8 {
    let toolchain = timed("toolchain", check_toolchain);
    if let Err(error) = toolchain {
        eprintln!("toolchain: FAIL: {error}");
        eprintln!("summary: FAIL (toolchain)");
        return 1;
    }
    println!("toolchain: PASS");
    println!("test-tier: macos-capstone");

    let mut checks = CAPSTONE_PACKAGES
        .iter()
        .map(|&package| {
            let args = package_test_args(package);
            (package, timed(package, || run_cargo(&args)))
        })
        .collect::<Vec<_>>();
    checks.push((
        "fixture-projects",
        timed("fixture-projects", || fixture_projects::run(&root(), false)),
    ));
    checks.extend(external_test_checks(&root(), &mut |args| {
        external_test::cargo(&root(), args)
    }));
    for &(name, passed) in &checks {
        println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
    }
    summarize_ci(CiTier::Capstone, &checks)
}

fn run_ci_born_valid() -> u8 {
    let toolchain = timed("toolchain", check_toolchain);
    if let Err(error) = toolchain {
        eprintln!("toolchain: FAIL: {error}");
        eprintln!("summary: FAIL (toolchain)");
        return 1;
    }
    println!("toolchain: PASS");
    println!("test-tier: macos-born-valid");

    let passed = timed("boxology-init-born-valid", || {
        external_test::run_with_cargo(&root(), &BORN_VALID_SPEC, |args| {
            external_test::cargo(&root(), args)
        })
        .map_err(|error| eprintln!("boxology-init-born-valid: {error}"))
        .is_ok()
    });
    println!(
        "boxology-init-born-valid: {}",
        if passed { "PASS" } else { "FAIL" }
    );
    summarize_ci(CiTier::PullRequest, &[("boxology-init-born-valid", passed)])
}

fn package_test_args(package: &'static str) -> Vec<&'static str> {
    let mut args = vec!["test", "-p", package, "--all-features"];
    if package == BORN_VALID_SPEC.package {
        args.extend(["--", "--skip", BORN_VALID_SPEC.tests[0]]);
    }
    args
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

fn workspace_test_args(deep: bool) -> &'static [&'static str] {
    if deep {
        &["test", "--workspace", "--all-features"]
    } else {
        &[
            "test",
            "--workspace",
            "--all-features",
            "--exclude",
            "boxology-init",
            "--exclude",
            "boxology-generator",
            "--exclude",
            "boxology-workspace",
            "--exclude",
            "xtask",
        ]
    }
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

fn run_fmt() -> bool {
    OWNED_FMT_PACKAGES.iter().all(|package| {
        debug_assert!(!FMT_EXCLUDED_PACKAGES.contains(package));
        run_cargo(&["fmt", "--check", "-p", package])
    })
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

/// Re-runs `boxology-schema`'s byte tests against `serde_json`'s insertion-ordered map backing,
/// the one configuration that can tell whether the crate sorts document keys or inherits them
/// from the default `BTreeMap`. Format 1's bytes depend on the answer.
fn run_key_order() -> bool {
    run_cargo(&[
        "test",
        "-p",
        "boxology-schema",
        "--features",
        "preserve-order",
    ])
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
    use std::collections::BTreeSet;

    #[test]
    fn owned_format_gate_passes_while_generated_fixture_fails_directly() {
        assert!(run_fmt());
        let output = Command::new("cargo")
            .args([
                "fmt",
                "--check",
                "--manifest-path",
                "crates/fixtures/generated-style-fmt/Cargo.toml",
            ])
            .current_dir(root())
            .output()
            .expect("run cargo fmt --check on generated-style-fmt");
        assert!(
            !output.status.success(),
            "generated-style-fmt must fail rustfmt --check"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Diff in") && stdout.contains("generated-style-fmt/src/lib.rs"),
            "expected affirmative rustfmt diff for generated-style-fmt/src/lib.rs, got: {stdout}"
        );
    }

    #[test]
    fn format_registries_cover_every_crate_once() {
        let (owned, excluded) = format_registry_sets(OWNED_FMT_PACKAGES, FMT_EXCLUDED_PACKAGES)
            .expect("format registries contain no duplicates");
        let mut manifests = Vec::new();
        find_manifests(&root().join("crates"), &mut manifests);
        let found: BTreeSet<_> = manifests.iter().map(|path| manifest_name(path)).collect();
        let classified: BTreeSet<_> = owned
            .union(&excluded)
            .map(|name| name.to_string())
            .collect();
        assert_eq!(manifests.len(), found.len());
        assert_eq!(found, classified);
    }

    fn format_registry_sets<'a>(
        owned: &[&'a str],
        excluded: &[&'a str],
    ) -> Option<(BTreeSet<&'a str>, BTreeSet<&'a str>)> {
        let owned_set: BTreeSet<_> = owned.iter().copied().collect();
        let excluded_set: BTreeSet<_> = excluded.iter().copied().collect();
        (owned_set.len() == owned.len()
            && excluded_set.len() == excluded.len()
            && owned_set.is_disjoint(&excluded_set))
        .then_some((owned_set, excluded_set))
    }

    #[test]
    fn format_registry_duplicates_are_rejected() {
        assert!(format_registry_sets(&["a", "a"], &["b"]).is_none());
        assert!(format_registry_sets(&["a"], &["b", "b"]).is_none());
        assert!(format_registry_sets(&["a"], &["a"]).is_none());
    }

    fn find_manifests(directory: &Path, found: &mut Vec<PathBuf>) {
        if fs::read_to_string(directory.join("Cargo.toml"))
            .is_ok_and(|text| text.lines().any(|line| line.trim() == "[workspace]"))
        {
            // Generated fixture crates remain in the excluded registry; their hand-authored
            // siblings are checked by the fixture-projects gate instead.
            let generated = directory.join("generated");
            if generated.is_dir() {
                find_manifests(&generated, found);
            }
            return;
        }
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                find_manifests(&path, found);
            } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
                found.push(path);
            }
        }
    }

    fn manifest_name(path: &Path) -> String {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .find_map(|line| line.trim().strip_prefix("name = \"")?.strip_suffix('"'))
            .unwrap()
            .to_string()
    }

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
        let expected: &[(&str, external_test::ExternalTestSpec)] = &[
            (
                "surface-lock",
                external_test::ExternalTestSpec {
                    package: "boxology-workspace",
                    target: "surface_lock",
                    manifest: "crates/boxology-workspace/Cargo.toml",
                    source: "crates/boxology-workspace/tests/surface_lock.rs",
                    default_source: "tests/surface_lock.rs",
                    tests: &["surface_and_live_evasions_are_locked"],
                    source_digest: "851bc809ce185a49fb0ef6b6b5758269c2ae559bd7f363ed1288c86883780eea",
                    body_digest: "3daaf29c01df87b82990aaeeaa74b8856f85e1ff8a984992910393b3528b60d9",
                },
            ),
            (
                "classifier-surface-lock",
                external_test::ExternalTestSpec {
                    package: "boxology-classifier",
                    target: "surface_lock",
                    manifest: "crates/boxology-classifier/Cargo.toml",
                    source: "crates/boxology-classifier/tests/surface_lock.rs",
                    default_source: "tests/surface_lock.rs",
                    tests: &["surface_and_live_evasions_are_locked"],
                    source_digest: "b633faf32525a7b4883e9f7f07c77a0738f199797927240d71abd32618f59dd3",
                    body_digest: "b010c6eb43ce00b40f6dd11c3aa63c1f62ad8c2e3196f8e7e1e4ffd10331e65a",
                },
            ),
            (
                "generator-source-inventory",
                external_test::ExternalTestSpec {
                    package: "boxology-generator-model",
                    target: "purity_lock",
                    manifest: "crates/boxology-generator-model/Cargo.toml",
                    source: "crates/boxology-generator-model/tests/purity_lock.rs",
                    default_source: "tests/purity_lock.rs",
                    tests: GENERATOR_SOURCE_INVENTORY_TESTS,
                    source_digest: "3359fcb86ff1890a0c358d391b8225a355a5995f97ab40bb1e11845e2e75c297",
                    body_digest: "41e7ab2344e9f8e3d860bdec77784288fb9bbbbea9666535d90f1e963dbfae45",
                },
            ),
        ];
        assert_eq!(
            EXTERNAL_TEST_SPECS.len(),
            3,
            "mutation survived: registry length"
        );
        for (index, (name, spec)) in expected.iter().enumerate() {
            assert_eq!(
                EXTERNAL_TEST_SPECS[index].0, *name,
                "mutation survived: report order {name}"
            );
            let count = EXTERNAL_TEST_SPECS
                .iter()
                .filter(|(n, s)| *n == *name && *s == spec)
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
        assert_eq!(results.len(), 3, "mutation survived: results.len()");
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
            3,
            "mutation survived: false-propagation length"
        );
        assert!(
            false_results.iter().all(|(_, passed)| !*passed),
            "mutation survived: false-propagation"
        );
    }

    #[test]
    fn external_expectations_are_tier_aware_and_false_results_propagate() {
        let production = include_str!("main.rs")
            .split_once("\nmod tests {")
            .unwrap()
            .0;
        assert!(
            production.contains("summarize_ci(tier, &checks)"),
            "run_ci must summarize with its tier"
        );
        assert!(
            production.contains(
                "CiTier::Capstone => {\n            if !checks.iter().any(|(name, _)| *name == \
                 \"fixture-projects\")"
            ),
            "capstone must fail closed if fixture-projects is displaced"
        );
        let passed: Vec<_> = EXTERNAL_TEST_SPECS
            .iter()
            .map(|(n, _)| (*n, true))
            .collect();
        for tier in [CiTier::PullRequest, CiTier::Deep] {
            assert!(ci_failure_names(tier, &passed).is_empty(), "{tier:?}");
        }
        let mut capstone_passed = passed.clone();
        capstone_passed.push(("fixture-projects", true));
        assert!(ci_failure_names(CiTier::Capstone, &capstone_passed).is_empty());
        assert_eq!(
            ci_failure_names(CiTier::PullRequest, &[]),
            ["generator-source-inventory"]
        );
        assert_eq!(
            ci_failure_names(CiTier::Deep, &[]),
            vec![
                "surface-lock",
                "classifier-surface-lock",
                "generator-source-inventory"
            ]
        );
        assert_eq!(
            ci_failure_names(CiTier::Capstone, &[]),
            vec![
                "fixture-projects",
                "surface-lock",
                "classifier-surface-lock",
                "generator-source-inventory"
            ]
        );
        // Hygiene is a non-PR tier that must not inherit Deep/Capstone external-test requirements.
        assert_eq!(
            ci_failure_names(CiTier::Hygiene, &[]),
            HYGIENE_CHECKS.to_vec()
        );

        let failed: Vec<_> = EXTERNAL_TEST_SPECS
            .iter()
            .map(|(name, _)| (*name, false))
            .collect();
        assert_eq!(
            ci_failure_names(CiTier::PullRequest, &failed).len(),
            3,
            "present false verdicts fail on PR, including the required generator lock"
        );
    }

    #[test]
    fn hygiene_tier_requires_exact_check_names_and_failed_checks() {
        assert_eq!(
            HYGIENE_CHECKS,
            &[
                "audit",
                "fmt",
                "key-order",
                "whitespace",
                "links",
                "records",
                "budget"
            ]
        );
        let passed: Vec<_> = HYGIENE_CHECKS.iter().map(|&name| (name, true)).collect();
        assert!(ci_failure_names(CiTier::Hygiene, &passed).is_empty());

        let mut missing_links = passed.clone();
        missing_links.retain(|(name, _)| *name != "links");
        assert_eq!(
            ci_failure_names(CiTier::Hygiene, &missing_links),
            vec!["links"]
        );

        let mut with_extra = passed.clone();
        with_extra.push(("deny", true));
        assert_eq!(ci_failure_names(CiTier::Hygiene, &with_extra), vec!["deny"]);

        let mut failed_fmt = passed;
        failed_fmt[1] = ("fmt", false);
        assert_eq!(ci_failure_names(CiTier::Hygiene, &failed_fmt), vec!["fmt"]);

        // Present external-test names must not be required merely because Hygiene is non-PR.
        let hygiene_plus_external: Vec<_> = HYGIENE_CHECKS
            .iter()
            .map(|&name| (name, true))
            .chain(EXTERNAL_TEST_SPECS.iter().map(|(name, _)| (*name, true)))
            .collect();
        assert_eq!(
            ci_failure_names(CiTier::Hygiene, &hygiene_plus_external),
            EXTERNAL_TEST_SPECS
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
        );
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
    fn pr_workflow_selects_changed_crates_only_inside_rust_lane() {
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
            scope.contains("if test \"$run_rust\" = true; then")
                && scope.contains("changed_manifests=\"$RUNNER_TEMP/boxology-changed-manifests\"")
                && scope.contains("$dir/Cargo.toml")
                && scope.contains("sort -u -o \"$changed_manifests\""),
            "changed-crate selection must be gated on run_rust so Markdown-only \
             crate paths stay hygiene-only"
        );
    }

    #[test]
    fn pr_workflow_keeps_full_suites_off_the_required_path() {
        let workflow = include_str!("../../../.github/workflows/pr.yml");
        assert!(workflow.contains("cargo test -p xtask --locked"));
        assert!(workflow.contains("- name: Test changed crates"));
        assert!(workflow.contains("- name: Check workspace build graph"));
        assert!(workflow.contains("steps.scope.outputs.run_workspace_check == 'true'"));
        assert!(workflow.contains("steps.scope.outputs.run_reaper == 'true'"));
        assert!(
            !workflow.contains("--bin boxology -- check"),
            "the complete product check belongs to dispatch-only deep validation"
        );
        assert!(
            !workflow.contains("cargo test --workspace"),
            "the full workspace test sweep belongs to dispatch-only deep validation"
        );
        assert!(
            !workflow.contains("ping-app/Cargo.toml"),
            "composition acceptance belongs to dispatch-only deep validation"
        );
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

    #[test]
    fn workspace_test_argv_pins_pr_exclusion_and_full_deep_suite() {
        assert_eq!(
            workspace_test_args(false),
            &[
                "test",
                "--workspace",
                "--all-features",
                "--exclude",
                "boxology-init",
                "--exclude",
                "boxology-generator",
                "--exclude",
                "boxology-workspace",
                "--exclude",
                "xtask",
            ]
        );
        assert_eq!(
            workspace_test_args(true),
            &["test", "--workspace", "--all-features"]
        );
    }

    #[test]
    fn capstone_package_inventory_and_argv_are_exact() {
        assert_eq!(
            CAPSTONE_PACKAGES,
            &[
                "boxology-init",
                "boxology-generator",
                "boxology-workspace",
                "xtask"
            ]
        );
        assert_eq!(
            package_test_args("boxology-init"),
            [
                "test",
                "-p",
                "boxology-init",
                "--all-features",
                "--",
                "--skip",
                "initialized_project_is_born_valid_and_regeneration_is_a_no_op"
            ]
        );
        for package in &CAPSTONE_PACKAGES[1..] {
            assert_eq!(
                package_test_args(package),
                ["test", "-p", package, "--all-features"]
            );
        }
    }
}
