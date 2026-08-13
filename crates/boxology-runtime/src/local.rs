//! In-process composition binding for generated typed handles.

use crate::{TransportBinding, TransportHandle, TransportJoinFuture, TransportRuntime};
use boxology_contract::{
    CallContext, CapabilityDescriptor, CapabilityId, CapabilityShape, Detail, ErasedCallError,
    ErasedCallTarget, ExposureLevel, SlotValue,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
};

/// A production in-process binding that can be passed directly to a generated handle.
#[derive(Default)]
pub struct LocalBinding {
    runtime: Mutex<Option<Weak<TransportRuntime<()>>>>,
}

impl LocalBinding {
    /// Constructs an unstarted local binding.
    pub fn new() -> Self {
        Self::default()
    }
}

/// Keeps an activated local binding alive for the composition lifetime.
#[doc(hidden)]
pub struct LocalHandle(Arc<TransportRuntime<()>>);

impl TransportHandle for LocalHandle {
    fn stop_intake(&self) {}
    fn cancel_tasks(&self) {}
    fn abort_tasks(&self) {}
    fn join_tasks(self: Box<Self>) -> TransportJoinFuture {
        drop(self.0);
        Box::pin(std::future::ready(Ok(())))
    }
}

impl TransportBinding for LocalBinding {
    type Config = ();
    type Handle = LocalHandle;

    fn config(&self) -> Arc<()> {
        Arc::new(())
    }

    fn conform(
        &self,
        descriptor: &CapabilityDescriptor,
        _level: ExposureLevel,
    ) -> Result<(), Detail> {
        matches!(descriptor.shape(), CapabilityShape::Unary)
            .then_some(())
            .ok_or_else(|| Detail::new("unsupported_interaction_shape"))
    }

    fn prepare(&self, _descriptors: &[&'static CapabilityDescriptor]) -> Result<(), Detail> {
        Ok(())
    }

    fn start(&self, runtime: TransportRuntime<()>) -> Result<LocalHandle, Detail> {
        let runtime = Arc::new(runtime);
        let mut retained = self.runtime.lock().map_err(|_| Detail::new("local_lock"))?;
        if retained.is_some() {
            return Err(Detail::new("local_binding_already_started"));
        }
        *retained = Some(Arc::downgrade(&runtime));
        Ok(LocalHandle(runtime))
    }
}

impl ErasedCallTarget for LocalBinding {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        let exposure = self
            .runtime
            .lock()
            .ok()
            .and_then(|runtime| runtime.as_ref()?.upgrade())
            .and_then(|runtime| {
                runtime
                    .exposures()
                    .iter()
                    .find(|exposure| exposure.descriptor().id() == capability)
                    .cloned()
            });
        Box::pin(async move {
            match exposure {
                Some(exposure) => exposure.dispatch(context, input).await,
                None => Err(ErasedCallError::Internal(Detail::new("local_capability"))),
            }
        })
    }
}
