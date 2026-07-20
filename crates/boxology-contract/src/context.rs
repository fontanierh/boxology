//! Transport-neutral invocation context primitives.

use std::error::Error;
use std::fmt;
use std::time::{Duration, Instant};

/// The identity on whose behalf a call is made.
///
/// This is an authentication placeholder until the authorization model is
/// introduced.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Caller {
    /// No authenticated caller identity is available.
    Anonymous,
    /// A trusted runtime subsystem identified by a static name.
    System(&'static str),
}

/// An absolute deadline on the process-local monotonic clock.
///
/// A deadline is expired when its remaining duration is [`Duration::ZERO`],
/// including exactly at the deadline boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Deadline(Instant);

impl Deadline {
    /// Constructs a deadline at an absolute monotonic-clock instant.
    pub fn at(instant: Instant) -> Self {
        Self(instant)
    }

    /// Returns the absolute monotonic-clock instant.
    pub fn instant(&self) -> Instant {
        self.0
    }

    /// Returns the remaining duration at `now`, saturating at zero.
    ///
    /// The result is [`Duration::ZERO`] when `now` is exactly at or after the
    /// deadline.
    pub fn remaining_at(&self, now: Instant) -> Duration {
        self.0.saturating_duration_since(now)
    }

    /// Returns the remaining duration using the current monotonic-clock time.
    ///
    /// Equality with [`Duration::ZERO`] is the expiry predicate, including
    /// exactly at the deadline boundary.
    pub fn remaining(&self) -> Duration {
        self.remaining_at(Instant::now())
    }
}

/// An advisory cancellation signal shared with clones.
///
/// Cancellation requests cooperative termination and does not roll back work
/// already performed. A child observes cancellation of its parent, while
/// cancelling a child does not cancel its parent.
#[derive(Debug, Clone)]
pub struct CancelToken(tokio_util::sync::CancellationToken);

impl CancelToken {
    /// Constructs a fresh, uncancelled token.
    pub fn new() -> Self {
        Self(tokio_util::sync::CancellationToken::new())
    }

    /// Signals cancellation to this token and its descendants.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// Returns whether cancellation has been signalled.
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }

    /// Waits until cancellation is signalled.
    pub async fn cancelled(&self) {
        self.0.cancelled().await;
    }

    /// Derives a token cancelled by its parent but isolated in reverse.
    pub fn child_token(&self) -> Self {
        Self(self.0.child_token())
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Opaque distributed-tracing headers carried without parsing or validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    traceparent: Option<String>,
    tracestate: Option<String>,
}

impl TraceContext {
    /// Constructs a context without tracing headers.
    pub fn empty() -> Self {
        Self {
            traceparent: None,
            tracestate: None,
        }
    }

    /// Carries the supplied header strings exactly, without validating them.
    pub fn new(traceparent: Option<String>, tracestate: Option<String>) -> Self {
        Self {
            traceparent,
            tracestate,
        }
    }

    /// Returns the uninterpreted `traceparent` string, when present.
    pub fn traceparent(&self) -> Option<&str> {
        self.traceparent.as_deref()
    }

    /// Returns the uninterpreted `tracestate` string, when present.
    pub fn tracestate(&self) -> Option<&str> {
        self.tracestate.as_deref()
    }
}

/// A call's idempotency key, transported but never honored in v0.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Constructs a key, rejecting only the empty string.
    pub fn new(value: impl Into<String>) -> Result<Self, IdempotencyKeyError> {
        let value = value.into();
        if value.is_empty() {
            Err(IdempotencyKeyError)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the exact key content.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The supplied idempotency key was empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdempotencyKeyError;

impl fmt::Display for IdempotencyKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("idempotency key must not be empty")
    }
}

impl Error for IdempotencyKeyError {}

/// Explicit transport-neutral metadata and control state for one call.
///
/// Construction supplies every field directly; no ambient or task-local state
/// is consulted. Idempotency keys are transported but never honored in v0.
#[derive(Debug, Clone)]
pub struct CallContext {
    caller: Caller,
    deadline: Option<Deadline>,
    cancellation: CancelToken,
    trace: TraceContext,
    idempotency_key: Option<IdempotencyKey>,
}

