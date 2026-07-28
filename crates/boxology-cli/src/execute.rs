//! Materialize one validated generation plan through the pure generator and atomic writer.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use crate::GenerationPlan;
use boxology_generator::OUTPUTS;
use boxology_generator_model::Diagnostics;
use boxology_generator_model::GenerationRequest;
use boxology_generator_writer::WriteError;
use boxology_manifest::RelativePath;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

type Rule = (&'static str, &'static str, &'static str);
const SOURCE: &str = "specs/s5-manifest-and-validation.md D5";
const INFERRED_SOURCE: &str = "specs/s5-manifest-and-validation.md D5 (inferred)";
const INPUT_TEXT: &str = "a generation input must be a readable regular file";
const GENERATOR_TEXT: &str = "the contract generator returned diagnostics";
const WRITER_TEXT: &str = "the generated tree could not be written";
const COVERAGE_TEXT: &str = "a generator output is not covered by a declared output pattern";
const INPUT: Rule = ("BXW0070", INPUT_TEXT, SOURCE);
const GENERATOR: Rule = ("BXW0071", GENERATOR_TEXT, SOURCE);
const WRITER: Rule = ("BXW0072", WRITER_TEXT, SOURCE);
// This is an inference: D5 supplies the fixed generator output set and the plan supplies the
// manifest output patterns, but does not state the implication that every fixed output is covered.
const COVERAGE: Rule = ("BXW0073", COVERAGE_TEXT, INFERRED_SOURCE);

/// The files changed by one accepted execution, in deterministic logical-path order.
#[derive(Debug, Eq, PartialEq)]
pub struct Outcome {
    written: Vec<String>,
    removed: Vec<String>,
}

impl Outcome {
    /// Returns generated logical paths created or replaced by this execution.
    pub fn written(&self) -> &[String] {
        &self.written
    }

    /// Returns stale declared logical paths removed by this execution.
    pub fn removed(&self) -> &[String] {
        &self.removed
    }

    /// Reports whether no generated file was written and no stale file was removed.
    pub fn is_unchanged(&self) -> bool {
        self.written.is_empty() && self.removed.is_empty()
    }
}

/// A stable execution failure with an optional verbatim generator or writer cause.
#[derive(Debug)]
pub struct ExecuteError {
    code: &'static str,
    location: PathBuf,
    detail: &'static str,
    cause: Cause,
}

#[derive(Debug)]
enum Cause {
    Input,
    Generator(Diagnostics),
    Writer(WriteError),
    Coverage,
}

impl ExecuteError {
    /// Returns the stable `BXW####` code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the filesystem location associated with the failure.
    pub fn location(&self) -> &Path {
        &self.location
    }

    /// Returns the filesystem location; this alias matches the earlier CLI error surfaces.
    pub fn path(&self) -> &Path {
        self.location()
    }

    /// Returns the stable static rule detail.
    pub fn detail(&self) -> &'static str {
        self.detail
    }

    /// Returns the generator diagnostics without changing their order or rendering.
    pub fn diagnostics(&self) -> Option<&Diagnostics> {
        match &self.cause {
            Cause::Generator(diagnostics) => Some(diagnostics),
            Cause::Input | Cause::Writer(_) | Cause::Coverage => None,
        }
    }

    /// Returns the writer error without changing its payload or rendering.
    pub fn write_error(&self) -> Option<&WriteError> {
        match &self.cause {
            Cause::Writer(error) => Some(error),
            Cause::Input | Cause::Generator(_) | Cause::Coverage => None,
        }
    }

    fn input(path: PathBuf) -> Self {
        Self {
            code: INPUT.0,
            location: path,
            detail: INPUT.1,
            cause: Cause::Input,
        }
    }

    fn generator(path: PathBuf, diagnostics: Diagnostics) -> Self {
        Self {
            code: GENERATOR.0,
            location: path,
            detail: GENERATOR.1,
            cause: Cause::Generator(diagnostics),
        }
    }

    fn writer(path: PathBuf, error: WriteError) -> Self {
        Self {
            code: WRITER.0,
            location: path,
            detail: WRITER.1,
            cause: Cause::Writer(error),
        }
    }

    fn coverage(path: PathBuf) -> Self {
        Self {
            code: COVERAGE.0,
            location: path,
            detail: COVERAGE.1,
            cause: Cause::Coverage,
        }
    }
}

