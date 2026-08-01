//! Compare checked-in derived artifacts with an in-memory regeneration.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use crate::{
    PlanError,
    execute::{ExecuteError, generate_tree, guarded},
    plan,
};
use boxology_contract::BoxId;
use boxology_generator::GeneratedTree;
use boxology_manifest::RelativePath;
use boxology_workspace::Workspace;
use std::{fmt, fs, io, path::Path};

type Rule = (&'static str, &'static str, &'static str);
const COMPARE_SOURCE: &str = "specs/s5-manifest-and-validation.md D6; boxology-details/08-rust-build-topology.md workspace operations and validation baseline step 2";
const COMPARE_TEXT: &str = "a checked-in derived artifact must be byte-identical to regeneration; regenerate the accountable package with boxology generate --package <id>";
const REGENERATION: Rule = ("BXW0083", COMPARE_TEXT, COMPARE_SOURCE);

/// Why one checked-in derived artifact does not match regeneration.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DifferenceKind {
    /// No checked-in file exists at a regenerated path.
    Missing,
    /// Checked-in bytes differ, or the checked-in path cannot be read.
    Differing,
    /// A classified derived-output file has no regenerated counterpart.
    Stale,
}
impl DifferenceKind {
    /// Returns the stable human and report-payload identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Differing => "differing",
            Self::Stale => "stale",
        }
    }
}

/// One package-relative difference between checked-in and regenerated derived output.
#[derive(Debug, Eq, PartialEq)]
pub struct CompareDifference {
    package: BoxId,
    path: RelativePath,
    kind: DifferenceKind,
}

impl CompareDifference {
    /// Returns the accountable package identity.
    pub fn package(&self) -> &BoxId {
        &self.package
    }

    /// Returns the package-relative differing path.
    pub fn path(&self) -> &RelativePath {
        &self.path
    }

    /// Returns why the path differs from regeneration.
    pub fn kind(&self) -> DifferenceKind {
        self.kind
    }

    /// Returns the stable compare code.
    pub fn code(&self) -> &'static str {
        REGENERATION.0
    }

    /// Returns the stable rule detail.
    pub fn detail(&self) -> &'static str {
        REGENERATION.1
    }

    /// Returns the exact command that repairs this package's generated output.
    pub fn repair_command(&self) -> String {
        format!("boxology generate --package {}", self.package.as_str())
    }

    /// Returns the normative source of the byte-identity comparison rule.
    pub fn rule_source(&self) -> &'static str {
        COMPARE_SOURCE
    }
}

/// A planning or generation failure while comparing derived output.
#[derive(Debug)]
pub enum CompareStepError {
    /// The workspace could not produce a generation plan.
    Plan(PlanError),
    /// The generation inputs or generator could not produce an in-memory tree.
    Execute(ExecuteError),
}

impl From<PlanError> for CompareStepError {
    fn from(error: PlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<ExecuteError> for CompareStepError {
    fn from(error: ExecuteError) -> Self {
        Self::Execute(error)
    }
}

impl fmt::Display for CompareStepError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => error.fmt(formatter),
            Self::Execute(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CompareStepError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Execute(error) => Some(error),
        }
    }
}

/// Compares every contract-generation plan with its checked-in derived files.
///
/// Regeneration is kept in memory. A checked-in path is read through the same ancestor guard used
/// by execution; an existing path that cannot be read fails closed as [`DifferenceKind::Differing`].
///
/// # Errors
/// Returns [`CompareStepError::Plan`] for planning failures or [`CompareStepError::Execute`] for
/// generation failures. Filesystem read failures for classified artifacts are differences rather
/// than ingestion errors.
pub fn compare_step(
    root: &Path,
    workspace: &Workspace,
) -> Result<Vec<CompareDifference>, CompareStepError> {
    let plans = plan(workspace, None)?;
    let mut differences = Vec::new();
    for plan in plans {
        let package_root = plan.package_root().map_or("", RelativePath::as_str);
        let package_dir = guarded(root, package_root, false)?;
        guarded(&package_dir, plan.crate_root().as_str(), true)?;
        let (package_dir, tree) = generate_tree(root, &plan)?;
        let package = workspace
            .packages()
            .iter()
            .find(|package| package.id() == plan.package_id())
            .expect("every generation plan belongs to a workspace package");
        let generated = generated_paths(&tree);
        for (path, file) in generated.iter().zip(tree.files()) {
            match checked_in(&package_dir, path) {
                CheckedIn::Missing => differences.push(difference(
                    plan.package_id(),
                    path.clone(),
                    DifferenceKind::Missing,
                )),
                CheckedIn::Unreadable => differences.push(difference(
                    plan.package_id(),
                    path.clone(),
                    DifferenceKind::Differing,
                )),
                CheckedIn::Bytes(bytes) if bytes != file.bytes() => differences.push(difference(
                    plan.package_id(),
                    path.clone(),
                    DifferenceKind::Differing,
                )),
                CheckedIn::Bytes(_) => {}
            }
        }
        for classification in workspace.classifications().iter().filter(|classification| {
            classification.package() == plan.package_id()
                && classification.derived_output() == Some(plan.derived_output_id())
        }) {
            let Some(path) = package.relative(classification.path()) else {
                continue;
            };
            if !generated.contains(&path) {
                differences.push(difference(plan.package_id(), path, DifferenceKind::Stale));
            }
        }
    }
    differences.sort_by(|left, right| {
        left.package
            .cmp(&right.package)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(differences)
}

fn generated_paths(tree: &GeneratedTree) -> Vec<RelativePath> {
    tree.files()
        .iter()
        .map(|file| {
            RelativePath::new(file.path().to_owned())
                .expect("generator outputs are valid relative paths")
        })
        .collect()
}

fn difference(package: &BoxId, path: RelativePath, kind: DifferenceKind) -> CompareDifference {
    CompareDifference {
        package: package.clone(),
        path,
        kind,
    }
}

enum CheckedIn {
    Missing,
    Unreadable,
    Bytes(Vec<u8>),
}

fn checked_in(package_dir: &Path, path: &RelativePath) -> CheckedIn {
    let location = package_dir.join(path.as_str());
    match fs::symlink_metadata(&location) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => CheckedIn::Missing,
        Err(_) => CheckedIn::Unreadable,
        Ok(_) => match guarded(package_dir, path.as_str(), true)
            .ok()
            .and_then(|path| fs::read(path).ok())
        {
            Some(bytes) => CheckedIn::Bytes(bytes),
            None => CheckedIn::Unreadable,
        },
    }
}
