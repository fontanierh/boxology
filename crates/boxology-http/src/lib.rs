#![forbid(unsafe_code)]

#[cfg(feature = "client")]
mod client;
#[cfg(any(feature = "client", feature = "server"))]
#[cfg_attr(not(feature = "client"), allow(dead_code))]
mod conformance;
#[allow(dead_code)]
mod encoder;
#[cfg(test)]
mod replay_tests;
#[allow(dead_code)]
mod semantic;
#[allow(dead_code)]
#[cfg(feature = "server")]
mod server;
#[allow(dead_code)]
mod syntax;

#[cfg(feature = "client")]
pub use client::{HttpClientConfig, HttpClientTarget};
