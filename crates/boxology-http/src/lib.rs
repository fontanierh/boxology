#![forbid(unsafe_code)]

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
