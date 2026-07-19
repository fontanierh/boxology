//! Transport-neutral contract values shared by generated Boxology surfaces.
//!
//! Values have a private representation so construction always preserves the
//! semantic invariants that bindings and generated code rely on.

mod value;

pub use value::{ContractValue, ObjectRef, SlotValue, ValueError, ValueRef};
