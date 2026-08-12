use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

mod advisories;
mod authority_digests;
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
    manifest_digest: None,
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
        manifest_digest: None,
        source_digest: "594db997b4d7eb79efcd05f9ad5d2235e1ffca4fca40bbd53d3c1e91125f5afe",
        body_digest: "49c5350e23a2c3bf6f84a86e34fc18df7fc8458395cd04ca8a4439cf077fdd89",
    };
// #107A closed generator source-surface and effect authority. Deep-required. Transitive: #358.
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
        manifest_digest: None,
        source_digest: "e164488085071ca4f4d66213d514a8e907f1b678ea1c30061dc8b5c7d7c21c9a",
        body_digest: "566a20373d93bb4c6451bf3d77bf16ffa31e58cffbbb399c0c70f698600dfa3b",
    };
const BORN_VALID_SPEC: external_test::ExternalTestSpec = external_test::ExternalTestSpec {
    package: "boxology-init",
    target: "born_valid",
    manifest: "crates/boxology-init/Cargo.toml",
    source: "crates/boxology-init/tests/born_valid.rs",
    default_source: "tests/born_valid.rs",
    tests: &["initialized_project_is_born_valid_and_regeneration_is_a_no_op"],
    manifest_digest: None,
    source_digest: "39f9cc5942c31a9f65dbce68579bfd45cf6784a05e7332dbe5253cebfabb62a1",
    body_digest: "3dc223a36c9fe30196323239f500762e909def84e4e70ace1bcdd25f37d54e78",
};
const CLI_END_TO_END_TESTS: &[&str] = &[
    "check_base_absent_schema_is_a_valid_none_base",
    "check_base_cat_file_failure_after_confirmed_presence_is_always_bxw0092",
    "check_base_git_boundary_uses_the_exact_nonmutating_argv",
    "check_base_git_spawn_failure_is_invocation_exit_two",
    "check_base_rejects_invalid_and_noncommit_revisions_without_echoing_them",
    "check_base_reports_a_present_non_blob_schema_as_bxw0092",
    "check_base_reports_malformed_schema_as_bxw0080",
    "check_base_reports_real_addition_and_unchanged_baseline_without_failing",
    "check_clean_workspace_reports_all_steps_and_exits_zero",
    "check_default_base_classifies_against_merge_base_with_main",
    "check_default_base_git_boundary_uses_the_exact_nonmutating_argv",
    "check_default_base_git_spawn_failure_is_invocation_exit_two",
    "check_default_base_merge_base_garbage_is_bxw0091",
    "check_default_base_skips_when_histories_are_disjoint",
    "check_default_base_skips_when_main_is_missing_after_committed_trunk",
    "check_default_base_skips_when_main_is_unborn",
    "check_default_base_skips_when_no_repository_is_available",
    "check_deleted_generation_input_is_a_step_error",
    "check_format_human_matches_default_bytes",
    "check_guard_rejection_is_fatal_without_a_misleading_report",
    "check_json_accepts_format_and_base_in_either_order",
    "check_json_clean_workspace_is_exact_byte_golden",
    "check_json_lock_failure_is_structured_failed_with_output",
    "check_json_planning_failure_is_exact_and_human_stays_unchanged",
    "check_json_preserves_controlled_contract_diagnostics_before_any_later_step",
    "check_json_preserves_request_diagnostics_before_any_later_step",
    "check_lock_failure_renders_finding_command_output_and_keeps_later_steps",
    "check_metadata_failure_is_invocation",
    "check_quality_commands_run_after_tests_in_package_id_order",
    "check_quality_failure_is_bxw0107_with_captured_output_and_continues",
    "check_rejects_unwired_flags_with_usage",
    "check_reports_an_absent_selected_box_as_bxw0087",
    "check_reports_schema_selector_and_exposure_findings_in_the_eight_step_report",
    "check_tampered_and_missing_artifacts_fail_naming_repair",
    "check_test_failure_renders_finding_command_output_and_exit_one",
    "check_tool_failure_renders_finding_command_output_and_exit_one",
    "check_validates_exact_and_wildcard_composition_with_the_planned_schema",
    "check_workspace_findings_fail_before_composition",
    "corrupted_cargo_toml_is_reported_with_captured_cargo_output",
    "first_write_then_byte_identical_unchanged_run_uses_exact_argv",
    "generate_incompatible_classification_still_exits_zero",
    "generate_package_ping_attaches_exact_additive_classification",
    "generate_unparseable_base_is_bxw0077_without_result_line",
    "generate_updates_provenance_digest_and_revision",
    "ingest::git_ingestion_boundary_discovery_and_cargo",
    "metadata_failure_reports_code_and_captured_stderr",
    "non_unicode_argument_is_usage_failure_without_panic",
    "non_utf8_metadata_is_a_coded_failure",
    "ownership::report_wiring_real_git_cases",
    "parsing_accepts_only_the_two_generate_forms",
    "unknown_package_is_invocation_failure",
    "unowned_tracked_file_is_a_workspace_failure",
];
const CLI_SURFACE_LOCK_TESTS: &[&str] = &[
    "invalid_path_is_not_skipped_on_unix",
    "root_gate_is_exact",
    "source_surface_is_exact_and_mutation_resistant",
    "symlink_root_is_rejected_before_external_ingestion",
    "walk_is_opaque_sorted_and_exact",
];
const CLI_END_TO_END_SPEC: external_test::ExternalTestSpec = external_test::ExternalTestSpec {
    package: "boxology-cli",
    target: "cli",
    manifest: "crates/boxology-cli/Cargo.toml",
    source: "crates/boxology-cli/tests/cli.rs",
    default_source: "tests/cli.rs",
    tests: CLI_END_TO_END_TESTS,
    manifest_digest: Some("a5f38361736e346461ff181ca6d90f49f44d976424081c46d19b8c8cffd13731"),
    source_digest: "50b8641327b54a40e40973d0394c0d6998797705d609ce70cc1f80ef2bfdf8fa",
    body_digest: "22b894e80b8da524466a16723349316863ff83f42675ead294cf298ba44b2ba7",
};
const CLI_SURFACE_LOCK_SPEC: external_test::ExternalTestSpec = external_test::ExternalTestSpec {
    package: "boxology-cli",
    target: "surface_lock",
    manifest: "crates/boxology-cli/Cargo.toml",
    source: "crates/boxology-cli/tests/surface_lock.rs",
    default_source: "tests/surface_lock.rs",
    tests: CLI_SURFACE_LOCK_TESTS,
    manifest_digest: Some("a5f38361736e346461ff181ca6d90f49f44d976424081c46d19b8c8cffd13731"),
    source_digest: "3d1952b5b995a3d3eb4fa01ce9accc454666cf379efbb948efc734313987a80d",
    body_digest: "68c2fa5d26fd67c2e95a0b478b4b25f9c6663298af94f97777885a3d1cc39fa1",
};
const EXTERNAL_TEST_SPECS: &[(&str, &external_test::ExternalTestSpec)] = &[
    ("cli-end-to-end-integrity", &CLI_END_TO_END_SPEC),
    ("cli-surface-lock-integrity", &CLI_SURFACE_LOCK_SPEC),
    ("surface-lock", &SURFACE_LOCK_SPEC),
    ("classifier-surface-lock", &CLASSIFIER_SURFACE_LOCK_SPEC),
    (
        "generator-source-inventory",
        &GENERATOR_SOURCE_INVENTORY_LOCK_SPEC,
    ),
    ("born-valid", &BORN_VALID_SPEC),
];
type ProductRunner<'a> = dyn FnMut(&[&str]) -> bool + 'a;
type CargoRunner<'a> = dyn FnMut(&[&str]) -> bool + 'a;
type SkillCommand = (&'static str, fn(&Path) -> u8);
const GENERATOR_DEEP_TEST_ARGS: &[&str] = &[
    "test",
    "-p",
    "boxology-generator",
    "--all-features",
    "--",
    "--ignored",
    "--test-threads=1",
];
const SKILL_COMMANDS: &[SkillCommand] = &[("skill-audit", skill_audit::run)];
const CI_SKILL_AUDITS: &[fn(&Path) -> bool] = &[run_skill_audit_ci];
const GENERATOR_MULTI_CAPABILITY_E2E: &str =
    "tests::generated_multi_capability_box_compiles_and_routes_both_capabilities";
const GENERATOR_SEALED_IMPORT_E2E: &str =
    "tests::structured_import_routes_through_provider_owned_alias_end_to_end";
const GENERATOR_PR_EXCLUDED_LIVE_TEST_SPEC: external_test::LiveTestSpec =
    external_test::LiveTestSpec {
        manifest: "crates/boxology-generator/Cargo.toml",
        manifest_digest: "2a5d0e715e019c59f919d6b547a2e6c4830e75369d1a75d6ba1d654a718634be",
        source: "crates/boxology-generator/src/lib.rs",
        tests: &[GENERATOR_MULTI_CAPABILITY_E2E, GENERATOR_SEALED_IMPORT_E2E],
        body_digest: "43d1ae661136f78917180c6483811fc13f315569d67a7e4af650c279b3ddbe45",
    };
#[cfg(test)]
const GENERATOR_PR_EXCLUDED_UNIT_TESTS: &[&str] =
    &[GENERATOR_MULTI_CAPABILITY_E2E, GENERATOR_SEALED_IMPORT_E2E];
#[cfg(test)]
const PR_EXCLUDED_INTEGRATION_TARGETS: &[(&str, &str)] = &[
    ("boxology-init", "born_valid"),
    ("boxology-cli", "cli"),
    ("boxology-cli", "surface_lock"),
    ("boxology-workspace", "surface_lock"),
    ("boxology-classifier", "surface_lock"),
    ("boxology-generator-model", "purity_lock"),
];

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
    "authority-digests",
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
    "cli-end-to-end-integrity",
    "cli-surface-lock-integrity",
    "surface-lock",
    "classifier-surface-lock",
    "generator-source-inventory",
    "born-valid",
    "generator-pr-excluded-unit-integrity",
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
    "cli-end-to-end-integrity",
    "cli-surface-lock-integrity",
    "surface-lock",
    "classifier-surface-lock",
    "generator-source-inventory",
    "born-valid",
    "generator-pr-excluded-unit-integrity",
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

fn external_test_integrity_checks(root: &Path) -> Vec<(&'static str, bool)> {
    EXTERNAL_TEST_SPECS
        .iter()
        .map(|(name, spec)| {
            (
                *name,
                timed(
                    name,
                    || match external_test::check_external_test_integrity(root, spec) {
                        Ok(()) => true,
                        Err(error) => {
                            eprintln!("{name}: {error}");
                            false
                        }
                    },
                ),
            )
        })
        .collect()
}

fn generator_pr_excluded_unit_integrity(root: &Path) -> bool {
    match external_test::check_live_test_integrity(root, &GENERATOR_PR_EXCLUDED_LIVE_TEST_SPEC) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("generator-pr-excluded-unit-integrity: {error}");
            false
        }
    }
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
                    run_generator_deep_tests(&mut |args| run_cargo(args))
                }),
            ),
            ("repo-doc", timed("repo-doc", run_doc)),
        ]);
    }
    checks.extend(external_test_integrity_checks(&root()));
    checks.push((
        "generator-pr-excluded-unit-integrity",
        timed("generator-pr-excluded-unit-integrity", || {
            generator_pr_excluded_unit_integrity(&root())
        }),
    ));
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
        (
            "authority-digests",
            timed("authority-digests", || authority_digests::check(&root())),
        ),
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
        "boxology-cli-core",
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

