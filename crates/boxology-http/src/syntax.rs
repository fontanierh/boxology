use boxology_contract::{OpaqueTree, json};

pub(crate) use json::SyntaxError;
pub(crate) const DEFAULT_DEPTH_LIMIT: usize = json::DEFAULT_DEPTH_LIMIT;

#[derive(Clone, Copy)]
pub(crate) struct SyntaxLimits(pub(crate) usize, pub(crate) usize);

pub(crate) fn parse(input: &[u8], limits: SyntaxLimits) -> Result<OpaqueTree, SyntaxError> {
    json::parse(input, json::Limits::new(limits.0, limits.1))
}
