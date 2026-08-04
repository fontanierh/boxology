//! Trusted command runner for `boxology check` lock, fmt, Clippy, test, and quality steps.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::BoxId;
use boxology_manifest::RelativePath;
use boxology_workspace::{Completion, Entry, Finding, Findings, Workspace};
use std::{fmt, path::Path, process::Command};

type Rule = (&'static str, &'static str, &'static str);
const TOOL_SOURCE: &str = "boxology-details/08-rust-build-topology.md workspace operations and validation baseline; specs/s5-manifest-and-validation.md D6";
const LOCK_SOURCE: &str = "boxology-details/08-rust-build-topology.md workspace operations and validation baseline step 4; specs/s5-manifest-and-validation.md D6";
const QUALITY_SOURCE: &str = "boxology-details/08-rust-build-topology.md workspace operations and validation baseline step 8; specs/s5-manifest-and-validation.md D6";
const FMT_TEXT: &str = "formatting check failed";
const CLIPPY_TEXT: &str = "clippy check failed";
const TESTS_TEXT: &str = "test check failed";
const LOCK_TEXT: &str = "cargo graph and lockfile freshness check failed";
const INVOKE_TEXT: &str = "a trusted check command could not be executed";
const QUALITY_TEXT: &str = "a declared quality command failed";
const FMT: Rule = ("BXW0093", FMT_TEXT, TOOL_SOURCE);
const CLIPPY: Rule = ("BXW0094", CLIPPY_TEXT, TOOL_SOURCE);
const TESTS: Rule = ("BXW0095", TESTS_TEXT, TOOL_SOURCE);
const INVOKE: Rule = ("BXW0096", INVOKE_TEXT, TOOL_SOURCE);
const LOCK: Rule = ("BXW0097", LOCK_TEXT, LOCK_SOURCE);
const QUALITY: Rule = ("BXW0107", QUALITY_TEXT, QUALITY_SOURCE);

/// Injectable trusted command: program name plus argv after the program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandSpec {
    program: String,
    args: Vec<String>,
}

impl CommandSpec {
    /// Builds a specification from a program name and argument list.
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns the argument vector.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Renders the exact command line used in findings and tests.
    pub fn render(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Captured stdout and stderr from one trusted command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl CapturedOutput {
    /// Builds a captured result without spawning a process.
    pub fn new(success: bool, stdout: impl Into<Vec<u8>>, stderr: impl Into<Vec<u8>>) -> Self {
        Self {
            success,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    /// Reports whether the process exited successfully.
    pub fn success(&self) -> bool {
        self.success
    }

    /// Concatenates stdout then stderr for human rendering after findings.
    pub fn combined(&self) -> Vec<u8> {
        let mut bytes = self.stdout.clone();
        bytes.extend_from_slice(&self.stderr);
        bytes
    }
}

/// The trusted command executable could not be started.
#[derive(Debug, Eq, PartialEq)]
pub struct SpawnError;

impl fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} Cargo.toml: {}", INVOKE.0, INVOKE.1)
    }
}

impl std::error::Error for SpawnError {}

/// Outcome of one external check step: completion plus optional captured tool text.
#[derive(Debug, Eq, PartialEq)]
pub struct ToolStep {
    completion: Completion,
    output: Option<Vec<u8>>,
}

impl ToolStep {
    /// Splits into completion and owned captured output.
    pub fn into_parts(self) -> (Completion, Option<Vec<u8>>) {
        (self.completion, self.output)
    }
}

/// Runs `spec` at `root` through the host process API.
pub fn run_command(root: &Path, spec: &CommandSpec) -> Result<CapturedOutput, SpawnError> {
    let output = Command::new(&spec.program)
        .args(&spec.args)
        .current_dir(root)
        .output()
        .map_err(|_| SpawnError)?;
    Ok(CapturedOutput::new(
        output.status.success(),
        output.stdout,
        output.stderr,
    ))
}

/// Injectable runner used by tool steps and tests.
pub type CommandRunner = dyn Fn(&Path, &CommandSpec) -> Result<CapturedOutput, SpawnError>;

/// Cargo package names selected for formatting: non-derived owned crate manifests only.
pub fn fmt_packages(workspace: &Workspace) -> Vec<String> {
    workspace
        .cargo_members()
        .iter()
        .filter(|member| {
            workspace
                .classifications()
                .iter()
                .find(|classified| classified.path() == member.manifest_path())
                .is_some_and(|classified| classified.derived_output().is_none())
        })
        .map(|member| member.cargo_package().to_owned())
        .collect()
}

/// Builds `cargo fmt --check -p …` over the manifest-derived hand-authored selection.
pub fn fmt_spec(workspace: &Workspace) -> Option<CommandSpec> {
    let packages = fmt_packages(workspace);
    if packages.is_empty() {
        return None;
    }
    // Surface lock forbids `vec!`; keep an explicit push loop instead.
    #[allow(clippy::vec_init_then_push)]
    let args = {
        let mut args = Vec::new();
        args.push("fmt".to_owned());
        args.push("--check".to_owned());
        for package in packages {
            args.push("-p".to_owned());
            args.push(package);
        }
        args
    };
    Some(CommandSpec::new("cargo", args))
}

/// Builds denied-warning workspace Clippy matching the 08 baseline.
pub fn clippy_spec() -> CommandSpec {
    CommandSpec::new(
        "cargo",
        [
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
    )
}

/// Builds workspace tests matching the 08 baseline.
pub fn test_spec() -> CommandSpec {
    CommandSpec::new("cargo", ["test", "--workspace", "--all-features"])
}

/// Builds full locked Cargo metadata matching the 08 baseline step-4 freshness check.
pub fn lock_spec() -> CommandSpec {
    CommandSpec::new("cargo", ["metadata", "--format-version", "1", "--locked"])
}

/// Runs formatting, or returns passed when the manifest-derived selection is empty.
pub fn run_fmt_step(
    runner: &CommandRunner,
    root: &Path,
    workspace: &Workspace,
) -> Result<ToolStep, SpawnError> {
    match fmt_spec(workspace) {
        Some(spec) => run_tool_step(runner, root, &spec, FMT, "Cargo.toml"),
        None => Ok(ToolStep {
            completion: Completion::Passed,
            output: None,
        }),
    }
}

/// Runs Clippy through the injectable runner.
pub fn run_clippy_step(runner: &CommandRunner, root: &Path) -> Result<ToolStep, SpawnError> {
    run_tool_step(runner, root, &clippy_spec(), CLIPPY, "Cargo.toml")
}

/// Runs workspace tests through the injectable runner.
pub fn run_test_step(runner: &CommandRunner, root: &Path) -> Result<ToolStep, SpawnError> {
    run_tool_step(runner, root, &test_spec(), TESTS, "Cargo.toml")
}

/// Runs full locked Cargo resolution through the injectable runner.
pub fn run_lock_step(runner: &CommandRunner, root: &Path) -> Result<ToolStep, SpawnError> {
    run_tool_step(runner, root, &lock_spec(), LOCK, "Cargo.lock")
}

/// One declared quality command bound to its package and manifest path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualityCommand {
    package: BoxId,
    manifest_path: RelativePath,
    spec: CommandSpec,
}

impl QualityCommand {
    /// Returns the package that declared the command.
    pub fn package(&self) -> &BoxId {
        &self.package
    }

