//! Transport-neutral contract values shared by generated Boxology surfaces.
//!
//! Values have a private representation so construction always preserves the
//! semantic invariants that bindings and generated code rely on.

#[cfg_attr(not(test), allow(dead_code))]
mod conform;
mod opaque;
mod presence;
mod value;

pub use opaque::{OpaqueNumber, OpaqueNumberError, OpaquePayload, OpaqueTree};
pub use presence::{DecodeRole, Field};
pub use value::{ContractValue, ObjectRef, SlotValue, ValueError, ValueRef};
