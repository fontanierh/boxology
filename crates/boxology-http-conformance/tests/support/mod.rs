use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use boxology_contract::{
    BoxId, CapabilityDescriptor, CapabilityId, CapabilityName, ErasedCallError, ExposureLevel,
};
use boxology_http::{HttpClientConfig, HttpClientTarget, HttpServerBinding, HttpServerConfig};
use boxology_runtime::{Composition, CompositionBuilder};
use hello_contract::HelloHandle;

const ASSERTION_TIMEOUT: Duration = Duration::from_secs(5);

pub struct RunningHello {
    address: std::net::SocketAddr,
    composition: Composition,
    greet: &'static CapabilityDescriptor,
}

impl RunningHello {
    pub fn start<T: hello_contract::HelloDispatch>(service: T) -> Self {
        Self::start_with_config(
            service,
            HttpServerConfig::new("127.0.0.1:0".parse().expect("loopback address is valid")),
        )
    }

    pub fn start_with_config<T: hello_contract::HelloDispatch>(
        service: T,
        config: HttpServerConfig,
    ) -> Self {
        let greet = hello_greet();
        let binding = Arc::new(HttpServerBinding::new(config));
        let mut builder = CompositionBuilder::new();
        builder.add_box(
            hello_implementation::generated::implementation_descriptor(),
            |imports| hello_implementation::generated::factory(service, imports),
        );
        builder.expose(
            greet.id().box_id().clone(),
            greet.id().clone(),
            Arc::clone(&binding),
            ExposureLevel::External,
        );
        let composition = builder.start().expect("generated Hello composition starts");
        let address = binding
            .local_addr()
            .expect("HTTP binding has a resolved address");
        Self {
            address,
            composition,
            greet,
        }
    }

    pub fn handle(&self) -> HelloHandle {
        let config = HttpClientConfig::new(format!("http://{}", self.address))
            .expect("running Hello address is a valid HTTP origin");
        let target = HttpClientTarget::new(config, [self.greet])
            .expect("generated hello.greet capability conforms to HTTP");
        HelloHandle::from_erased(Arc::new(target))
    }

    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.address
    }

    pub async fn shutdown(self) -> Result<(), ErasedCallError> {
        self.composition.shutdown(Duration::from_secs(1)).await
    }

    pub async fn assert_then_shutdown<F>(self, assertions: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.assert_then_shutdown_with(assertions, ASSERTION_TIMEOUT, |result| result)
            .await;
    }

    pub async fn assert_then_shutdown_with<F, M>(
        self,
        assertions: F,
        assertion_timeout: Duration,
        map_shutdown: M,
    ) where
        F: Future<Output = ()> + Send + 'static,
        M: FnOnce(Result<(), ErasedCallError>) -> Result<(), ErasedCallError> + Send + 'static,
    {
        let assertion_result = bounded_assertions(assertions, assertion_timeout).await;
        let shutdown_result = map_shutdown(self.shutdown().await);
        finish(assertion_result, shutdown_result, assertion_timeout);
    }
}

enum AssertionResult {
    Joined(Result<(), tokio::task::JoinError>),
    TimedOut,
}

async fn bounded_assertions<F>(assertions: F, limit: Duration) -> AssertionResult
where
    F: Future<Output = ()> + Send + 'static,
{
    let mut task = tokio::spawn(assertions);
    match tokio::time::timeout(limit, &mut task).await {
        Ok(result) => AssertionResult::Joined(result),
        Err(_) => {
            task.abort();
            let _ = task.await;
            AssertionResult::TimedOut
        }
    }
}

fn finish(
    assertion: AssertionResult,
    shutdown: Result<(), ErasedCallError>,
    assertion_timeout: Duration,
) {
    match (assertion, shutdown) {
        (AssertionResult::Joined(Err(assertion)), shutdown) if assertion.is_panic() => {
            if let Err(error) = shutdown {
                eprintln!("HTTP shutdown also failed after assertion panic: {error}");
            }
            std::panic::resume_unwind(assertion.into_panic());
        }
        (AssertionResult::Joined(Ok(())), Ok(())) => {}
        (AssertionResult::Joined(Err(error)), Ok(())) => {
            panic!("assertion task failed: {error}");
        }
        (AssertionResult::Joined(Ok(())), Err(error)) => {
            panic!("HTTP shutdown failed: {error}");
        }
        (AssertionResult::Joined(Err(assertion)), Err(shutdown)) => {
            panic!("assertion task failed: {assertion}; HTTP shutdown also failed: {shutdown}");
        }
        (AssertionResult::TimedOut, Ok(())) => {
            panic!("assertion task exceeded {assertion_timeout:?}");
        }
        (AssertionResult::TimedOut, Err(shutdown)) => {
            panic!(
                "assertion task exceeded {assertion_timeout:?}; HTTP shutdown also failed: {shutdown}"
            );
        }
    }
}

fn hello_greet() -> &'static CapabilityDescriptor {
    let id = CapabilityId::new(
        BoxId::new("hello").expect("fixture box id is valid"),
        CapabilityName::new("greet").expect("fixture capability name is valid"),
    );
    let mut matches = hello_contract::contract_descriptor()
        .capabilities()
        .iter()
        .filter(|capability| capability.id() == &id);
    let greet = matches
        .next()
        .expect("generated Hello contract is missing hello.greet");
    assert!(
        matches.next().is_none(),
        "generated Hello contract contains duplicate hello.greet"
    );
    greet
}
