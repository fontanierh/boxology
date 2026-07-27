//! Effectful filesystem inputs for the future `boxology` command.
//!
//! This first CLI slice stops at the boundary between the filesystem and the pure
//! [`boxology_workspace`] checker: it walks a workspace and returns its raw files and manifest
//! bytes. Argument parsing, Cargo metadata, generation, and command execution belong to later
//! slices.
#![deny(missing_docs)]
#![forbid(unsafe_code)]
mod walk;
pub use walk::{WalkError, WalkedWorkspace, walk};
