#![forbid(unsafe_code)]

use boxology_cli::{
    BaseSchemasError, ClassifyStepError, CompareDifference, DefaultBase, ExecuteError,
    GenerationPlan, PlanError, ResolvedBase, SpawnError, base_package_schemas,
    cargo_metadata_command, classify_step, compare_plans, composition_step, execute, plan,
    resolve_base, resolve_default_base, run_clippy_step, run_command, run_fmt_step, run_lock_step,
    run_test_step, walk,
};
use boxology_contract::BoxId;
use boxology_manifest::RelativePath;
use boxology_workspace::{
    CheckReport, Completion, ContractClassificationCompletion, DiffOwnershipCompletion,
    DiffOwnershipSkip, Entry, ExternalOutput, Finding, Findings, SkipReason, StepSkip, Workspace,
    WorkspaceInputs,
};
use std::{
    env,
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

type Rule = (&'static str, &'static str, &'static str);
const METADATA_SOURCE: &str = "specs/s5-manifest-and-validation.md D4";
const METADATA_TEXT: &str =
    "cargo metadata could not be executed or did not return valid workspace metadata";
const METADATA: Rule = ("BXW0075", METADATA_TEXT, METADATA_SOURCE);

enum Selection {
    Generate(Option<BoxId>),
    Check(Option<String>),
}

struct MetadataFailure {
    stderr: Vec<u8>,
}

fn main() -> ExitCode {
    let args = env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect::<Result<Vec<_>, _>>();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let code = match args {
        Ok(args) => run(&args, Path::new("."), &mut stdout, &mut stderr),
        Err(_) => {
            usage(&mut stderr);
            2
        }
    };
    ExitCode::from(code)
}

fn run(args: &[String], root: &Path, stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let selection = match parse(args) {
        Ok(selection) => selection,
        Err(()) => {
            usage(stderr);
            return 2;
        }
    };
    let walked = match walk(root) {
        Ok(walked) => walked,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            return 1;
        }
    };
    let metadata = match read_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) => return report_metadata_failure(error, stderr),
    };
    let inputs = WorkspaceInputs::new(
        walked.files().to_vec(),
        walked.manifests().to_vec(),
        &metadata,
    )
    .expect("the filesystem walk cannot produce duplicate logical paths");
    let workspace = match inputs.check() {
        Ok(workspace) => workspace,
        Err(findings) => {
            let _ = writeln!(stderr, "{findings}");
            return 1;
        }
    };
    match selection {
        Selection::Generate(package) => run_generate(root, workspace, &package, stdout, stderr),
        Selection::Check(base) => run_check(root, workspace, base.as_deref(), stdout, stderr),
    }
}

fn run_generate(
    root: &Path,
    workspace: Workspace,
    package: &Option<BoxId>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let plans = match plan(&workspace, package.as_ref()) {
        Ok(plans) => plans,
        Err(error) => return report_plan_failure(error, stderr),
    };
    let mut changed = false;
    for generation in &plans {
        let outcome = match execute(root, generation) {
            Ok(outcome) => outcome,
            Err(error) => return report_execute_failure(error, stderr),
        };
        changed |= !outcome.is_unchanged();
        let state = if outcome.is_unchanged() {
            "unchanged"
        } else {
            "written"
        };
        let _ = writeln!(stdout, "generate {} {state}", generation.package_id());
        for path in outcome.written() {
            let _ = writeln!(stdout, "  written {path}");
        }
        for path in outcome.removed() {
            let _ = writeln!(stdout, "  removed {path}");
        }
        if !outcome.is_unchanged() {
            match boxology_cli::classify(outcome.base_schema(), outcome.submitted_schema()) {
                Ok(report) => {
                    let _ = write!(stdout, "{}", boxology_classifier::render_text(&report));
                }
                Err(error) => {
                    let _ = writeln!(stderr, "{error}");
                    return 1;
                }
            }
        }
    }
    let result = if changed { "changed" } else { "unchanged" };
    let _ = writeln!(stdout, "generate result {result}");
    0
}

fn run_check(
    root: &Path,
    workspace: Workspace,
    base: Option<&str>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let plans = match plan(&workspace, None) {
        Ok(plans) => plans,
        Err(error) => return report_plan_failure(error, stderr),
    };
    let discovery = match composition_step(root, &workspace, &plans) {
        Ok(discovery) => discovery,
        Err(error) => return report_execute_failure(error, stderr),
    };
    let differences = match compare_plans(root, &workspace, &plans) {
        Ok(differences) => differences,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            return 1;
        }
    };
    let regeneration = if differences.is_empty() {
        Completion::Passed
    } else {
        let entries = differences
            .iter()
            .map(|difference| Entry::Workspace(difference_finding(&workspace, difference)))
            .collect();
        Completion::Failed(Findings::new(entries).expect("differences produce findings"))
    };
    let resolved = match base {
        None => match resolve_default_base(root) {
            Ok(DefaultBase::NoRepository) => Err(SkipReason::NoRepository),
            Ok(DefaultBase::NoMergeBase) => Err(SkipReason::NoMergeBase),
            Ok(DefaultBase::Commit(oid)) => match ResolvedBase::from_oid(oid) {
                Ok(base) => Ok(base),
                Err(error) => {
                    let _ = writeln!(stderr, "{error}");
                    return 1;
                }
            },
            Err(error) => {
                let _ = writeln!(stderr, "{error}");
                return 2;
            }
        },
        Some(revision) => match resolve_base(root, revision) {
            Ok(base) => Ok(base),
            Err(error) => return report_base_failure(error, stderr),
        },
    };
    let contract_classification = match resolved {
        Err(reason) => ContractClassificationCompletion::Skipped(reason),
        Ok(base) => match classify_contracts(root, &base, &plans, stderr) {
            Ok(completion) => completion,
            Err(code) => return code,
        },
    };
    let runner = &run_command;
    let (cargo_graph, cargo_graph_output) = match run_lock_step(runner, root) {
        Ok(step) => step.into_parts(),
        Err(error) => return report_spawn_failure(error, stderr),
    };
    let (fmt, fmt_output) = match run_fmt_step(runner, root, &workspace) {
        Ok(step) => step.into_parts(),
        Err(error) => return report_spawn_failure(error, stderr),
    };
    let (clippy, clippy_output) = match run_clippy_step(runner, root) {
        Ok(step) => step.into_parts(),
        Err(error) => return report_spawn_failure(error, stderr),
    };
    let (tests, tests_output) = match run_test_step(runner, root) {
        Ok(step) => step.into_parts(),
        Err(error) => return report_spawn_failure(error, stderr),
    };
    let report = CheckReport {
        discovery,
        regeneration,
        contract_classification,
        diff_ownership: DiffOwnershipCompletion::Skipped(DiffOwnershipSkip::NotImplemented),
        cargo_graph,
        fmt,
        clippy,
        tests,
        quality: not_implemented(),
        external_output: ExternalOutput {
            cargo_graph: cargo_graph_output,
            fmt: fmt_output,
            clippy: clippy_output,
            tests: tests_output,
            quality: None,
        },
    };
    let _ = writeln!(stdout, "{}", report.render_human());
    report.exit_code()
}

