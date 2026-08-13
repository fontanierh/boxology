#![forbid(unsafe_code)]

use boxology_cli::{
    CheckComposition, ClassifierComposition, ExecuteError, PlanError, cargo_metadata_command,
    execute_plans, plan, project_check, walk,
};
use boxology_contract::BoxId;
use boxology_workspace::{Workspace, WorkspaceInputs};
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
const USAGE: &str = "usage: boxology generate\n       boxology generate --package <id>\n       boxology check\n       boxology check --base <revision>\n       boxology check --format human|json";

#[derive(Clone, Copy)]
enum CheckFormat {
    Human,
    Json,
}

enum Selection {
    Generate(Option<BoxId>),
    Check {
        base: Option<String>,
        format: CheckFormat,
    },
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
    if args == ["--help"] {
        let _ = writeln!(stdout, "{USAGE}");
        return 0;
    }
    let selection = match parse(args) {
        Ok(selection) => selection,
        Err(()) => {
            usage(stderr);
            return 2;
        }
    };
    match selection {
        Selection::Generate(package) => run_generate_setup(root, &package, stdout, stderr),
        Selection::Check { base, format } => run_check(base, format, stdout, stderr),
    }
}

fn run_generate_setup(
    root: &Path,
    package: &Option<BoxId>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let walked = match walk(root) {
        Ok(walked) => walked,
        Err(error) => {
            let _ = writeln!(stderr, "{error}");
            return 1;
        }
    };
    let metadata = read_metadata(root);
    let checked = metadata
        .as_ref()
        .ok()
        .and_then(|metadata| workspace_inputs(&walked, metadata).check().ok());
    if let Some(workspace) = checked {
        return run_generate(root, workspace, package, stdout, stderr);
    }

    let bootstrap = match workspace_inputs(&walked, "").check_for_generation() {
        Ok(workspace) => workspace,
        Err(findings) => {
            let _ = writeln!(stderr, "{findings}");
            return 1;
        }
    };
    let plans = match plan(&bootstrap, package.as_ref()) {
        Ok(plans) => plans,
        Err(error) => return report_plan_failure(error, CheckFormat::Human, stderr),
    };
    if !plans.iter().any(|plan| {
        let package_root = plan
            .package_root()
            .map_or_else(|| root.to_owned(), |path| root.join(path.as_str()));
        !package_root.join("generated/contract/Cargo.toml").is_file()
    }) {
        return match metadata {
            Ok(metadata) => report_workspace_failure(workspace_inputs(&walked, &metadata), stderr),
            Err(error) => report_metadata_failure(error, stderr),
        };
    }

    let code = run_generate(root, bootstrap, package, stdout, stderr);
    if code != 0 {
        return code;
    }
    validate_generated_workspace(root, stderr)
}

fn workspace_inputs(walked: &boxology_cli::WalkedWorkspace, metadata: &str) -> WorkspaceInputs {
    WorkspaceInputs::new(
        walked.files().to_vec(),
        walked.manifests().to_vec(),
        metadata,
    )
    .expect("the filesystem walk cannot produce duplicate logical paths")
}

fn report_workspace_failure(inputs: WorkspaceInputs, stderr: &mut dyn Write) -> u8 {
    match inputs.check() {
        Ok(_) => 0,
        Err(findings) => {
            let _ = writeln!(stderr, "{findings}");
            1
        }
    }
}

fn validate_generated_workspace(root: &Path, stderr: &mut dyn Write) -> u8 {
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
    report_workspace_failure(workspace_inputs(&walked, &metadata), stderr)
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
        Err(error) => return report_plan_failure(error, CheckFormat::Human, stderr),
    };
    let mut changed = false;
    let mut classifier = None;
    for step in execute_plans(root, &plans) {
        let (generation, outcome) = match step {
            Ok(step) => step,
            Err(error) => return report_execute_failure(error, CheckFormat::Human, stderr),
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
            if classifier.is_none() {
                classifier = match ClassifierComposition::start() {
                    Ok(classifier) => Some(classifier),
                    Err(error) => {
                        let _ = writeln!(stderr, "classifier composition: {error}");
                        return 1;
                    }
                };
            }
            match classifier
                .as_ref()
                .expect("classifier composition was initialized")
                .classify(outcome.base_schema(), outcome.submitted_schema())
            {
                Ok(report) => {
                    let _ = write!(stdout, "{}", report.rendered_text);
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
    base: Option<String>,
    format: CheckFormat,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let check = match CheckComposition::start() {
        Ok(check) => check,
        Err(error) => {
            let _ = writeln!(stderr, "check composition: {error}");
            return 1;
        }
    };
    let json = match format {
        CheckFormat::Human => false,
        CheckFormat::Json => true,
    };
    let projected = project_check(check.check(base), json);
    let _ = stdout.write_all(&projected.stdout);
    let _ = stderr.write_all(&projected.stderr);
    projected.code
}

fn parse(args: &[String]) -> Result<Selection, ()> {
    match args {
        [command] if command == "generate" => Ok(Selection::Generate(None)),
        [command, flag, package] if command == "generate" && flag == "--package" => {
            BoxId::new(package.clone())
                .map(|package| Selection::Generate(Some(package)))
                .map_err(|_| ())
        }
        [command, rest @ ..] if command == "check" => parse_check(rest),
        _ => Err(()),
    }
}

fn parse_check(args: &[String]) -> Result<Selection, ()> {
    let mut base = None;
    let mut format = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--base" => {
                if base.is_some() {
                    return Err(());
                }
                index += 1;
                let revision = args.get(index).ok_or(())?;
                if revision.is_empty() || revision.starts_with('-') {
                    return Err(());
                }
                base = Some(revision.clone());
            }
            "--format" => {
                if format.is_some() {
                    return Err(());
                }
                index += 1;
                let value = args.get(index).ok_or(())?;
                format = Some(match value.as_str() {
                    "human" => CheckFormat::Human,
                    "json" => CheckFormat::Json,
                    _ => return Err(()),
                });
            }
            _ => return Err(()),
        }
        index += 1;
    }
    Ok(Selection::Check {
        base,
        format: format.unwrap_or(CheckFormat::Human),
    })
}

fn usage(stderr: &mut dyn Write) {
    let _ = writeln!(stderr, "{USAGE}");
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

fn report_plan_failure(error: PlanError, format: CheckFormat, stderr: &mut dyn Write) -> u8 {
    match format {
        CheckFormat::Human => {
            let _ = writeln!(stderr, "{error}");
        }
        CheckFormat::Json => {
            let _ = write!(stderr, "{}", error.render_json());
        }
    }
    if error.is_unknown_package() { 2 } else { 1 }
}

fn report_execute_failure(error: ExecuteError, format: CheckFormat, stderr: &mut dyn Write) -> u8 {
    match (format, error.diagnostics()) {
        (CheckFormat::Json, Some(diagnostics)) => {
            let _ = write!(stderr, "{}", diagnostics.render_json());
        }
        _ => {
            let _ = writeln!(stderr, "{error}");
        }
    }
    1
}
