//! Transport binding lifecycle and composition-owned runtime carriers.
use boxology_contract::{
    CallContext, CapabilityDescriptor, Detail, ErasedCallError, ErasedTarget, ExposureLevel,
    SlotValue, call_guarded,
};
use std::{future::Future, pin::Pin, sync::Arc};
use tokio_util::sync::CancellationToken;
/// The composition-owned task tracker shared with every transport binding.
pub type TransportTaskTracker = tokio_util::task::TaskTracker;
/// A payload-safe completion future for every task owned by one transport.
pub type TransportJoinFuture = Pin<Box<dyn Future<Output = Result<(), Detail>> + Send + 'static>>;
/// One capability exposed through a transport binding.
#[derive(Clone)]
pub struct TransportExposure {
    descriptor: &'static CapabilityDescriptor,
    level: ExposureLevel,
    target: Arc<dyn ErasedTarget>,
}
#[allow(dead_code)]
impl TransportExposure {
    pub(crate) fn new(
        descriptor: &'static CapabilityDescriptor,
        level: ExposureLevel,
        target: Arc<dyn ErasedTarget>,
    ) -> Self {
        Self {
            descriptor,
            level,
            target,
        }
    }
    /// Returns the exact descriptor selected for this exposure.
    pub fn descriptor(&self) -> &'static CapabilityDescriptor {
        self.descriptor
    }
    /// Returns the boundary level selected for this exposure.
    pub fn level(&self) -> ExposureLevel {
        self.level
    }
    /// Delegates to the retained target through guarded dispatch without adding policy.
    pub fn dispatch<'a>(
        &'a self,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        call_guarded(self.target.as_ref(), self.descriptor.id(), context, input)
    }
}
/// Per-binding state whose activation gate commits startup, not call cancellation.
pub struct TransportRuntime<C>
where
    C: Send + Sync + 'static,
{
    exposures: Arc<[TransportExposure]>,
    tracker: TransportTaskTracker,
    config: Arc<C>,
    activation: CancellationToken,
}
impl<C: Send + Sync + 'static> Clone for TransportRuntime<C> {
    fn clone(&self) -> Self {
        Self {
            exposures: self.exposures.clone(),
            tracker: self.tracker.clone(),
            config: self.config.clone(),
            activation: self.activation.clone(),
        }
    }
}
#[allow(dead_code)]
impl<C: Send + Sync + 'static> TransportRuntime<C> {
    pub(crate) fn new(
        exposures: Arc<[TransportExposure]>,
        tracker: TransportTaskTracker,
        config: Arc<C>,
        activation: CancellationToken,
    ) -> Self {
        Self {
            exposures,
            tracker,
            config,
            activation,
        }
    }
    /// Returns exposures in builder-call order.
    pub fn exposures(&self) -> &[TransportExposure] {
        &self.exposures
    }
    /// Returns the composition-owned tracker shared by all bindings.
    pub fn tracker(&self) -> &TransportTaskTracker {
        &self.tracker
    }
    /// Returns this binding's concrete retained configuration.
    pub fn config(&self) -> &C {
        &self.config
    }
    /// Returns whether startup has committed and traffic may be admitted.
    pub fn is_active(&self) -> bool {
        self.activation.is_cancelled()
    }
    /// Waits until startup commits and traffic may be admitted.
    pub async fn wait_until_active(&self) {
        self.activation.cancelled().await;
    }
    pub(crate) fn activate(&self) {
        self.activation.cancel();
    }
}
/// A configured transport lifecycle participating in composition startup.
/// Conformance is repeatable and non-authorizing. Preparation only preflights
/// the complete descriptor set. Startup stays closed until activation and an
/// error leaves no live intake, task, resource, or handle requiring cleanup.
pub trait TransportBinding: Send + Sync + 'static {
    /// Concrete configuration retained for this binding.
    type Config: Send + Sync + 'static;
    /// Live handle returned after transactional startup.
    type Handle: TransportHandle;
    /// Returns the configuration shared with the binding's runtime carrier.
    fn config(&self) -> Arc<Self::Config>;
    /// Checks one requested exposure without authorizing traffic.
    fn conform(
        &self,
        descriptor: &CapabilityDescriptor,
        level: ExposureLevel,
    ) -> Result<(), Detail>;
    /// Transactionally preflights the complete ordered descriptor set.
    fn prepare(&self, descriptors: &[&'static CapabilityDescriptor]) -> Result<(), Detail>;
    /// Starts closed intake and returns its synchronous lifecycle handle.
    fn start(&self, runtime: TransportRuntime<Self::Config>) -> Result<Self::Handle, Detail>;
}
/// Synchronous lifecycle controls; dropping a handle has no defined behavior.
pub trait TransportHandle: Send + Sync + 'static {
    /// Prevents admission of new transport requests.
    fn stop_intake(&self);
    /// Requests cooperative cancellation of transport tasks.
    fn cancel_tasks(&self);
    /// Aborts transport tasks that remain live.
    fn abort_tasks(&self);
    /// Consumes the handle and joins every transport-owned task.
    fn join_tasks(self: Box<Self>) -> TransportJoinFuture;
}
#[cfg(test)]
mod tests {
    use super::{TransportTaskTracker as Tracker, *};
    use boxology_contract::{
        BoxId, Caller, CancelToken, CapabilityId, CapabilityName, CapabilityShape, ContractValue,
        Idempotency, TraceContext, TypeDescriptor,
    };
    use std::future::{Future, ready};
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};
    fn descriptor() -> &'static CapabilityDescriptor {
        Box::leak(Box::new(CapabilityDescriptor::new(
            CapabilityId::new(
                BoxId::new("transport-test").unwrap(),
                CapabilityName::new("call").unwrap(),
            ),
            TypeDescriptor::bool(),
            TypeDescriptor::bool(),
            TypeDescriptor::bool(),
            CapabilityShape::Unary,
            ExposureLevel::External,
            Idempotency::None,
            None,
        )))
    }
    fn context() -> CallContext {
        CallContext::new(
            Caller::Anonymous,
            None,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        )
    }
    fn poll_once<F: Future + ?Sized>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }
    #[derive(Default)]
    struct Target(Mutex<Vec<CapabilityId>>);
    impl ErasedTarget for Target {
        fn call<'a>(
            &'a self,
            capability: &'a CapabilityId,
            _context: CallContext,
            input: SlotValue,
        ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
            self.0.lock().unwrap().push(capability.clone());
            let result = match input {
                SlotValue::Null => Ok(SlotValue::Null),
                SlotValue::Missing => Err(ErasedCallError::Unavailable(Detail::new("test_call"))),
                SlotValue::Value(_) => panic!("guarded transport panic"),
            };
            Box::pin(ready(result))
        }
    }
    struct Config(u8);
    struct Handle;
    struct Binding;
    impl TransportHandle for Handle {
        fn stop_intake(&self) {}
        fn cancel_tasks(&self) {}
        fn abort_tasks(&self) {}
        fn join_tasks(self: Box<Self>) -> TransportJoinFuture {
            Box::pin(ready(Ok(())))
        }
    }
    impl TransportBinding for Binding {
        type Config = Config;
        type Handle = Handle;
        fn config(&self) -> Arc<Config> {
            Arc::new(Config(0))
        }
        fn conform(
            &self,
            _descriptor: &CapabilityDescriptor,
            _level: ExposureLevel,
        ) -> Result<(), Detail> {
            Ok(())
        }
        fn prepare(&self, _: &[&'static CapabilityDescriptor]) -> Result<(), Detail> {
            Ok(())
        }
        fn start(&self, _: TransportRuntime<Config>) -> Result<Handle, Detail> {
            Ok(Handle)
        }
    }
    #[test]
    fn transport_carriers_preserve_dispatch_sharing_and_activation_contracts() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<TransportExposure>();
        bounds::<TransportRuntime<Config>>();
        bounds::<Tracker>();
        let descriptor = descriptor();
        let target = Arc::new(Target::default());
        let exposure = TransportExposure::new(descriptor, ExposureLevel::Internal, target.clone());
        assert!(std::ptr::eq(exposure.descriptor(), descriptor));
        assert_eq!(exposure.level(), ExposureLevel::Internal);
        let mut success = exposure.dispatch(context(), SlotValue::Null);
        let result = poll_once(success.as_mut());
        assert_eq!(result, Poll::Ready(Ok(SlotValue::Null)));
        let mut failure = exposure.dispatch(context(), SlotValue::Missing);
        let result = poll_once(failure.as_mut());
        let expected = ErasedCallError::Unavailable(Detail::new("test_call"));
        assert_eq!(result, Poll::Ready(Err(expected)));
        let mut panic = exposure.dispatch(context(), SlotValue::Value(ContractValue::bool(true)));
        let Poll::Ready(Err(ErasedCallError::Internal(detail))) = poll_once(panic.as_mut()) else {
            panic!("transport panic escaped guarded dispatch")
        };
        assert_eq!(detail.code(), "panic");
        assert_eq!(*target.0.lock().unwrap(), vec![descriptor.id().clone(); 3]);
        let runtime = TransportRuntime::new(
            Arc::from([exposure.clone()]),
            Tracker::new(),
            Binding.config(),
            CancellationToken::new(),
        );
        let clone = runtime.clone();
        let same_tracker = Tracker::ptr_eq(&runtime.tracker, &clone.tracker);
        assert!(same_tracker);
        assert!(Arc::ptr_eq(&runtime.config, &clone.config));
        assert_eq!(runtime.config().0, 0);
        assert!(Arc::ptr_eq(&runtime.exposures, &clone.exposures));
        let token = clone.tracker().token();
        assert_eq!(runtime.tracker().len(), 1);
        drop(token);
        assert!(runtime.tracker().is_empty());
        let mut active = Box::pin(runtime.wait_until_active());
        assert!(!runtime.is_active() && matches!(poll_once(active.as_mut()), Poll::Pending));
        clone.activate();
        assert!(runtime.is_active() && matches!(poll_once(active.as_mut()), Poll::Ready(())));
    }
}
