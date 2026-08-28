use boxology_cli::walk;
use boxology_manifest::RelativePath;
use boxology_workspace::FileEntry;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use syn::visit::Visit;
const NAMES: &str = "lib.rs walk.rs generate.rs execute.rs compare.rs classify.rs check.rs base.rs runner.rs main.rs";
const FILES: &str = "Cargo.toml boxology.toml src/lib.rs src/main.rs tests/check_composition.rs tests/classifier_composition.rs tests/cli.rs tests/surface_lock.rs";
const CORE_FILES: &str = "Cargo.toml src/base.rs src/check.rs src/classify.rs src/compare.rs src/execute.rs src/generate.rs src/lib.rs src/runner.rs src/walk.rs tests/bxw.golden tests/check.rs tests/classify.rs tests/compare.rs tests/execute.rs tests/generation_plan.rs tests/runner.rs";
const PACKAGE: &str = include_str!("../Cargo.toml");
const CORE_PACKAGE: &str = include_str!("../../boxology-cli-core/Cargo.toml");
const FACADE: &str = include_str!("../src/lib.rs");
const PACKAGE_HASH: u64 = 17_610_981_520_671_971_549;
const CORE_PACKAGE_HASH: u64 = 1_800_175_424_823_312_622;
const FACADE_HASH: u64 = 17_028_389_826_028_418_546;
const LIB: &str = include_str!("../../boxology-cli-core/src/lib.rs");
const WALK: &str = include_str!("../../boxology-cli-core/src/walk.rs");
const GENERATE: &str = include_str!("../../boxology-cli-core/src/generate.rs");
const EXECUTE: &str = include_str!("../../boxology-cli-core/src/execute.rs");
const COMPARE: &str = include_str!("../../boxology-cli-core/src/compare.rs");
const CLASSIFY: &str = include_str!("../../boxology-cli-core/src/classify.rs");
const CHECK: &str = include_str!("../../boxology-cli-core/src/check.rs");
const BASE: &str = include_str!("../../boxology-cli-core/src/base.rs");
const RUNNER: &str = include_str!("../../boxology-cli-core/src/runner.rs");
const MAIN: &str = include_str!("../src/main.rs");
const SOURCES: &[(&str, &str)] = &[
    ("lib.rs", LIB),
    ("walk.rs", WALK),
    ("generate.rs", GENERATE),
    ("execute.rs", EXECUTE),
    ("compare.rs", COMPARE),
    ("classify.rs", CLASSIFY),
    ("check.rs", CHECK),
    ("base.rs", BASE),
    ("runner.rs", RUNNER),
    ("main.rs", MAIN),
];
const GOLDEN: &str = include_str!("../../boxology-cli-core/tests/bxw.golden");
const CODES: &str = "BXW0061 BXW0062 BXW0063 BXW0064 BXW0065 BXW0066 BXW0067 BXW0069 BXW0070 BXW0071 BXW0072 BXW0073 BXW0075 BXW0076 BXW0077 BXW0078 BXW0079 BXW0080 BXW0081 BXW0082 BXW0083 BXW0084 BXW0085 BXW0086 BXW0091 BXW0092 BXW0093 BXW0094 BXW0095 BXW0096 BXW0097 BXW0103 BXW0104 BXW0105 BXW0106 BXW0107";
const LIB_HASH: u64 = 11_652_192_000_931_803_304;
const WALK_HASH: u64 = 12_408_747_065_446_683_334;
const GENERATE_HASH: u64 = 5_898_066_629_614_279_057;
const EXECUTE_HASH: u64 = 15_138_597_921_723_061_807;
const COMPARE_HASH: u64 = 202_095_199_502_936_122;
const CLASSIFY_HASH: u64 = 17_939_391_275_069_315_174;
const CHECK_HASH: u64 = 16_295_088_236_220_879_986;
const BASE_HASH: u64 = 4_344_763_148_694_917_719;
const RUNNER_HASH: u64 = 14_976_080_536_641_793_496;
const MAIN_ANCHORS: &str = "env::args_os()\ncollect::<Result<Vec<_>, _>>()\nargs == [\"--help\"]\nwriteln!(stdout, \"{USAGE}\")\nSelection::Generate(package) => run_generate_setup(root, &package, stdout, stderr)\nSelection::Check { base, format } => run_check(base, format, stdout, stderr)\nfn run_generate_setup(\nplan(&bootstrap, package.as_ref())\nfn missing_contract_members(\nentry.role() == CrateRole::BoxContract\nmissing_contract_members.iter().any(|path| !path.is_file())\nfn validate_generated_workspace(\ncargo_metadata_command(root)\nstatus.success()\nString::from_utf8(stdout)\nWorkspaceInputs::new\ncheck_for_generation()\nvalidate_generated_workspace(root, stderr)\nplan(&workspace, package.as_ref())\nexecute_plans(root, &plans)\nClassifierComposition::start()\n.classify(outcome.base_schema(), outcome.submitted_schema())\nreport.rendered_text\nCheckComposition::start()\nCheckFormat::Human => false\nCheckFormat::Json => true\nproject_check(check.check(base), json)\nstdout.write_all(&projected.stdout)\nstderr.write_all(&projected.stderr)\nprojected.code\nerror.render_json()\ndiagnostics.render_json()\nBXW0075\nif error.is_unknown_package() { 2 } else { 1 }\nfn parse_check(args: &[String]) -> Result<Selection, ()>\n\"human\" => CheckFormat::Human\n\"json\" => CheckFormat::Json\n_ => Err(())";
const ARGV_SHAPE: &str = "pub const CARGO_METADATA_ARGS: [&str; 5] =\n    [\"metadata\", \"--format-version\", \"1\", \"--locked\", \"--no-deps\"];";
const MAIN_HASH: u64 = 11_601_466_459_558_289_107;
const HASHES: [u64; 10] = [
    LIB_HASH,
    WALK_HASH,
    GENERATE_HASH,
    EXECUTE_HASH,
    COMPARE_HASH,
    CLASSIFY_HASH,
    CHECK_HASH,
    BASE_HASH,
    RUNNER_HASH,
    MAIN_HASH,
];
const ANCHORS: &str = "symlink_metadata(root).is_ok_and\nsymlink_metadata(&cargo).is_ok_and\nentry.file_name() == \".git\"\nentry.file_name() == \"target\"\nlogical_path(root, &physical)?\nkind.is_symlink()\nfs::read_link(&physical)\nentry.file_name() == MANIFEST\nread_manifest(&physical, |path| fs::read(path))?\nfiles.sort_unstable_by\nmanifests.sort_unstable_by";
const GENERATE_ANCHORS: &str = "output.generator() == CARGO_GENERATOR\noutput.generator() == CONTRACT_GENERATOR\ntarget.id() == import.package()\nclassification.package() == package.id()\nclassification.derived_output().is_none()\nentry.role() == CrateRole::BoxImplementation\npackage.relative(classification.path())?\nserde_json::to_string(value)\nlet raw_root = implementations[0].path().nested().map_or_else(\n|| \"src/lib.rs\".to_owned(),\n|path| format!(\"{}/src/lib.rs\", path.as_str()),";
const EXECUTE_ANCHORS: &str = "fs::symlink_metadata(&path)\npattern.matches(&output)\nOUTPUTS.iter().map(|path| (*path).to_owned()).collect()\nboxology_generator_writer::write(&package_dir, &tree, plan.outputs())\nfor import in plan.imports()\nguarded(root, schema.as_str(), true)\nimport.package().clone()\nread_optional_file(root, plan.schema_path())\nfile.path() == package_schema_path(plan)";
const EXECUTE_PUBLIC: &str = "Outcome written removed is_unchanged base_schema submitted_schema ExecuteError code location path detail diagnostics write_error ExecutePlans execute_plans execute";
const COMPARE_ANCHORS: &str = "plan(workspace, None)\nclassification.derived_output() == Some(plan.derived_output_id())\npackage_relative(plan, classification.path())\nDifferenceKind::Stale\ndifferences.sort_by\nread_optional_file(root, plan.schema_path())\nworkspace.check_compositions(&schemas)\nBXW0083";
const COMPARE_PUBLIC: &str = "DifferenceKind as_str CompareDifference package path kind code detail repair_command rule_source CompareStepError compare_step compare_plans composition_step";
const CLASSIFY_ANCHORS: &str = "map_err(ClassifyError::base)\nmap_err(ClassifyError::submitted)\nmap_err(ClassifyError::pairing)\nboxology_classifier::classify(base.as_ref(), Some(&submitted))";
const CLASSIFY_PUBLIC: &str = "ClassifyError code side detail diagnostics classify";
const CHECK_ANCHORS: &str = "CHECK_BASE, \"base\", diagnostics)\nCHECK_SUBMITTED,\n            \"submitted\",\nCHECK_PAIRING,\n                \"pairing\",\nboxology_classifier::classify(base.as_ref(), Some(&submitted))";
const CHECK_PUBLIC: &str = "DuplicatePackages PackageSchemas new package base submitted CheckClassificationError package code side detail diagnostics ClassifyStepError classify_step";
const BASE_PUBLIC: &str = "GitToolError BaseError code location detail BaseSchemasError BaseInputsError DefaultBase ResolvedBase as_str from_oid resolve_default_base resolve_base base_package_schemas BaseDiffInputs packages submitted_packages changed is_bootstrapping base_paths manifest_changes base_diff_inputs base_diff_inputs_with_candidate";
const BASE_ANCHORS: &str = "[\"rev-parse\", \"--git-dir\"]\n[\"merge-base\", \"HEAD\", \"main\"]\n[\"rev-parse\", \"--verify\", \"--end-of-options\", &requested]\n[\"ls-tree\", \"-r\", \"-z\", base.as_str(), \"--\", \".\"]\nformat!(\"{oid}:./{}\", plan.schema_path().as_str())\n\"--relative\",\n\"--no-ext-diff\",\n[\"cat-file\", \"-e\", &object]\n[\"cat-file\", \"blob\", &object]\n[\"cat-file\", \"blob\", oid]\nread_optional_file(root, plan.schema_path())\nPackageSchemas::new(\nbase_diff_inputs_inner(root, base, None)\nSome(CandidateDeclarations {\ncandidate.is_some() && objects.is_empty()\nif self.bootstrapping {\nlet (base_packages, findings) = inputs.discover();\nlet submitted_packages = match candidate {\ncandidate.files.to_vec(), candidate.manifests.to_vec()\nsource.derived_output().is_none() && source.package() == held.package()";
const RUNNER_PUBLIC: &str = "CommandSpec new args render CapturedOutput new success combined SpawnError ToolStep into_parts run_command CommandRunner fmt_packages fmt_spec clippy_spec test_spec lock_spec run_fmt_step run_clippy_step run_test_step run_lock_step QualityCommand package manifest_path spec quality_specs run_quality_step";
const RUNNER_ANCHORS: &str = "classified.derived_output().is_none()\n\"fmt\".to_owned()\n\"-D\"\nBXW0093\nBXW0095\nBXW0096\nBXW0097\nBXW0107\n[\"metadata\", \"--format-version\", \"1\", \"--locked\"]\nformat!(\"command=\\\"{}\\\"\", spec.render())\nsplit_ascii_whitespace()\nquality_specs(workspace)\nCommand::new(&spec.program)";
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> Fixture {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("boxology-cli-surface-{}-{id}", std::process::id()));
    fs::create_dir(&root).expect("fixture root is new");
    Fixture(root)
}
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).expect("fixture parent can be created");
    fs::write(path, bytes).expect("fixture file can be written");
}
fn path(name: &str) -> RelativePath {
    RelativePath::new(name).expect("test path is valid")
}
fn files(names: &[&str]) -> Vec<FileEntry> {
    names
        .iter()
        .map(|name| FileEntry::file(path(name)))
        .collect()
}
fn error_is(error: boxology_cli::WalkError, code: &str, at: &Path, detail: &str) {
    assert_eq!(error.code(), code);
    assert_eq!(error.path(), at);
    assert_eq!(error.detail(), detail);
    assert_eq!(error.to_string(), format!("{code} {at:?}: {detail}"));
}
#[test]
fn root_gate_is_exact() {
    let fixture = fixture();
    let root = fixture.0.join("missing");
    error_is(
        walk(&root).expect_err("missing root must fail"),
        "BXW0061",
        &root,
        "workspace root must be a real directory containing a regular Cargo.toml",
    );
    let empty = fixture.0.join("empty");
    fs::create_dir(&empty).unwrap();
    error_is(
        walk(&empty).expect_err("root manifest is required"),
        "BXW0061",
        &empty.join("Cargo.toml"),
        "workspace root must be a real directory containing a regular Cargo.toml",
    );
}
#[cfg(unix)]
#[test]
fn symlink_root_is_rejected_before_external_ingestion() {
    use std::os::unix::fs::symlink;
    let fixture = fixture();
    let external = fixture.0.join("external");
    put(&external, "Cargo.toml", b"cargo");
    put(&external, "boxology.toml", b"must not be ingested");
    let root = fixture.0.join("root-link");
    symlink(&external, &root).expect("root symlink can be created");
    error_is(
        walk(&root).expect_err("symlink root must fail before traversal"),
        "BXW0061",
        &root,
        "workspace root must be a real directory containing a regular Cargo.toml",
    );
    let root = fixture.0.join("cargo-link-root");
    fs::create_dir(&root).unwrap();
    symlink(external.join("Cargo.toml"), root.join("Cargo.toml")).unwrap();
    error_is(
        walk(&root).expect_err("symlink root manifest must fail"),
        "BXW0061",
        &root.join("Cargo.toml"),
        "workspace root must be a real directory containing a regular Cargo.toml",
    );
}
#[test]
fn walk_is_opaque_sorted_and_exact() {
    let fixture = fixture();
    put(&fixture.0, "Zed.txt", b"zed");
    put(&fixture.0, "nested/boxology.toml", b"nested");
    put(&fixture.0, "apple.txt", b"apple");
    put(&fixture.0, "a/boxology.toml", b"a");
    put(&fixture.0, "boxology.toml", b"root");
    put(&fixture.0, "docs/not-boxology.toml", b"near miss");
    put(&fixture.0, "boxology.toml.bak", b"near miss");
    put(&fixture.0, "Cargo.toml", b"cargo");
    put(&fixture.0, ".git", b"gitdir: ../linked-worktree");
    put(&fixture.0, "target/debug/ignored", b"target");
    put(&fixture.0, "nested/.git/ignored", b"nested git");
    put(&fixture.0, "nested/.git/boxology.toml", b"excluded git");
    put(&fixture.0, "nested/target/ignored", b"nested target");
    put(
        &fixture.0,
        "nested/target/boxology.toml",
        b"excluded target",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        put(&fixture.0, "real/boxology.toml", b"real");
        put(&fixture.0, "real/hidden.txt", b"hidden");
        symlink("real", fixture.0.join("alias")).expect("symlink can be created");
    }
    let walked = walk(&fixture.0).expect("fixture is walkable");
    let mut expected = files(&[
        "Cargo.toml",
        "Zed.txt",
        "a/boxology.toml",
        "apple.txt",
        "boxology.toml",
        "boxology.toml.bak",
        "docs/not-boxology.toml",
        "nested/boxology.toml",
    ]);
    #[cfg(unix)]
    {
        expected.insert(3, FileEntry::symlink(path("alias"), "real".to_owned()));
        expected.insert(9, FileEntry::file(path("real/boxology.toml")));
        expected.insert(10, FileEntry::file(path("real/hidden.txt")));
    }
    assert_eq!(walked.files(), expected.as_slice());
    let mut manifests = vec![
        (path("a/boxology.toml"), b"a".to_vec()),
        (path("boxology.toml"), b"root".to_vec()),
        (path("nested/boxology.toml"), b"nested".to_vec()),
    ];
    #[cfg(unix)]
    manifests.push((path("real/boxology.toml"), b"real".to_vec()));
    assert_eq!(walked.manifests(), manifests.as_slice());
}
#[cfg(unix)]
#[test]
fn invalid_path_is_not_skipped_on_unix() {
    let fixture = fixture();
    put(&fixture.0, "Cargo.toml", b"cargo");
    let invalid = fixture.0.join("bad\nname");
    fs::write(&invalid, b"bad").unwrap();
    error_is(
        walk(&fixture.0).expect_err("invalid path must not be skipped"),
        "BXW0063",
        &invalid,
        "walked name/path is not a valid RelativePath",
    );
}
#[test]
fn source_surface_is_exact_and_mutation_resistant() {
    assert!(locked());
    let (production, module) = WALK.split_once("\n#[cfg(test)]").unwrap();
    let cases = vec![
        format!("{WALK}\nconst AFTER_TESTS: u8 = 0;"),
        format!("{production}\nconst BAD: &str = \"BXC9999\";\n#[cfg(test)]{module}"),
        WALK.replace("PR1 task authority", "mutant authority"),
        WALK.replace(
            "read_manifest(&physical, |path| fs::read(path))?",
            "Vec::new() /* unreachable IO */",
        ),
        format!("{production}\n#[cfg(test)]\nmod tests {{}}\n"),
        format!("#[cfg(test)]{module}\n{production}"),
        format!("{WALK}\n// {}", ANCHORS.lines().next().unwrap()),
    ];
    for source in cases {
        rejects(
            [LIB, &source, GENERATE, EXECUTE, CLASSIFY, CHECK, MAIN],
            [
                LIB_HASH,
                hash(&source),
                GENERATE_HASH,
                EXECUTE_HASH,
                CLASSIFY_HASH,
                CHECK_HASH,
                hash(MAIN),
            ],
        );
    }
    let mutant = format!("{LIB}// hash mutant\n");
    rejects(
        [&mutant, WALK, GENERATE, EXECUTE, CLASSIFY, CHECK, MAIN],
        HASHES,
    );
    let mutant = format!("{WALK}// hash mutant\n");
    rejects(
        [LIB, &mutant, GENERATE, EXECUTE, CLASSIFY, CHECK, MAIN],
        HASHES,
    );
    let mutant = format!("{GENERATE}// hash mutant\n");
    rejects([LIB, WALK, &mutant, EXECUTE, CLASSIFY, CHECK, MAIN], HASHES);
    let mutant = format!("{EXECUTE}\n/// Mutant.\npub fn mutant() {{}}\n");
    rejects(
        [LIB, WALK, GENERATE, &mutant, CLASSIFY, CHECK, MAIN],
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            hash(&mutant),
            CLASSIFY_HASH,
            CHECK_HASH,
            MAIN_HASH,
        ],
    );
    let mutant = EXECUTE.replace("BXW0070", "BXW9999");
    rejects(
        [LIB, WALK, GENERATE, &mutant, CLASSIFY, CHECK, MAIN],
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            hash(&mutant),
            CLASSIFY_HASH,
            CHECK_HASH,
            MAIN_HASH,
        ],
    );
    for (anchor, replacement) in [
        ("fs::symlink_metadata(&path)", "fs::metadata(&path)"),
        (
            "pattern.matches(&output)",
            "pattern.as_str() == output.as_str()",
        ),
        (
            "OUTPUTS.iter().map(|path| (*path).to_owned()).collect()",
            "Vec::new()",
        ),
        (
            "boxology_generator_writer::write(&package_dir, &tree, plan.outputs())",
            "boxology_generator_writer::write(root, &tree, plan.outputs())",
        ),
        (
            "read_optional_file(root, plan.schema_path())",
            "read_optional_file(root, plan.crate_root())",
        ),
        (
            "file.path() == package_schema_path(plan)",
            "file.path() == OUTPUTS[3]",
        ),
    ] {
        let changed = EXECUTE.replace(anchor, replacement);
        rejects(
            [LIB, WALK, GENERATE, &changed, CLASSIFY, CHECK, MAIN],
            [
                LIB_HASH,
                WALK_HASH,
                GENERATE_HASH,
                hash(&changed),
                CLASSIFY_HASH,
                CHECK_HASH,
                hash(MAIN),
            ],
        );
    }
    for (anchor, replacement) in [
        (
            "output.generator() == CARGO_GENERATOR",
            "output.generator() == CONTRACT_GENERATOR",
        ),
        (
            "classification.package() == package.id()",
            "classification.package() != package.id()",
        ),
        (
            "classification.derived_output().is_none()",
            "classification.derived_output().is_some()",
        ),
        (
            r#"implementations[0].path().nested().map_or_else(
        || "src/lib.rs".to_owned(),
        |path| format!("{}/src/lib.rs", path.as_str()),
    )"#,
            r#"format!("{}/src/lib.rs", implementations[0].path().as_str())"#,
        ),
    ] {
        let changed = GENERATE.replace(anchor, replacement);
        rejects(
            [LIB, WALK, &changed, EXECUTE, CLASSIFY, CHECK, MAIN],
            [
                LIB_HASH,
                WALK_HASH,
                hash(&changed),
                EXECUTE_HASH,
                CLASSIFY_HASH,
                CHECK_HASH,
                MAIN_HASH,
            ],
        );
    }
    for (needle, replacement) in [
        ("\"--locked\", ", ""),
        ("\"--no-deps\"", "\"--no-deps\", \"--offline\""),
    ] {
        let changed = LIB.replace(needle, replacement);
        rejects(
            [&changed, WALK, GENERATE, EXECUTE, CLASSIFY, CHECK, MAIN],
            [
                hash(&changed),
                WALK_HASH,
                GENERATE_HASH,
                EXECUTE_HASH,
                CLASSIFY_HASH,
                CHECK_HASH,
                MAIN_HASH,
            ],
        );
    }
    let duplicate = format!("{MAIN}\nconst DUPLICATE: &str = \"BXW0075\";\n");
    rejects(
        [LIB, WALK, GENERATE, EXECUTE, CLASSIFY, CHECK, &duplicate],
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            EXECUTE_HASH,
            CLASSIFY_HASH,
            CHECK_HASH,
            hash(&duplicate),
        ],
    );
    let reworded = MAIN.replace(
        "cargo metadata could not be executed or did not return valid workspace metadata",
        "metadata failed",
    );
    rejects(
        [LIB, WALK, GENERATE, EXECUTE, CLASSIFY, CHECK, &reworded],
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            EXECUTE_HASH,
            CLASSIFY_HASH,
            CHECK_HASH,
            hash(&reworded),
        ],
    );
    let second_exit = MAIN.replace(
        "if error.is_unknown_package() { 2 } else { 1 }",
        "if error.is_unknown_package() { 2 } else { 2 }",
    );
    rejects(
        [LIB, WALK, GENERATE, EXECUTE, CLASSIFY, CHECK, &second_exit],
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            EXECUTE_HASH,
            CLASSIFY_HASH,
            CHECK_HASH,
            hash(&second_exit),
        ],
    );
    let fallthrough = MAIN.replace("_ => Err(())", "_ => Ok(None)");
    rejects(
        [LIB, WALK, GENERATE, EXECUTE, CLASSIFY, CHECK, &fallthrough],
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            EXECUTE_HASH,
            CLASSIFY_HASH,
            CHECK_HASH,
            hash(&fallthrough),
        ],
    );
    for (anchor, replacement) in [
        (
            "plan(&bootstrap, package.as_ref())",
            "plan(&bootstrap, None)",
        ),
        (
            "entry.role() == CrateRole::BoxContract",
            "entry.role() != CrateRole::BoxContract",
        ),
        (
            "missing_contract_members.iter().any(|path| !path.is_file())",
            "missing_contract_members.is_empty()",
        ),
    ] {
        let changed = MAIN.replace(anchor, replacement);
        rejects(
            [LIB, WALK, GENERATE, EXECUTE, CLASSIFY, CHECK, &changed],
            [
                LIB_HASH,
                WALK_HASH,
                GENERATE_HASH,
                EXECUTE_HASH,
                CLASSIFY_HASH,
                CHECK_HASH,
                hash(&changed),
            ],
        );
    }
    for (anchor, replacement) in [
        ("if self.bootstrapping {", "if !self.bootstrapping {"),
        (
            "let (base_packages, findings) = inputs.discover();",
            "let (wrong_packages, findings) = inputs.discover();",
        ),
        (
            "let submitted_packages = match candidate {",
            "let submitted_packages = Vec::new(); match candidate {",
        ),
        (
            "candidate.files.to_vec(), candidate.manifests.to_vec()",
            "Vec::new(), Vec::new()",
        ),
        (
            "source.derived_output().is_none() && source.package() == held.package()",
            "source.derived_output().is_some() && source.package() == held.package()",
        ),
    ] {
        let changed = BASE.replace(anchor, replacement);
        let mut sources = SOURCES.to_vec();
        sources[7] = ("base.rs", &changed);
        let mut hashes = HASHES;
        hashes[7] = hash(&changed);
        assert!(!locked_sources(&sources, hashes, GOLDEN));
    }
    let extra = [
        ("lib.rs", LIB),
        ("walk.rs", WALK),
        ("generate.rs", GENERATE),
        ("execute.rs", EXECUTE),
        ("classify.rs", CLASSIFY),
        ("check.rs", CHECK),
        ("main.rs", MAIN),
        ("extra.rs", ""),
    ];
    assert!(!locked_sources(&extra, HASHES, GOLDEN));
    let golden_mutant = format!("{GOLDEN}mutant");
    assert!(!locked_sources(SOURCES, HASHES, &golden_mutant));
    let files: Vec<_> = FILES.split_whitespace().map(str::to_owned).collect();
    for extra in ["build.rs", "src/bin/hidden.rs"] {
        let mut mutant = files.clone();
        mutant.push(extra.to_owned());
        mutant.sort_unstable();
        assert!(!package_is(PACKAGE, PACKAGE_HASH, &mutant, FILES));
    }
    for mutant in [
        format!("{PACKAGE}\n[lib]\npath = \"src/walk.rs\"\n"),
        format!("{PACKAGE}\n[[example]]\nname = \"escape\"\npath = \"src/lib.rs\"\n"),
    ] {
        assert!(!package_is(&mutant, PACKAGE_HASH, &files, FILES));
    }
}
fn rejects<const N: usize, const M: usize>(bodies: [&str; N], hashes: [u64; M]) {
    let names = [
        "lib.rs",
        "walk.rs",
        "generate.rs",
        "execute.rs",
        "classify.rs",
        "check.rs",
        "main.rs",
    ];
    let mut sources = Vec::with_capacity(N + 3);
    for (index, body) in bodies.iter().enumerate() {
        sources.push((names[index], *body));
        if index == 3 {
            sources.push(("compare.rs", COMPARE));
        }
        if index == 5 {
            sources.push(("base.rs", BASE));
            sources.push(("runner.rs", RUNNER));
        }
    }
    let mut expected = hashes.to_vec();
    if expected.len() == 7 {
        expected.insert(4, COMPARE_HASH);
        expected.insert(7, BASE_HASH);
        expected.insert(8, RUNNER_HASH);
    }
    assert!(!locked_sources(&sources, expected, GOLDEN));
}

