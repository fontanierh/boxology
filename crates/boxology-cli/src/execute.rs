//! Materialize one validated generation plan through the pure generator and atomic writer.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use crate::GenerationPlan;
use boxology_generator::{GeneratedTree, OUTPUTS};
use boxology_generator_model::Diagnostics;
use boxology_generator_model::GenerationRequest;
use boxology_generator_writer::WriteError;
use boxology_manifest::RelativePath;
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

type Rule = (&'static str, &'static str, &'static str);
const SOURCE: &str = "specs/s5-manifest-and-validation.md D5";
const INFERRED_SOURCE: &str = "specs/s5-manifest-and-validation.md D5 (inferred)";
const INPUT_TEXT: &str = "a generation input must be a readable regular file";
const GENERATOR_TEXT: &str = "the contract generator returned diagnostics";
const WRITER_TEXT: &str = "the generated tree could not be written";
const COVERAGE_TEXT: &str = "a generator output is not covered by a declared output pattern";
const SCHEMA_TEXT: &str = "the checked-in schema document must be a readable regular file";
const INPUT: Rule = ("BXW0070", INPUT_TEXT, SOURCE);
const GENERATOR: Rule = ("BXW0071", GENERATOR_TEXT, SOURCE);
const WRITER: Rule = ("BXW0072", WRITER_TEXT, SOURCE);
// This is an inference: D5 supplies the fixed generator output set and the plan supplies the
// manifest output patterns, but does not state the implication that every fixed output is covered.
const COVERAGE: Rule = ("BXW0073", COVERAGE_TEXT, INFERRED_SOURCE);
// This is an inference: D5 requires reading the checked-in schema as classification base, and the
// CLI's ingestion refuses symlinks and non-regular files with the same strength as BXW0070.
const SCHEMA_FILE: Rule = ("BXW0076", SCHEMA_TEXT, INFERRED_SOURCE);

/// The files changed by one accepted execution, in deterministic logical-path order.
#[derive(Debug, Eq, PartialEq)]
pub struct Outcome {
    written: Vec<String>,
    removed: Vec<String>,
    base_schema: Option<Vec<u8>>,
    submitted_schema: Vec<u8>,
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

    /// Returns the pre-write checked-in `generated/schema.json` bytes, when present.
    pub fn base_schema(&self) -> Option<&[u8]> {
        self.base_schema.as_deref()
    }

    /// Returns the regenerated `generated/schema.json` bytes from the in-memory tree.
    pub fn submitted_schema(&self) -> &[u8] {
        &self.submitted_schema
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
    Schema,
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
            Cause::Input | Cause::Writer(_) | Cause::Coverage | Cause::Schema => None,
        }
    }

    /// Returns the writer error without changing its payload or rendering.
    pub fn write_error(&self) -> Option<&WriteError> {
        match &self.cause {
            Cause::Writer(error) => Some(error),
            Cause::Input | Cause::Generator(_) | Cause::Coverage | Cause::Schema => None,
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

    fn schema(path: PathBuf) -> Self {
        Self {
            code: SCHEMA_FILE.0,
            location: path,
            detail: SCHEMA_FILE.1,
            cause: Cause::Schema,
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
            Cause::Input | Cause::Coverage | Cause::Schema => Ok(()),
        }
    }
}

impl std::error::Error for ExecuteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            Cause::Writer(error) => Some(error),
            Cause::Input | Cause::Generator(_) | Cause::Coverage | Cause::Schema => None,
        }
    }
}

/// Lazy sequential multi-plan execution seam used by `boxology generate`.
///
/// Each plan is fully executed — live imports are read, the tree is generated, and declared
/// outputs are written — before the next plan begins. Callers that need import-before-importer
/// convergence must supply plans in that dependency order.
///
/// The iterator is terminal after its first execution error: once an `Err` is yielded, every later
/// `next` call returns `None` and no later plan is executed or written.
#[derive(Debug)]
pub struct ExecutePlans<'a, I> {
    root: &'a Path,
    plans: I,
    terminal: bool,
}

impl<'a, I> Iterator for ExecutePlans<'a, I>
where
    I: Iterator<Item = &'a GenerationPlan>,
{
    type Item = Result<(&'a GenerationPlan, Outcome), ExecuteError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.terminal {
            return None;
        }
        let plan = self.plans.next()?;
        match execute(self.root, plan) {
            Ok(outcome) => Some(Ok((plan, outcome))),
            Err(error) => {
                self.terminal = true;
                Some(Err(error))
            }
        }
    }
}

/// Returns a lazy executor that runs `plans` in iterator order against `root`.
///
/// This is the canonical multi-plan sequential seam: each plan observes import bytes left on disk
/// by earlier writes in the same pass. Production `boxology generate` and one-pass convergence
/// proofs both drive this iterator so generate-all-before-write refactors cannot slip past them.
/// After the first execution error the iterator is terminal and yields `None` forever, so later
/// plans are neither executed nor written.
pub fn execute_plans<'a, I>(root: &'a Path, plans: I) -> ExecutePlans<'a, I::IntoIter>
where
    I: IntoIterator<Item = &'a GenerationPlan>,
{
    ExecutePlans {
        root,
        plans: plans.into_iter(),
        terminal: false,
    }
}

