#![forbid(unsafe_code)]

use boxology_cli::{ExecuteError, PlanError, cargo_metadata_command, execute, plan, walk};
use boxology_contract::BoxId;
use boxology_workspace::WorkspaceInputs;
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
    let plans = match plan(&workspace, selection.as_ref()) {
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
    }
    let result = if changed { "changed" } else { "unchanged" };
    let _ = writeln!(stdout, "generate result {result}");
    0
}

fn parse(args: &[String]) -> Result<Option<BoxId>, ()> {
    match args {
        [command] if command == "generate" => Ok(None),
        [command, flag, package] if command == "generate" && flag == "--package" => {
            BoxId::new(package.clone()).map(Some).map_err(|_| ())
        }
        _ => Err(()),
    }
}

fn usage(stderr: &mut dyn Write) {
    let _ = writeln!(
        stderr,
        "usage: boxology generate\n       boxology generate --package <id>"
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
