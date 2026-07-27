boxology::contract! {
    #[error]
    pub enum HelloError {
        EmptyName,
    }

    #[capability(exposure = external)]
    pub async fn ping(nonce: u64) -> Result<u64, HelloError>;
}

pub struct PingService;

#[boxology::implementation]
impl PingService {
    pub async fn ping(
        &self,
        context: boxology::CallContext,
        nonce: u64,
    ) -> Result<u64, HelloError> {
        let _ = context;
        Ok(nonce)
    }
}

pub mod generated {
    include!("../../generated/adapter/adapter.rs");
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use boxology_contract::{
        BoxId, CallContext, Caller, CancelToken, ContractType, ErasedCallError, ExposureLevel,
        SlotValue, TraceContext,
    };
    use boxology_runtime::{CompositionBuilder, test_support::StubTransport};

    use super::{PingService, generated};

    fn context() -> CallContext {
        CallContext::new(
            Caller::Anonymous,
            None,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        )
    }

    fn run<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("ping implementation future unexpectedly pending"),
        }
    }

    #[test]
    fn generated_adapter_and_dispatch_are_send_sync() {
        fn assert_receiver<T: Send + Sync + 'static>() {}
        fn assert_dispatch<T: boxology_generated_contract::PingDispatch + Send + Sync + 'static>() {
        }
        fn assert_bounds<T: Send + Sync + 'static>() {}

        assert_receiver::<PingService>();
        assert_dispatch::<PingService>();
        assert_bounds::<generated::PingAdapter<PingService>>();
        assert!(std::ptr::eq(
            generated::implementation_descriptor().contract(),
            boxology_generated_contract::contract_descriptor()
        ));
    }

    #[test]
    fn generated_adapter_echoes_distinct_nonces_and_rejects_malformed_input() {
        let descriptor = generated::implementation_descriptor();
        let capability = descriptor.contract().capabilities()[0].id().clone();
        let transport = std::sync::Arc::new(StubTransport::new());
        let mut builder = CompositionBuilder::new();
        builder.add_box(descriptor, |imports| {
            generated::factory(PingService, imports)
        });
        builder.expose(
            BoxId::new("ping").unwrap(),
            capability,
            transport.clone(),
            ExposureLevel::External,
        );
        let composition = builder.start().unwrap();
        let runtime = transport.runtime().unwrap();
        let exposure = &runtime.exposures()[0];

        let first = run(exposure.dispatch(context(), 17_u64.encode().unwrap())).unwrap();
        let second = run(exposure.dispatch(context(), 9_001_u64.encode().unwrap())).unwrap();
        assert_eq!(u64::decode(&first).unwrap(), 17);
        assert_eq!(u64::decode(&second).unwrap(), 9_001);

        let Err(ErasedCallError::ContractViolation(detail)) =
            run(exposure.dispatch(context(), SlotValue::Null))
        else {
            panic!("malformed provider input was accepted")
        };
        assert_eq!(detail.code(), "input_decode");
        drop(composition);
    }
}