impl CallContext {
    /// Constructs a call context from entirely explicit state.
    pub fn new(
        caller: Caller,
        deadline: Option<Deadline>,
        cancellation: CancelToken,
        trace: TraceContext,
        idempotency_key: Option<IdempotencyKey>,
    ) -> Self {
        Self {
            caller,
            deadline,
            cancellation,
            trace,
            idempotency_key,
        }
    }

    /// Returns the caller identity.
    pub fn caller(&self) -> Caller {
        self.caller
    }

    /// Returns the absolute deadline, when one was supplied.
    pub fn deadline(&self) -> Option<Deadline> {
        self.deadline
    }

    /// Returns this call's advisory cancellation token.
    pub fn cancellation(&self) -> &CancelToken {
        &self.cancellation
    }

    /// Returns the opaque tracing context.
    pub fn trace(&self) -> &TraceContext {
        &self.trace
    }

    /// Returns the transported v0 idempotency key, when one was supplied.
    pub fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }

    /// Derives explicit context for a nested call.
    ///
    /// The child inherits caller, absolute deadline, and tracing context. Its
    /// cancellation token follows parent-to-child cancellation, while child
    /// cancellation does not affect the parent. The operation-scoped
    /// idempotency key is transported but never honored in v0 and is always
    /// dropped here rather than blindly inherited. A different deadline
    /// requires constructing a fresh context.
    pub fn child(&self) -> CallContext {
        Self {
            caller: self.caller,
            deadline: self.deadline,
            cancellation: self.cancellation.child_token(),
            trace: self.trace.clone(),
            idempotency_key: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use super::*;

    #[test]
    fn deadline_remaining_saturates_at_and_after_expiry() {
        let before = Instant::now();
        let instant = before + Duration::from_secs(5);
        let deadline = Deadline::at(instant);

        assert_eq!(deadline.instant(), instant);
        assert_eq!(deadline.remaining_at(before), Duration::from_secs(5));
        assert_eq!(deadline.remaining_at(instant), Duration::ZERO);
        assert_eq!(
            deadline.remaining_at(instant + Duration::from_nanos(1)),
            Duration::ZERO
        );
    }

    #[test]
    fn fresh_token_can_be_cancelled() {
        let token = CancelToken::new();
        let clone = token.clone();
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.is_cancelled());
        assert!(clone.is_cancelled());
    }

    #[test]
    fn cancellation_propagates_from_parent_but_not_from_child() {
        let parent = CancelToken::new();
        let child = parent.child_token();
        let grandchild = child.child_token();

        child.cancel();
        assert!(!parent.is_cancelled());
        assert!(child.is_cancelled());
        assert!(grandchild.is_cancelled());

        let parent = CancelToken::new();
        let child = parent.child_token();
        let grandchild = child.child_token();
        parent.cancel();
        assert!(child.is_cancelled());
        assert!(grandchild.is_cancelled());
    }

    #[test]
    fn cancelled_future_transitions_without_an_async_runtime() {
        let token = CancelToken::new();
        let mut cancelled = pin!(token.cancelled());
        let mut context = Context::from_waker(Waker::noop());
        assert!(matches!(
            cancelled.as_mut().poll(&mut context),
            Poll::Pending
        ));

        token.cancel();
        assert!(matches!(
            cancelled.as_mut().poll(&mut context),
            Poll::Ready(())
        ));

        let token = CancelToken::new();
        token.cancel();
        let mut already_cancelled = pin!(token.cancelled());
        assert!(matches!(
            already_cancelled.as_mut().poll(&mut context),
            Poll::Ready(())
        ));
    }

    #[test]
    fn trace_context_preserves_uninterpreted_strings() {
        let trace = TraceContext::new(Some("not a traceparent".into()), Some("\0=garbage".into()));
        assert_eq!(trace.traceparent(), Some("not a traceparent"));
        assert_eq!(trace.tracestate(), Some("\0=garbage"));
        assert_eq!(TraceContext::empty().traceparent(), None);
        assert_eq!(TraceContext::empty().tracestate(), None);
    }

    #[test]
    fn idempotency_key_rejects_only_empty_content() {
        assert_eq!(IdempotencyKey::new(""), Err(IdempotencyKeyError));
        assert_eq!(
            IdempotencyKeyError.to_string(),
            "idempotency key must not be empty"
        );

        let content = "  arbitrary key \0 ";
        assert_eq!(IdempotencyKey::new(content).unwrap().as_str(), content);
    }

    #[test]
    fn fully_populated_call_context_round_trips_through_accessors() {
        let caller = Caller::System("test-runtime");
        let deadline = Deadline::at(Instant::now() + Duration::from_secs(30));
        let cancellation = CancelToken::new();
        let cancellation_probe = cancellation.clone();
        let trace = TraceContext::new(Some("opaque-parent".into()), Some("opaque-state".into()));
        let key = IdempotencyKey::new("operation-17").unwrap();

        let context = CallContext::new(
            caller,
            Some(deadline),
            cancellation,
            trace.clone(),
            Some(key),
        );

        assert_eq!(context.caller(), caller);
        assert_eq!(context.deadline(), Some(deadline));
        assert!(!context.cancellation().is_cancelled());
        cancellation_probe.cancel();
        assert!(context.cancellation().is_cancelled());
        assert_eq!(context.trace(), &trace);
        assert_eq!(
            context.idempotency_key().map(IdempotencyKey::as_str),
            Some("operation-17")
        );
        assert!(format!("{context:?}").starts_with("CallContext"));
    }

    #[test]
    fn child_inherits_call_state_and_drops_operation_key() {
        let deadline = Deadline::at(Instant::now() + Duration::from_secs(30));
        let trace = TraceContext::new(Some("trace".into()), Some("state".into()));
        let parent = CallContext::new(
            Caller::System("parent"),
            Some(deadline),
            CancelToken::new(),
            trace.clone(),
            Some(IdempotencyKey::new("parent-operation").unwrap()),
        );

        let child = parent.child();

        assert_eq!(child.caller(), parent.caller());
        assert_eq!(child.deadline(), parent.deadline());
        assert_eq!(child.deadline().unwrap().instant(), deadline.instant());
        assert_eq!(child.trace(), &trace);
        assert_eq!(child.idempotency_key(), None);
    }

    #[test]
    fn child_cancellation_is_directional() {
        let parent = CallContext::new(
            Caller::Anonymous,
            None,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        );
        let child = parent.child();
        child.cancellation().cancel();
        assert!(!parent.cancellation().is_cancelled());
        assert!(child.cancellation().is_cancelled());

        let parent = CallContext::new(
            Caller::Anonymous,
            None,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        );
        let child = parent.child();
        parent.cancellation().cancel();
        assert!(child.cancellation().is_cancelled());
    }

    #[test]
    fn child_preserves_an_absent_deadline() {
        let parent = CallContext::new(
            Caller::Anonymous,
            None,
            CancelToken::new(),
            TraceContext::empty(),
            None,
        );

        assert_eq!(parent.child().deadline(), None);
    }

    #[test]
    fn call_context_has_thread_safe_static_bounds() {
        fn assert_bounds<T: Send + Sync + 'static>() {}

        assert_bounds::<CallContext>();
    }

    #[test]
    fn context_primitives_have_thread_safe_static_bounds() {
        fn assert_bounds<T: Send + Sync + 'static>() {}
        fn assert_send<T: Send>(_: T) {}

        assert_bounds::<Caller>();
        assert_bounds::<Deadline>();
        assert_bounds::<CancelToken>();
        assert_bounds::<TraceContext>();
        assert_bounds::<IdempotencyKey>();
        assert_bounds::<IdempotencyKeyError>();
        let token = CancelToken::new();
        assert_send(token.cancelled());
    }
}
