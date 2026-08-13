mod contract;
pub use contract::*;

pub struct HelloService;

#[boxology::implementation]
impl HelloService {
    pub async fn greet(
        &self,
        context: boxology::CallContext,
        name: String,
    ) -> Result<String, GreetError> {
        let _ = context;
        if name.is_empty() {
            Err(GreetError::EmptyName)
        } else {
            Ok(format!("Hello, {name}!"))
        }
    }
}

#[allow(dead_code)]
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

    use super::{GreetError, HelloService, generated};

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
            Poll::Pending => panic!("hello implementation future unexpectedly pending"),
        }
    }

    #[test]
    fn greets_ada_exactly() {
        let result = run(HelloService.greet(context(), "Ada".into()));
        assert_eq!(result, Ok("Hello, Ada!".into()));
    }

    #[test]
    fn rejects_an_empty_name() {
        let result = run(HelloService.greet(context(), String::new()));
        assert_eq!(result, Err(GreetError::EmptyName));
    }

    #[test]
    fn service_receiver_is_send_sync_and_static() {
        fn assert_receiver<T: Send + Sync + 'static>() {}

        assert_receiver::<HelloService>();
    }

    #[test]
    fn generated_adapter_and_dispatch_are_send_sync() {
        fn assert_dispatch<
            T: hello_contract::HelloDispatch + Send + Sync + 'static,
        >() {
        }
        fn assert_bounds<T: Send + Sync + 'static>() {}

        assert_dispatch::<HelloService>();
        assert_bounds::<generated::HelloAdapter<HelloService>>();
        assert!(std::ptr::eq(
            generated::implementation_descriptor().contract(),
            hello_contract::contract_descriptor()
        ));
    }

    #[test]
    fn generated_adapter_runs_through_stub_transport() {
        let descriptor = generated::implementation_descriptor();
        let capability = descriptor.contract().capabilities()[0].id().clone();
        let transport = std::sync::Arc::new(StubTransport::new());
        let mut builder = CompositionBuilder::new();
        builder.add_box(descriptor, |imports| {
            generated::factory(HelloService, imports)
        });
        builder.expose(
            BoxId::new("hello").unwrap(),
            capability,
            transport.clone(),
            ExposureLevel::External,
        );
        let composition = builder.start().unwrap();
        let runtime = transport.runtime().unwrap();
        let exposure = &runtime.exposures()[0];
        let output = run(exposure.dispatch(context(), "Ada".to_owned().encode().unwrap())).unwrap();
        assert_eq!(String::decode(&output).unwrap(), "Hello, Ada!");

        let malformed = run(exposure.dispatch(context(), SlotValue::Null));
        let Err(ErasedCallError::ContractViolation(detail)) = malformed else {
            panic!("malformed provider input was accepted")
        };
        assert_eq!(detail.code(), "input_decode");

        let domain = run(exposure.dispatch(context(), String::new().encode().unwrap()));
        assert!(matches!(
            domain,
            Err(ErasedCallError::Domain { error_tag, .. }) if error_tag == "EmptyName"
        ));
        drop(composition);
    }
}