/// Executes one plan against `root`, reading package inputs, generating its tree, and writing it.
///
/// Inputs are read in sorted logical-path order after refusing symlinks in every input ancestor;
/// resolved imports follow in manifest declaration order. The package directory is `root` joined
/// with the plan's validated package root.
/// Pre-write capture records the checked-in schema bytes beside the regenerated tree bytes.
///
/// # Errors
/// Returns `BXW0070` for a missing, unreadable, or non-regular input; `BXW0071` with generator
/// diagnostics; `BXW0072` with the writer error; `BXW0073` when a fixed generator output is not
/// covered by the plan's declared patterns; or `BXW0076` when the checked-in schema is present but
/// not a readable regular file.
pub fn execute(root: &Path, plan: &GenerationPlan) -> Result<Outcome, ExecuteError> {
    let package_root = plan.package_root().map_or("", RelativePath::as_str);
    let (package_dir, tree) = generate_tree(root, plan)?;
    let submitted_schema = tree
        .files()
        .iter()
        .find(|file| file.path() == package_schema_path(plan))
        .expect("generator outputs include schema.json")
        .bytes()
        .to_vec();
    let base_schema = read_base_schema(root, plan)?;
    guarded(root, package_root, false)?;
    let changes = boxology_generator_writer::write(&package_dir, &tree, plan.outputs())
        .map_err(|error| ExecuteError::writer(package_dir, error))?;
    let mut written = changes.written;
    let mut removed = changes.removed;
    written.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    removed.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    Ok(Outcome {
        written,
        removed,
        base_schema,
        submitted_schema,
    })
}

pub(crate) fn generate_tree(
    root: &Path,
    plan: &GenerationPlan,
) -> Result<(PathBuf, GeneratedTree), ExecuteError> {
    let package_root = plan.package_root().map_or("", RelativePath::as_str);
    let package_dir = guarded(root, package_root, false)?;
    let mut input_paths = plan.inputs().to_vec();
    input_paths.sort_unstable();
    let guarded_inputs = input_paths
        .into_iter()
        .map(|input| guarded(&package_dir, input.as_str(), true).map(|path| (input, path)))
        .collect::<Result<Vec<_>, ExecuteError>>()?;
    let mut inputs = guarded_inputs
        .into_iter()
        .map(|(input, path)| {
            let bytes = fs::read(&path).map_err(|_| ExecuteError::input(path))?;
            Ok((input.as_str().to_owned(), bytes))
        })
        .collect::<Result<Vec<_>, ExecuteError>>()?;
    let raw_imports = plan
        .imports()
        .iter()
        .map(|import| {
            (
                import.package().clone(),
                import.schema().as_str().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    for import in plan.imports() {
        let schema = import.schema().clone();
        let path = guarded(root, schema.as_str(), true)?;
        let bytes = fs::read(&path).map_err(|_| ExecuteError::input(path))?;
        inputs.push((schema.as_str().to_owned(), bytes));
    }
    let request = GenerationRequest::new(
        plan.package_id().clone(),
        plan.crate_root().as_str().to_owned(),
        inputs,
        raw_imports,
        OUTPUTS.iter().map(|path| (*path).to_owned()).collect(),
    )
    .map_err(|diagnostics| {
        ExecuteError::generator(package_dir.join(plan.crate_root().as_str()), diagnostics)
    })?;
    let tree = boxology_generator::generate(request).map_err(|diagnostics| {
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
    Ok((package_dir, tree))
}

fn read_base_schema(root: &Path, plan: &GenerationPlan) -> Result<Option<Vec<u8>>, ExecuteError> {
    read_optional_file(root, plan.schema_path())
}

pub(crate) fn missing_schema(root: &Path, plan: &GenerationPlan) -> ExecuteError {
    ExecuteError::schema(root.join(plan.schema_path().as_str()))
}

pub(crate) fn read_optional_file(
    root: &Path,
    logical: &RelativePath,
) -> Result<Option<Vec<u8>>, ExecuteError> {
    let location = root.join(logical.as_str());
    match fs::symlink_metadata(&location) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) | Ok(_) => {
            let path = guarded(root, logical.as_str(), true)
                .map_err(|error| ExecuteError::schema(error.location().to_path_buf()))?;
            let bytes = fs::read(&path).map_err(|_| ExecuteError::schema(path))?;
            Ok(Some(bytes))
        }
    }
}

fn package_schema_path(plan: &GenerationPlan) -> &str {
    plan.package_root().map_or_else(
        || plan.schema_path().as_str(),
        |root| {
            plan.schema_path()
                .as_str()
                .strip_prefix(root.as_str())
                .and_then(|path| path.strip_prefix('/'))
                .expect("the plan's schema is inside its package root")
        },
    )
}

pub(crate) fn guarded(root: &Path, relative: &str, file: bool) -> Result<PathBuf, ExecuteError> {
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
