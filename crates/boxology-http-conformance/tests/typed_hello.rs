mod support;

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use boxology_contract::{
    CallContext, CallError, Caller, CancelToken, Detail, ErasedCallError, IdempotencyKey,
    TraceContext,
};
use hello_contract::{GreetError, HelloDispatch};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use support::RunningHello;

fn context(key: Option<IdempotencyKey>) -> CallContext {
    CallContext::new(
        Caller::Anonymous,
        None,
        CancelToken::new(),
        TraceContext::empty(),
        key,
    )
}

#[test]
fn named_evidence_resolves_through_inventory() {
    boxology_http_conformance::assert_named_evidence_resolution(
        "typed_hello",
        &[
            (
                "typed_hello_round_trips_success_and_domain_error",
                typed_hello_round_trips_success_and_domain_error as *const (),
            ),
            (
                "assertion_panic_survives_a_forced_shutdown_error",
                assertion_panic_survives_a_forced_shutdown_error as *const (),
            ),
            (
                "stalled_assertions_are_aborted_before_shutdown",
                stalled_assertions_are_aborted_before_shutdown as *const (),
            ),
            (
                "typed_hello_preserves_keys_and_executes_each_serial_call",
                typed_hello_preserves_keys_and_executes_each_serial_call as *const (),
            ),
        ],
    );
}

#[tokio::test]
async fn typed_hello_round_trips_success_and_domain_error() {
    let running = RunningHello::start(hello_implementation::HelloService);
    let handle = running.handle();

    running
        .assert_then_shutdown(async move {
            assert_eq!(
                handle.greet(context(None), "Ada".to_owned()).await,
                Ok("Hello, Ada!".to_owned())
            );
            assert_eq!(
                handle.greet(context(None), "Grace".to_owned()).await,
                Ok("Hello, Grace!".to_owned())
            );
            assert_eq!(
                handle.greet(context(None), String::new()).await,
                Err(CallError::Domain(GreetError::EmptyName))
            );
        })
        .await;
}

#[tokio::test]
async fn assertion_panic_survives_a_forced_shutdown_error() {
    let running = RunningHello::start(hello_implementation::HelloService);
    let address = running.local_addr();
    let finalizer = tokio::spawn(running.assert_then_shutdown_with(
        async {
            panic!("assertion sentinel");
        },
        Duration::from_secs(1),
        |actual| {
            actual.expect("real HTTP shutdown failed");
            Err(ErasedCallError::Internal(Detail::new("forced_shutdown")))
        },
    ));
    let payload = finalizer_panic_after_listener_closes(address, finalizer).await;
    assert_eq!(panic_message(payload.as_ref()), Some("assertion sentinel"));
}

#[tokio::test]
async fn stalled_assertions_are_aborted_before_shutdown() {
    let running = RunningHello::start(hello_implementation::HelloService);
    let address = running.local_addr();
    let finalizer = tokio::spawn(running.assert_then_shutdown_with(
        std::future::pending(),
        Duration::from_millis(25),
        |result| result,
    ));
    let payload = finalizer_panic_after_listener_closes(address, finalizer).await;
    assert_eq!(
        panic_message(payload.as_ref()),
        Some("assertion task exceeded 25ms")
    );
}

async fn finalizer_panic_after_listener_closes(
    address: std::net::SocketAddr,
    mut finalizer: JoinHandle<()>,
) -> Box<dyn Any + Send> {
    let outcome = match timeout(Duration::from_secs(5), &mut finalizer).await {
        Ok(result) => result.expect_err("assertion panic was swallowed"),
        Err(_) => {
            finalizer.abort();
            let _ = finalizer.await;
            panic!("shutdown finalizer exceeded five seconds");
        }
    };
    let connection = timeout(Duration::from_secs(1), TcpStream::connect(address))
        .await
        .expect("post-shutdown connection attempt exceeded one second");
    assert!(
        connection.is_err(),
        "HTTP listener accepted a connection after shutdown"
    );
    assert!(outcome.is_panic());
    outcome.into_panic()
}

fn panic_message(payload: &(dyn Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
}

#[derive(Debug, PartialEq, Eq)]
struct Observation {
    key: Option<IdempotencyKey>,
    ordinal: usize,
}

struct RecordingHello {
    observations: Arc<Mutex<Vec<Observation>>>,
}

impl HelloDispatch for RecordingHello {
    fn greet<'a>(
        &'a self,
        context: CallContext,
        _name: String,
    ) -> Pin<Box<dyn Future<Output = Result<String, GreetError>> + Send + 'a>> {
        let mut observations = self.observations.lock().unwrap();
        let ordinal = observations.len() + 1;
        observations.push(Observation {
            key: context.idempotency_key().cloned(),
            ordinal,
        });
        Box::pin(async move { Ok(format!("execution-{ordinal}")) })
    }
}

#[tokio::test]
async fn typed_hello_preserves_keys_and_executes_each_serial_call() {
    let observations = Arc::new(Mutex::new(Vec::new()));
    let running = RunningHello::start(RecordingHello {
        observations: Arc::clone(&observations),
    });
    let handle = running.handle();

    running
        .assert_then_shutdown(async move {
            let key = IdempotencyKey::new("same").unwrap();
            let first = handle
                .greet(context(Some(key.clone())), "Ada".to_owned())
                .await;
            let second = handle
                .greet(context(Some(key.clone())), "Ada".to_owned())
                .await;

            assert_eq!(first, Ok("execution-1".to_owned()));
            assert_eq!(second, Ok("execution-2".to_owned()));
            let observed = observations.lock().unwrap();
            assert_eq!(observed.len(), 2);
            assert_eq!(
                observed.as_slice(),
                [
                    Observation {
                        key: Some(key.clone()),
                        ordinal: 1,
                    },
                    Observation {
                        key: Some(key),
                        ordinal: 2,
                    },
                ]
            );
        })
        .await;
}
