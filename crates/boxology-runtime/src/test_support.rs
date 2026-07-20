//! Minimal transport support for runtime-level composition tests.

use std::sync::{Arc, Mutex, Weak};

use crate::{TransportBinding, TransportHandle, TransportRuntime};
use boxology_contract::{CapabilityDescriptor, CapabilityShape, Detail, ExposureLevel};

/// A unary-only transport binding that retains no wire or task behavior.
pub struct StubTransport {
    runtime: Mutex<Option<Weak<TransportRuntime<()>>>>,
}

impl StubTransport {
    /// Constructs an unstarted stub transport allocation.
    pub fn new() -> Self {
        Self {
            runtime: Mutex::new(None),
        }
    }
    /// Returns the runtime while its composition-owned handle remains live.
    pub fn runtime(&self) -> Option<Arc<TransportRuntime<()>>> {
        self.runtime
            .lock()
            .expect("stub runtime lock poisoned")
            .as_ref()
            .and_then(Weak::upgrade)
    }
}

impl Default for StubTransport {
    fn default() -> Self {
        Self::new()
    }
}
/// The composition-owned strong runtime handle for a [`StubTransport`].
pub struct StubTransportHandle {
    _runtime: Arc<TransportRuntime<()>>,
}
impl TransportHandle for StubTransportHandle {
    fn stop_intake(&self) {}
    fn cancel_tasks(&self) {}
    fn abort_tasks(&self) {}
}
impl TransportBinding for StubTransport {
    type Config = ();
    type Handle = StubTransportHandle;
    fn config(&self) -> Arc<()> {
        Arc::new(())
    }
    fn conform(
        &self,
        descriptor: &CapabilityDescriptor,
        _level: ExposureLevel,
    ) -> Result<(), Detail> {
        match descriptor.shape() {
            CapabilityShape::Unary => Ok(()),
            _ => Err(Detail::new("unsupported_interaction_shape")
                .with_message("stub transport supports unary capabilities only")),
        }
    }
    fn prepare(&self, _descriptors: &[&'static CapabilityDescriptor]) -> Result<(), Detail> {
        Ok(())
    }
    fn start(&self, runtime: TransportRuntime<()>) -> Result<StubTransportHandle, Detail> {
        let mut probe = self.runtime.lock().expect("stub runtime lock poisoned");
        if probe.is_some() {
            return Err(Detail::new("stub_already_started"));
        }
        let runtime = Arc::new(runtime);
        *probe = Some(Arc::downgrade(&runtime));
        Ok(StubTransportHandle { _runtime: runtime })
    }
}