fn difference_finding(workspace: &Workspace, difference: &CompareDifference) -> Finding {
    let package = workspace
        .packages()
        .iter()
        .find(|package| package.id() == difference.package())
        .expect("every compare difference belongs to a workspace package");
    let path = match package.root() {
        Some(root) => {
            RelativePath::new(format!("{}/{}", root.as_str(), difference.path().as_str()))
                .expect("package-root prefix is a valid relative path")
        }
        None => difference.path().clone(),
    };
    Finding::external(
        difference.code(),
        difference.detail(),
        difference.rule_source(),
        path,
        Some(difference.package().clone()),
        format!(
            "kind={} repair=\"{}\"",
            difference.kind().as_str(),
            difference.repair_command()
        ),
    )
}

fn not_implemented() -> Completion {
    Completion::Skipped(StepSkip::NotImplemented)
}

fn classify_contracts(
    root: &Path,
    base: &ResolvedBase,
    plans: &[GenerationPlan],
    stderr: &mut dyn Write,
) -> Result<ContractClassificationCompletion, u8> {
    let schemas = match base_package_schemas(root, base, plans) {
        Ok(schemas) => schemas,
        Err(error) => return Err(report_base_failure(error, stderr)),
    };
    match classify_step(&schemas) {
        Ok(completion) => Ok(completion),
        Err(error) => Err(report_classification_failure(error, stderr)),
    }
}

fn parse(args: &[String]) -> Result<Selection, ()> {
    match args {
        [command] if command == "generate" => Ok(Selection::Generate(None)),
        [command, flag, package] if command == "generate" && flag == "--package" => {
            BoxId::new(package.clone())
                .map(|package| Selection::Generate(Some(package)))
                .map_err(|_| ())
        }
        [command] if command == "check" => Ok(Selection::Check(None)),
        [command, flag, revision]
            if command == "check"
                && flag == "--base"
                && !revision.is_empty()
                && !revision.starts_with('-') =>
        {
            Ok(Selection::Check(Some(revision.clone())))
        }
        _ => Err(()),
    }
}

fn usage(stderr: &mut dyn Write) {
    let _ = writeln!(
        stderr,
        "usage: boxology generate\n       boxology generate --package <id>\n       boxology check\n       boxology check --base <revision>"
    );
}

fn read_metadata(root: &Path) -> Result<String, MetadataFailure> {
    let output = cargo_metadata_command(root)
        .output()
        .map_err(|_| MetadataFailure { stderr: Vec::new() })?;
    let std::process::Output {
        status,
        stdout,
        stderr,
    } = output;
    if !status.success() {
        return Err(MetadataFailure { stderr });
    }
    String::from_utf8(stdout).map_err(|_| MetadataFailure { stderr })
}

fn report_metadata_failure(error: MetadataFailure, stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(stderr, "{} Cargo.toml: {}", METADATA.0, METADATA.1);
    if !error.stderr.is_empty() {
        let _ = stderr.write_all(&error.stderr);
        if error.stderr.last() != Some(&b'\n') {
            let _ = stderr.write_all("\n".as_bytes());
        }
    }
    2
}

fn report_plan_failure(error: PlanError, stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(stderr, "{error}");
    if error.is_unknown_package() { 2 } else { 1 }
}

fn report_execute_failure(error: ExecuteError, stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(stderr, "{error}");
    1
}

fn report_base_failure(error: BaseSchemasError, stderr: &mut dyn Write) -> u8 {
    match error {
        BaseSchemasError::Tool(error) => {
            let _ = writeln!(stderr, "{error}");
            return 2;
        }
        BaseSchemasError::Git(error) => {
            let _ = writeln!(stderr, "{error}");
        }
        BaseSchemasError::Submitted(error) => {
            let _ = writeln!(stderr, "{error}");
        }
    }
    1
}

fn report_classification_failure(error: ClassifyStepError, stderr: &mut dyn Write) -> u8 {
    match error {
        ClassifyStepError::Classification(error) => {
            let _ = writeln!(stderr, "{error}");
        }
        ClassifyStepError::Duplicate(_) => {}
    }
    1
}

fn report_spawn_failure(error: SpawnError, stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(stderr, "{error}");
    2
}
