//! Compatibility facade for the effectful Boxology command core.
//!
//! The installed `boxology` binary remains in this package. Reusable command behavior lives in
//! `boxology-cli-core` and is re-exported here so existing library consumers keep the same seam.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub use boxology_cli_core::*;