    /// Returns the package manifest path used for findings.
    pub fn manifest_path(&self) -> &RelativePath {
        &self.manifest_path
    }

    /// Returns the split command specification.
    pub fn spec(&self) -> &CommandSpec {
        &self.spec
    }
}

/// Flattens declared quality commands in package-id then declaration order.
pub fn quality_specs(workspace: &Workspace) -> Vec<QualityCommand> {
    // Surface lock forbids `vec!`; keep an explicit push loop instead.
    #[allow(clippy::vec_init_then_push)]
    {
        let mut commands = Vec::new();
        for package in workspace.packages() {
            for command in package.manifest().quality_commands() {
                let mut tokens = command.split_ascii_whitespace();
                let program = tokens
                    .next()
                    .expect("manifest validation guarantees a nonblank quality command");
                #[allow(clippy::vec_init_then_push)]
                let args = {
                    let mut args = Vec::new();
                    for token in tokens {
                        args.push(token.to_owned());
                    }
                    args
                };
                commands.push(QualityCommand {
                    package: package.id().clone(),
                    manifest_path: package.manifest_path().clone(),
                    spec: CommandSpec::new(program, args),
                });
            }
        }
        commands
    }
}

/// Runs every declared quality command, continuing after nonzero exits.
pub fn run_quality_step(
    runner: &CommandRunner,
    root: &Path,
    workspace: &Workspace,
) -> Result<ToolStep, SpawnError> {
    let commands = quality_specs(workspace);
    if commands.is_empty() {
        return Ok(ToolStep {
            completion: Completion::Passed,
            output: None,
        });
    }
    #[allow(clippy::vec_init_then_push)]
    let mut entries = Vec::new();
    let mut output: Option<Vec<u8>> = None;
    for command in &commands {
        let captured = runner(root, command.spec())?;
        if captured.success() {
            continue;
        }
        let finding = Finding::external(
            QUALITY.0,
            QUALITY.1,
            QUALITY.2,
            command.manifest_path().clone(),
            Some(command.package().clone()),
            format!("command=\"{}\"", command.spec().render()),
        );
        entries.push(Entry::Workspace(finding));
        let buffer = output.get_or_insert_with(Vec::new);
        if !buffer.is_empty() && buffer.last() != Some(&b'\n') {
            buffer.push(b'\n');
        }
        buffer.extend_from_slice(format!("command=\"{}\"\n", command.spec().render()).as_bytes());
        buffer.extend_from_slice(&captured.combined());
    }
    match Findings::new(entries) {
        None => Ok(ToolStep {
            completion: Completion::Passed,
            output: None,
        }),
        Some(findings) => Ok(ToolStep {
            completion: Completion::Failed(findings),
            output,
        }),
    }
}

fn run_tool_step(
    runner: &CommandRunner,
    root: &Path,
    spec: &CommandSpec,
    rule: Rule,
    finding_path: &str,
) -> Result<ToolStep, SpawnError> {
    let captured = runner(root, spec)?;
    if captured.success() {
        return Ok(ToolStep {
            completion: Completion::Passed,
            output: None,
        });
    }
    let finding = Finding::external(
        rule.0,
        rule.1,
        rule.2,
        RelativePath::new(finding_path).expect("tool finding path is a valid relative path"),
        None,
        format!("command=\"{}\"", spec.render()),
    );
    // Surface lock forbids `vec!`; clippy's vec_init_then_push is accepted for that reason.
    #[allow(clippy::vec_init_then_push)]
    let findings = {
        let mut entries = Vec::new();
        entries.push(Entry::Workspace(finding));
        Findings::new(entries).expect("one finding is nonempty")
    };
    Ok(ToolStep {
        completion: Completion::Failed(findings),
        output: Some(captured.combined()),
    })
}
