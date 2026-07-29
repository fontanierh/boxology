use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

mod advisories;
mod budget;
mod deny;
pub mod determinism;
mod determinism_compare;
mod determinism_cross;
#[cfg(test)]
mod determinism_meta;
mod determinism_publish;
mod determinism_run;
mod determinism_verify;
mod generated_project_subject;
mod generator_model_subject;
mod links;
mod records;
mod skill_audit;
mod workspace_subject;

// Bootstrap registries. S7 replaces both with manifest-derived classification (S0 D10).
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
    "boxology-runtime",
    "boxology-telegram",
    "greeter-implementation",
    "hello-implementation",
    "ping-implementation",
    "ping-app",
    "xtask",
];
const FMT_EXCLUDED_PACKAGES: &[&str] = &[
    "generated-style-fmt",
    "greeter-contract",
    "hello-contract",
    "ping-contract",
];
const EDITOR_FIXTURE: &str = "crates/fixtures/hello/implementation";
const EDITOR_CHECK_ARGS: &[&str] = &[
    "analysis-stats",
    "--disable-build-scripts",
    "--disable-proc-macros",
    "--no-test",
    EDITOR_FIXTURE,
];
const SURFACE_LOCK_TEST_ARGS: &[&str] =
    &["test", "-p", "boxology-workspace", "--test", "surface_lock"];
type SkillCommand = (&'static str, fn(&Path) -> u8);
const SKILL_COMMANDS: &[SkillCommand] = &[("skill-audit", skill_audit::run)];
const CI_SKILL_AUDITS: &[fn(&Path) -> bool] = &[run_skill_audit_ci];

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
        "usage: cargo xtask advisories --repo <owner/repo> [--simulate <RUSTSEC-id>]\n       cargo xtask ci (--base <revision> | --no-budget)\n       cargo xtask budget --base <revision>\n       cargo xtask deny\n       cargo xtask determinism\n       cargo xtask determinism-manifest --out <directory>\n       cargo xtask determinism-manifest --out <directory> --meta-cross\n       cargo xtask determinism-compare <a> <b>\n       cargo xtask determinism-meta-cross <linux> <macos>\n       cargo xtask determinism-verify <directory> --target <triple> [--require-image]\n       cargo xtask skill-audit\n       cargo xtask links\n       cargo xtask records [--base <revision>]\n       cargo xtask test\n       cargo xtask subject-run <name> --out <directory>  (internal)"
    );
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run_ci(base: Option<&str>) -> u8 {
    let toolchain = timed("toolchain", check_toolchain);
    if let Err(error) = toolchain {
        eprintln!("toolchain: FAIL: {error}");
        eprintln!("summary: FAIL (toolchain)");
        return 1;
    }
    println!("toolchain: PASS");

    let mut checks = vec![
        (
            "audit",
            timed("audit", || registered_ci_skill_audits(&root())),
        ),
        ("fmt", timed("fmt", run_fmt)),
        ("editor", timed("editor", run_editor)),
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
        (
            "test",
            timed("test", || {
                run_cargo(&["test", "--workspace", "--all-features"])
            }),
        ),
        (
            "surface-lock",
            timed("surface-lock", || run_cargo(SURFACE_LOCK_TEST_ARGS)),
        ),
        ("key-order", timed("key-order", run_key_order)),
        ("doc", timed("doc", run_doc)),
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
    ];
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
    let failed: Vec<_> = checks
        .iter()
        .filter_map(|(name, passed)| (!passed).then_some(*name))
        .collect();
    if failed.is_empty() {
        println!("summary: PASS");
        0
    } else {
        eprintln!("summary: FAIL ({})", failed.join(", "));
        1
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
        assert!(!run_cargo(&["fmt", "--check", "-p", "generated-style-fmt"]));
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

    fn replace_once(source: &str, anchor: &str) -> String {
        assert_eq!(
            source.match_indices(anchor).count(),
            1,
            "anchor: {anchor:?}"
        );
        source.replacen(anchor, "", 1)
    }

    fn run_ci_body(source: &str) -> &str {
        source
            .split_once("fn run_ci(base: Option<&str>) -> u8 {")
            .and_then(|(_, body)| body.split_once("\nfn timed<").map(|(body, _)| body))
            .expect("run_ci body")
    }

    #[test]
    fn surface_lock_registration_is_live_and_deletion_is_red() {
        let source = include_str!("main.rs");
        assert_eq!(
            SURFACE_LOCK_TEST_ARGS,
            &["test", "-p", "boxology-workspace", "--test", "surface_lock"]
        );
        let registration = "        (\n            \"surface-lock\",\n            timed(\"surface-lock\", || run_cargo(SURFACE_LOCK_TEST_ARGS)),\n        ),";
        assert_eq!(run_ci_body(source).match_indices(registration).count(), 1);
        let deleted = replace_once(source, registration);
        assert_eq!(run_ci_body(&deleted).match_indices(registration).count(), 0);
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
