//! Transport-neutral contract values shared by generated Boxology surfaces.
//!
//! Values have a private representation so construction always preserves the
//! semantic invariants that bindings and generated code rely on.

#[cfg(test)]
mod ac9_demo;
#[cfg_attr(not(test), allow(dead_code))]
mod conform;
mod identity;
mod opaque;
mod presence;
mod typed;
mod value;

pub use identity::{BoxId, CapabilityId, CapabilityName, ContractRevision, IdentityError};
pub use opaque::{OpaqueNumber, OpaqueNumberError, OpaquePayload, OpaqueTree};
pub use presence::{DecodeRole, Field};
pub use typed::{
    Blob, ContractError, ContractType, DecodeError, DecodeErrorKind, EncodeError, EncodeErrorKind,
    PathSegment, Secret,
};
pub use value::{ContractValue, ObjectRef, SlotValue, ValueError, ValueRef};
