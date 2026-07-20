//! Transport-neutral contract values shared by generated Boxology surfaces.
//!
//! Values have a private representation so construction always preserves the
//! semantic invariants that bindings and generated code rely on.

#[cfg(test)]
mod ac9_demo;
mod conform;
mod context;
mod contract;
mod descriptor;
mod dispatch;
mod error;
mod identity;
mod implementation;
mod opaque;
mod presence;
mod typed;
mod value;

pub use conform::{ConformanceError, ConformanceErrorKind};
pub use context::{
    CallContext, Caller, CancelToken, Deadline, IdempotencyKey, IdempotencyKeyError, TraceContext,
};
pub use contract::{
    CapabilityDescriptor, CapabilityShape, ContractDescriptor, ContractDescriptorError,
    ExposureLevel, Idempotency,
};
pub use descriptor::{
    Deprecation, DescriptorError, DescriptorRef, FieldDescriptor, TypeDescriptor,
    VariantDescriptor, VariantPayload,
};
pub use dispatch::ErasedTarget;
pub use error::{CallError, Detail, ErasedCallError};
pub use identity::{BoxId, CapabilityId, CapabilityName, ContractRevision, IdentityError};
pub use implementation::{
    ImplementationDescriptor, ImplementationDescriptorError, ImportDescriptor,
    ImportDescriptorError,
};
pub use opaque::{OpaqueNumber, OpaqueNumberError, OpaquePayload, OpaqueTree};
pub use presence::{DecodeRole, Field};
pub use typed::{
    Blob, ContractError, ContractType, DecodeError, DecodeErrorKind, EncodeError, EncodeErrorKind,
    PathSegment, Secret,
};
pub use value::{ContractValue, ObjectRef, SlotValue, ValueError, ValueRef};
