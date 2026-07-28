//! Effectful filesystem inputs for the future `boxology` command.
//!
//! The CLI's effectful boundary walks a workspace, while pure planning selects and describes
//! contract-generation candidates. Byte reads, Cargo metadata, generation, and writes belong to
//! later slices.
#![deny(missing_docs)]
#![forbid(unsafe_code)]
mod generate;
mod walk;
pub use generate::{GenerationPlan, PlanError, plan};
pub use walk::{WalkError, WalkedWorkspace, walk};