#[derive(Default)]
struct Lock {
    codes: Vec<String>,
    constants: BTreeMap<String, String>,
    rules: Vec<(String, String, String)>,
    public: Vec<String>,
    next_public: bool,
    bad: bool,
    tests: usize,
}
impl<'ast> Visit<'ast> for Lock {
    fn visit_visibility(&mut self, visibility: &'ast syn::Visibility) {
        self.next_public = matches!(visibility, syn::Visibility::Public(_));
    }
    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        if std::mem::take(&mut self.next_public) {
            self.public.push(ident.to_string());
        }
    }
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        self.bad |= attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr");
        syn::visit::visit_attribute(self, attr);
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if let syn::Expr::Lit(expression) = item.expr.as_ref()
            && let syn::Lit::Str(value) = &expression.lit
        {
            self.constants.insert(item.ident.to_string(), value.value());
        }
        if type_ident(item.ty.as_ref()).as_deref() == Some("Rule")
            && let syn::Expr::Tuple(tuple) = item.expr.as_ref()
            && let [code, text, source] = tuple.elems.iter().collect::<Vec<_>>().as_slice()
            && let (Some(code), Some(text), Some(source)) = (
                literal(code),
                expression_ident(text),
                expression_ident(source),
            )
        {
            self.rules.push((code, text, source));
        }
        syn::visit::visit_item_const(self, item);
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        self.bad = true;
        syn::visit::visit_item_mod(self, item);
    }
    fn visit_lit_byte_str(&mut self, _: &'ast syn::LitByteStr) {
        self.bad = true;
    }
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if diagnostic(&literal.value()) {
            self.codes.push(literal.value());
        }
    }
    fn visit_macro(&mut self, called: &'ast syn::Macro) {
        self.bad |= !called.path.is_ident("write")
            && !called.path.is_ident("writeln")
            && !called.path.is_ident("format");
        syn::visit::visit_macro(self, called);
    }
    fn visit_use_glob(&mut self, _: &'ast syn::UseGlob) {
        self.bad = true;
    }
}
fn locked() -> bool {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let core = root.join("../boxology-cli-core");
    let mut files = Vec::new();
    let inventory = package_files(root, root, &mut files);
    files.sort_unstable();
    let mut core_files = Vec::new();
    let core_inventory = package_files(&core, &core, &mut core_files);
    core_files.sort_unstable();
    inventory
        && core_inventory
        && package_is(PACKAGE, PACKAGE_HASH, &files, FILES)
        && package_is(CORE_PACKAGE, CORE_PACKAGE_HASH, &core_files, CORE_FILES)
        && fs::read_to_string(root.join("Cargo.toml")).is_ok_and(|text| text == PACKAGE)
        && fs::read_to_string(core.join("Cargo.toml")).is_ok_and(|text| text == CORE_PACKAGE)
        && fs::read_to_string(root.join("src/lib.rs")).is_ok_and(|text| text == FACADE)
        && hash(FACADE) == FACADE_HASH
        && SOURCES.iter().all(|(name, source)| {
            let owner = if *name == "main.rs" { root } else { &core };
            fs::read_to_string(owner.join("src").join(name)).is_ok_and(|text| text == *source)
        })
        && locked_sources(SOURCES, HASHES, GOLDEN)
}
fn package_files(root: &Path, directory: &Path, files: &mut Vec<String>) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            return false;
        };
        if kind.is_dir() {
            if !package_files(root, &path, files) {
                return false;
            }
        } else if kind.is_file() {
            let Some(name) = path.strip_prefix(root).ok().and_then(Path::to_str) else {
                return false;
            };
            files.push(name.replace(std::path::MAIN_SEPARATOR, "/"));
        } else if !(kind.is_symlink()
            && directory == root
            && matches!(
                entry.file_name().to_str(),
                Some("LICENSE-APACHE" | "LICENSE-MIT")
            ))
        {
            return false;
        }
    }
    true
}
fn package_is(manifest: &str, expected_hash: u64, files: &[String], expected_files: &str) -> bool {
    hash(manifest) == expected_hash && files.join(" ") == expected_files
}
fn locked_sources(sources: &[(&str, &str)], hashes: impl AsRef<[u64]>, golden: &str) -> bool {
    let hashes = hashes.as_ref();
    if !sources
        .iter()
        .map(|(name, _)| *name)
        .eq(NAMES.split_whitespace())
    {
        return false;
    }
    if sources.len() != hashes.len()
        || sources
            .iter()
            .zip(hashes.iter().copied())
            .any(|((_, source), expected)| hash(source) != expected)
    {
        return false;
    }
    let mut lock = Lock::default();
    let mut execute_public = Vec::new();
    let mut compare_public = Vec::new();
    let mut classify_public = Vec::new();
    let mut check_public = Vec::new();
    let mut base_public = Vec::new();
    let mut runner_public = Vec::new();
    for &(name, source) in sources.iter().skip(1) {
        lock.public.clear();
        let Ok(file) = syn::parse_file(source) else {
            return false;
        };
        for attr in &file.attrs {
            lock.visit_attribute(attr);
        }
        for (position, item) in file.items.iter().enumerate() {
            if name == "walk.rs" && test_module(item) {
                lock.tests += usize::from(position + 1 == file.items.len());
            } else {
                lock.visit_item(item);
            }
        }
        if name == "execute.rs" {
            execute_public = lock.public.clone();
        }
        if name == "compare.rs" {
            compare_public = lock.public.clone();
        }
        if name == "classify.rs" {
            classify_public = lock.public.clone();
        }
        if name == "check.rs" {
            check_public = lock.public.clone();
        }
        if name == "base.rs" {
            base_public = lock.public.clone();
        }
        if name == "runner.rs" {
            runner_public = lock.public.clone();
        }
    }
    let codes = CODES
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lock.codes.sort_unstable();
    lock.rules.sort_unstable();
    !lock.bad
        && lock.tests == 1
        && lock.codes == codes
        && execute_public.join(" ") == EXECUTE_PUBLIC
        && compare_public.join(" ") == COMPARE_PUBLIC
        && classify_public.join(" ") == CLASSIFY_PUBLIC
        && check_public.join(" ") == CHECK_PUBLIC
        && base_public.join(" ") == BASE_PUBLIC
        && runner_public.join(" ") == RUNNER_PUBLIC
        && render(&lock).is_some_and(|rendered| rendered == golden)
        && ANCHORS
            .lines()
            .all(|anchor| sources[1].1.matches(anchor).count() == 1)
        && GENERATE_ANCHORS
            .lines()
            .all(|anchor| sources[2].1.matches(anchor).count() == 1)
        && EXECUTE_ANCHORS
            .lines()
            .all(|anchor| sources[3].1.matches(anchor).count() == 1)
        && COMPARE_ANCHORS
            .lines()
            .all(|anchor| sources[4].1.matches(anchor).count() == 1)
        && CLASSIFY_ANCHORS
            .lines()
            .all(|anchor| sources[5].1.matches(anchor).count() == 1)
        && CHECK_ANCHORS
            .lines()
            .all(|anchor| sources[6].1.matches(anchor).count() == 1)
        && BASE_ANCHORS
            .lines()
            .all(|anchor| sources[7].1.matches(anchor).count() == 1)
        && RUNNER_ANCHORS
            .lines()
            .all(|anchor| sources[8].1.matches(anchor).count() == 1)
        && MAIN_ANCHORS
            .lines()
            .all(|anchor| sources[9].1.matches(anchor).count() == 1)
        && sources[0].1.matches(ARGV_SHAPE).count() == 1
        && sources[0].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[0].1.matches("#![forbid(unsafe_code)]").count() == 1
        && sources[3].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[3].1.matches("#![forbid(unsafe_code)]").count() == 1
        && sources[4].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[4].1.matches("#![forbid(unsafe_code)]").count() == 1
        && sources[5].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[5].1.matches("#![forbid(unsafe_code)]").count() == 1
        && sources[6].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[6].1.matches("#![forbid(unsafe_code)]").count() == 1
        && sources[7].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[7].1.matches("#![forbid(unsafe_code)]").count() == 1
        && sources[8].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[8].1.matches("#![forbid(unsafe_code)]").count() == 1
}
fn render(lock: &Lock) -> Option<String> {
    let mut output = format!("sources={}\n", NAMES.replace(' ', ","));
    for (code, text, source) in &lock.rules {
        output.push_str(&format!(
            "{code}|{}|{}\n",
            lock.constants.get(text)?,
            lock.constants.get(source)?
        ));
    }
    Some(output)
}
fn type_ident(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    Some(path.path.segments.last()?.ident.to_string())
}
fn expression_ident(expression: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expression else {
        return None;
    };
    Some(path.path.segments.last()?.ident.to_string())
}
fn literal(expression: &syn::Expr) -> Option<String> {
    let syn::Expr::Lit(expression) = expression else {
        return None;
    };
    let syn::Lit::Str(value) = &expression.lit else {
        return None;
    };
    Some(value.value())
}
fn diagnostic(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes.starts_with(b"BX")
        && bytes[2].is_ascii_uppercase()
        && bytes[3..].iter().all(u8::is_ascii_digit)
}
fn hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
fn test_module(item: &syn::Item) -> bool {
    let syn::Item::Mod(module) = item else {
        return false;
    };
    let Some((_, items)) = &module.content else {
        return false;
    };
    let [syn::Item::Use(_), syn::Item::Use(_), syn::Item::Fn(test)] = items.as_slice() else {
        return false;
    };
    module.ident == "tests"
        && module.attrs.len() == 1
        && matches!(&module.attrs[0].meta, syn::Meta::List(meta)
            if meta.path.is_ident("cfg") && meta.tokens.to_string() == "test")
        && test.sig.ident == "refused_manifest_read_is_stable_and_payload_safe"
        && test.attrs.len() == 1
        && test.attrs[0].path().is_ident("test")
}
