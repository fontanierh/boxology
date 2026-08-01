//! Effectful filesystem inputs for the future `boxology` command.
//!
//! The CLI's effectful boundary walks a workspace and executes one validated generation plan, while
//! pure planning selects and describes contract-generation candidates. Generation remains pure;
//! execution delegates its writes to the generator writer. Classification of checked-in versus
//! regenerated schema bytes is also pure, as is the check-step classification seam over supplied
//! base-revision and checked-in schema bytes.
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

mod check;
mod classify;
mod execute;
mod generate;
mod walk;
pub use check::{
    CheckClassificationError, ClassifyStepError, DuplicatePackages, PackageSchemas, classify_step,
};
pub use classify::{ClassifyError, classify};
pub use execute::{ExecuteError, Outcome, execute};
pub use generate::{GenerationPlan, PlanError, plan};
pub use walk::{WalkError, WalkedWorkspace, walk};