fn run_generator_deep_tests(run: &mut CargoRunner<'_>) -> bool {
    run(GENERATOR_DEEP_TEST_ARGS)
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

    // Phase B changes this integrity expectation and the workflow route in the
    // same two-file PR after the primary registration is proven live.
    const REQUIRED_PR_RUNNER_LABEL: &str = "boxology-macos-pr-primary";
    const XTASK_AUTHORITY_SELECTOR: &str =
        r"^(crates/xtask/|\.github/workflows/|ops/ci-runner/|\.agents/skills/boxology/SKILL\.md$)";

    fn selector_matches(expression: &str, inventory: &str) -> bool {
        use std::io::Write as _;

        let mut child = Command::new("grep")
            .args(["-Eq", expression])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(inventory.as_bytes())
            .unwrap();
        child.wait().unwrap().success()
    }

    struct ScopeRepo(std::path::PathBuf);

    impl ScopeRepo {
        fn new() -> Self {
            for attempt in 0..1000 {
                let path = std::env::temp_dir().join(format!(
                    "boxology-pr-scope-{}-{attempt}",
                    std::process::id()
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("cannot create scope repo: {error}"),
                }
            }
            panic!("cannot allocate scope repo")
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.0)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        }
    }

    impl Drop for ScopeRepo {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
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
        let expected = [
            ("cli-end-to-end-integrity", &CLI_END_TO_END_SPEC),
            ("cli-surface-lock-integrity", &CLI_SURFACE_LOCK_SPEC),
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
    fn external_test_integrity_checks_are_complete_and_nonexecuting() {
        let results = external_test_integrity_checks(&root());
        assert_eq!(results.len(), EXTERNAL_TEST_SPECS.len());
        assert!(
            results.iter().all(|(_, passed)| *passed),
            "mutation survived: integrity verdict"
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
    fn deep_has_one_behavioral_executor_and_integrity_owns_every_pr_exclusion() {
        let mut integrity_owned_integrations = EXTERNAL_TEST_SPECS
            .iter()
            .map(|(_, spec)| (spec.package, spec.target))
            .collect::<Vec<_>>();
        integrity_owned_integrations.sort_unstable();
        let mut expected_integrations = PR_EXCLUDED_INTEGRATION_TARGETS.to_vec();
        expected_integrations.sort_unstable();
        assert_eq!(integrity_owned_integrations, expected_integrations);
        integrity_owned_integrations.dedup();
        assert_eq!(
            integrity_owned_integrations.len(),
            PR_EXCLUDED_INTEGRATION_TARGETS.len()
        );
        assert_eq!(
            GENERATOR_PR_EXCLUDED_LIVE_TEST_SPEC.tests,
            GENERATOR_PR_EXCLUDED_UNIT_TESTS
        );
        assert!(generator_pr_excluded_unit_integrity(&root()));

        let mut cargo_calls = Vec::new();
        assert!(run_product_check(None, &mut |args| {
            cargo_calls.push(args.iter().map(ToString::to_string).collect::<Vec<_>>());
            true
        }));
        assert!(run_generator_deep_tests(&mut |args| {
            cargo_calls.push(args.iter().map(ToString::to_string).collect::<Vec<_>>());
            true
        }));
        assert_eq!(
            cargo_calls,
            [
                product_check_args(None)
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                GENERATOR_DEEP_TEST_ARGS
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            ]
        );
        assert_eq!(
            boxology_cli_core::test_spec().render(),
            "cargo test --workspace --all-features"
        );
        assert!(GENERATOR_DEEP_TEST_ARGS.contains(&"--ignored"));
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
        assert!(matches!(
            REQUIRED_PR_RUNNER_LABEL,
            "boxology-macos-pr" | "boxology-macos-pr-primary"
        ));
        let expected_route =
            format!("runs-on: [self-hosted, macOS, ARM64, {REQUIRED_PR_RUNNER_LABEL}]");
        assert_eq!(
            workflow
                .lines()
                .filter(|line| line.trim() == expected_route)
                .count(),
            1
        );
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
                && scope.contains("ops/process-reaper/")
                && scope.contains("ops/delivery-worker/"),
            "opaque fixtures and process owners retain scoped path gates"
        );
        assert!(!workflow.contains("--bin boxology -- check"));
        assert!(!workflow.contains("cargo xtask ci --base"));
        assert!(workflow.contains("cargo xtask ci-fixtures"));
        for exact in [
            "run_xtask=false",
            "run_xtask=true",
            "echo \"run_xtask=$run_xtask\" | tee -a \"$GITHUB_OUTPUT\"",
        ] {
            assert_eq!(
                scope.lines().filter(|line| line.trim() == exact).count(),
                1,
                "{exact}"
            );
        }
        let selector = format!("if grep -Eq '{XTASK_AUTHORITY_SELECTOR}' <<<\"$changed\"; then");
        assert_eq!(
            scope.lines().filter(|line| line.trim() == selector).count(),
            1
        );
        assert_eq!(
            scope
                .matches("git diff --name-only --no-renames \"$BASE_SHA\" HEAD")
                .count(),
            1
        );
        assert!(!scope.contains("git diff --name-only \"$BASE_SHA\" HEAD"));
        for authority in [
            "crates/xtask/src/main.rs\n",
            ".github/workflows/pr.yml\n",
            "ops/ci-runner/supervise-macos.sh\n",
            ".agents/skills/boxology/SKILL.md\n",
        ] {
            assert!(
                selector_matches(XTASK_AUTHORITY_SELECTOR, authority),
                "{authority}"
            );
        }
        assert!(!selector_matches(
            XTASK_AUTHORITY_SELECTOR,
            "crates/boxology-telegram/implementation/src/lib.rs\n"
        ));
        assert_eq!(
            workflow
                .lines()
                .filter(|line| line.trim() == "if: steps.scope.outputs.run_xtask == 'true'")
                .count(),
            1
        );
        assert!(workflow.contains("steps.scope.outputs.run_reaper == 'true'"));
        assert!(workflow.contains("steps.scope.outputs.run_delivery_worker == 'true'"));
        assert!(workflow.contains("cargo test -p xtask --locked"));
        assert!(workflow.contains("- name: Test changed crates"));
        assert!(workflow.contains("- name: Check workspace build graph"));
        let changed_crates = workflow
            .split_once("- name: Test changed crates")
            .unwrap()
            .1
            .split_once("- name: Check workspace build graph")
            .unwrap()
            .0;
        assert_eq!(changed_crates.matches("--doc").count(), 1);
        for (package, target) in PR_EXCLUDED_INTEGRATION_TARGETS {
            let target = format!("{package}:{target}");
            assert_eq!(changed_crates.matches(&target).count(), 1, "{target}");
        }
        assert_eq!(
            changed_crates
                .matches("crates/boxology-generator/Cargo.toml")
                .count(),
            2
        );
        for test in GENERATOR_PR_EXCLUDED_UNIT_TESTS {
            assert_eq!(changed_crates.matches(test).count(), 1, "{test}");
        }
        let exact_skips = format!(
            "--lib --bins -- \\\n                    --exact \\\n                    --skip {} \\\n                    --skip {}",
            GENERATOR_MULTI_CAPABILITY_E2E, GENERATOR_SEALED_IMPORT_E2E
        );
        assert_eq!(changed_crates.matches(&exact_skips).count(), 1);
        assert!(changed_crates.contains("--test \"$target\""));
        let steps = workflow
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- name: "))
            .collect::<Vec<_>>();
        assert_eq!(
            steps,
            [
                "Install pinned toolchain",
                "Record runner evidence",
                "CI hygiene",
                "Select test scope",
                "Test repository invariants",
                "Test changed crates",
                "Check workspace build graph",
                "Test changed opaque fixtures",
                "Test process reaper",
                "Test delivery worker ownership",
            ]
        );
        let cargo_tests = workflow
            .lines()
            .filter_map(|line| {
                let line = line.trim().strip_prefix("run: ").unwrap_or(line.trim());
                line.starts_with("cargo test").then_some(line)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            cargo_tests,
            [
                "cargo test -p xtask --locked",
                "cargo test --manifest-path \"$manifest\" --all-features --locked --lib --bins -- \\",
                "cargo test --manifest-path \"$manifest\" --all-features --locked --lib --bins",
                "cargo test --manifest-path \"$manifest\" --all-features --locked --doc",
                "cargo test --manifest-path \"$manifest\" --all-features --locked --test \"$target\"",
                "cargo test --manifest-path \"$manifest\" --all-features --locked",
            ]
        );
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
    fn pr_scope_inventory_keeps_a_renamed_authority_old_path() {
        let repo = ScopeRepo::new();
        repo.git(&["init", "-q"]);
        repo.git(&["config", "user.email", "scope@example.invalid"]);
        repo.git(&["config", "user.name", "Scope Test"]);
        let old = repo.0.join(".github/workflows/legacy.yml");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "name: legacy\n# stable bytes\n# rename proof\n").unwrap();
        repo.git(&["add", "."]);
        repo.git(&["commit", "-qm", "base"]);
        let new = repo.0.join("crates/boxology-telegram/legacy.yml");
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::rename(old, new).unwrap();
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-qm", "rename"]);

        let default_inventory = repo.git(&["diff", "--name-only", "HEAD^", "HEAD"]);
        assert_eq!(default_inventory, "crates/boxology-telegram/legacy.yml\n");
        assert!(!selector_matches(
            XTASK_AUTHORITY_SELECTOR,
            &default_inventory
        ));

        let fail_closed_inventory =
            repo.git(&["diff", "--name-only", "--no-renames", "HEAD^", "HEAD"]);
        assert_eq!(
            fail_closed_inventory,
            ".github/workflows/legacy.yml\ncrates/boxology-telegram/legacy.yml\n"
        );
        assert!(selector_matches(
            XTASK_AUTHORITY_SELECTOR,
            &fail_closed_inventory
        ));
    }

    #[test]
    fn base_runner_configures_one_primary_affinity_and_capacity_slots_stay_generic() {
        let supervisor = include_str!("../../../ops/ci-runner/supervise-macos.sh");
        assert!(supervisor.contains("RUNNER_LABEL=boxology-macos-pr\n"));
        assert!(supervisor.contains("RUNNER_EXTRA_LABEL=\"${RUNNER_EXTRA_LABEL:-}\""));
        assert!(supervisor.contains("$RUNNER_EXTRA_LABEL\" != \"$RUNNER_LABEL"));
        assert!(supervisor.contains("length) < 1"));
        assert!(supervisor.contains("if $extra == \"\" then [] else [$extra] end"));

        let base =
            include_str!("../../../ops/ci-runner/com.fontanierh.boxology-ci-macos-runner.plist");
        assert_eq!(base.matches("boxology-macos-pr-primary").count(), 1);

        let slots = include_str!("../../../ops/ci-runner/supervise-slots.sh");
        let slots_plist = include_str!(
            "../../../ops/ci-runner/com.fontanierh.boxology-ci-macos-runner-slots.plist"
        );
        assert_eq!(
            slots
                .lines()
                .filter(|line| line.trim() == "export RUNNER_EXTRA_LABEL=")
                .count(),
            1
        );
        assert_eq!(
            slots
                .lines()
                .filter(|line| line.trim().starts_with("export RUNNER_EXTRA_LABEL="))
                .count(),
            1
        );
        assert!(!slots_plist.contains("boxology-macos-pr-primary"));
    }

    #[test]
    fn generated_multi_capability_box_compiles_and_routes_both_capabilities_suffix_collision() {}

    #[test]
    fn exact_generator_skips_preserve_suffix_collision() {
        let test = "tests::generated_multi_capability_box_compiles_and_routes_both_capabilities_suffix_collision";
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                test,
                "--exact",
                "--skip",
                GENERATOR_MULTI_CAPABILITY_E2E,
                "--skip",
                GENERATOR_SEALED_IMPORT_E2E,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("1 passed"));
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
