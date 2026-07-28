//! Effectful filesystem inputs for the future `boxology` command.
//!
//! The CLI's effectful boundary walks a workspace and executes one validated generation plan, while
//! pure planning selects and describes contract-generation candidates. Generation remains pure;
//! execution delegates its writes to the generator writer.
#![deny(missing_docs)]
#![forbid(unsafe_code)]
mod execute;
mod generate;
mod walk;
pub use execute::{ExecuteError, Outcome, execute};
pub use generate::{GenerationPlan, PlanError, plan};
pub use walk::{WalkError, WalkedWorkspace, walk};
