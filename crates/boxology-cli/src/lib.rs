//! Effectful filesystem inputs for the `boxology` command.
//!
//! The CLI's effectful boundary walks a workspace and executes one validated generation plan, while
//! pure planning selects and describes contract-generation candidates. Generation remains pure;
//! execution delegates its writes to the generator writer. Classification of checked-in versus
//! regenerated schema bytes is also pure, as is the check-step classification seam over supplied
//! base-revision and checked-in schema bytes and regeneration comparison over derived artifacts.
//! Trusted lock/fmt/Clippy/test steps use an injectable runner; tool output is outside determinism claims.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::{path::Path, process::Command};

/// The exact argv passed to the one Cargo metadata invocation owned by the CLI.
pub const CARGO_METADATA_ARGS: [&str; 5] =
    ["metadata", "--format-version", "1", "--locked", "--no-deps"];

/// Builds the captured Cargo metadata command for `root`.
pub fn cargo_metadata_command(root: &Path) -> Command {
    let mut command = Command::new("cargo");
    command.args(CARGO_METADATA_ARGS).current_dir(root);
    command
}

mod base;
mod check;
mod classify;
mod compare;
mod execute;
mod generate;
mod runner;
mod walk;
pub use base::{BaseError, BaseSchemasError, GitToolError, base_package_schemas};
pub use check::{
    CheckClassificationError, ClassifyStepError, DuplicatePackages, PackageSchemas, classify_step,
};
pub use classify::{ClassifyError, classify};
pub use compare::{
    CompareDifference, CompareStepError, DifferenceKind, compare_plans, compare_step,
    composition_step,
};
pub use execute::{ExecuteError, Outcome, execute};
pub use generate::{GenerationPlan, PlanError, ResolvedImport, plan};
pub use runner::{
    CapturedOutput, CommandRunner, CommandSpec, SpawnError, ToolStep, clippy_spec, fmt_packages,
    fmt_spec, lock_spec, run_clippy_step, run_command, run_fmt_step, run_lock_step, run_test_step,
    test_spec,
};
pub use walk::{WalkError, WalkedWorkspace, walk};
