use boxology_contract::CallContext;
use hello_contract::GreetError;

pub struct HelloService;

impl HelloService {
    pub async fn greet(&self, context: CallContext, name: String) -> Result<String, GreetError> {
        let _ = context;
        if name.is_empty() {
            Err(GreetError::EmptyName)
        } else {
            Ok(format!("Hello, {name}!"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use boxology_contract::{CallContext, Caller, CancelToken, TraceContext};
    use hello_contract::GreetError;

    use super::HelloService;

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
}