impl fmt::Display for ExecuteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {:?}: {}",
            self.code, self.location, self.detail
        )?;
        match &self.cause {
            Cause::Generator(diagnostics) => write!(formatter, ": {diagnostics}"),
            Cause::Writer(error) => write!(formatter, ": {error}"),
            Cause::Input | Cause::Coverage => Ok(()),
        }
    }
}

impl std::error::Error for ExecuteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            Cause::Writer(error) => Some(error),
            Cause::Input | Cause::Generator(_) | Cause::Coverage => None,
        }
    }
}

/// Executes one plan against `root`, reading package inputs, generating its tree, and writing it.
///
/// Inputs are read in sorted logical-path order after refusing symlinks in every input ancestor.
/// The package directory is `root` joined with the plan's validated package root.
///
/// # Errors
/// Returns `BXW0070` for a missing, unreadable, or non-regular input; `BXW0071` with generator
/// diagnostics; `BXW0072` with the writer error; or `BXW0073` when a fixed generator output is not
/// covered by the plan's declared patterns.
pub fn execute(root: &Path, plan: &GenerationPlan) -> Result<Outcome, ExecuteError> {
    let package_root = plan.package_root().map_or("", RelativePath::as_str);
    let package_dir = guarded(root, package_root, false)?;
    let mut input_paths = plan.inputs().to_vec();
    input_paths.sort_unstable();
    let guarded_inputs = input_paths
        .into_iter()
        .map(|input| guarded(&package_dir, input.as_str(), true).map(|path| (input, path)))
        .collect::<Result<Vec<_>, ExecuteError>>()?;
    let inputs = guarded_inputs
        .into_iter()
        .map(|(input, path)| {
            let bytes = fs::read(&path).map_err(|_| ExecuteError::input(path))?;
            Ok((input.as_str().to_owned(), bytes))
        })
        .collect::<Result<Vec<_>, ExecuteError>>()?;
    let request = GenerationRequest::new(
        plan.package_id().clone(),
        plan.crate_root().as_str().to_owned(),
        inputs,
        Vec::new(),
        OUTPUTS.iter().map(|path| (*path).to_owned()).collect(),
    )
    .map_err(|diagnostics| {
        ExecuteError::generator(package_dir.join(plan.crate_root().as_str()), diagnostics)
    })?;
    let tree = boxology_generator::generate(&request).map_err(|diagnostics| {
        ExecuteError::generator(package_dir.join(plan.crate_root().as_str()), diagnostics)
    })?;
    for output in OUTPUTS {
        let output =
            RelativePath::new(output.to_owned()).expect("generator outputs are valid paths");
        if !plan
            .outputs()
            .iter()
            .any(|pattern| pattern.matches(&output))
        {
            return Err(ExecuteError::coverage(package_dir.join(output.as_str())));
        }
    }
    guarded(root, package_root, false)?;
    let changes = boxology_generator_writer::write(&package_dir, &tree, plan.outputs())
        .map_err(|error| ExecuteError::writer(package_dir, error))?;
    let mut written = changes.written;
    let mut removed = changes.removed;
    written.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    removed.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    Ok(Outcome { written, removed })
}

fn guarded(root: &Path, relative: &str, file: bool) -> Result<PathBuf, ExecuteError> {
    let mut path = root.to_owned();
    let mut parts = relative
        .split('/')
        .filter(|part| !part.is_empty())
        .peekable();
    loop {
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| ExecuteError::input(path.clone()))?;
        let accepted = if parts.peek().is_none() && file {
            metadata.is_file()
        } else {
            metadata.is_dir()
        };
        if metadata.file_type().is_symlink() || !accepted {
            return Err(ExecuteError::input(path));
        }
        let Some(part) = parts.next() else {
            return Ok(path);
        };
        path.push(part);
    }
}
