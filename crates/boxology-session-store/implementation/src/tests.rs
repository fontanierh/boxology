use super::*;
use boxology_contract::{
    BoxId, CallContext, Caller, CancelToken, CapabilityId, Deadline, ErasedCallError,
    ErasedCallTarget, ExposureLevel, SlotValue, TraceContext,
};
use boxology_runtime::{
    Composition, CompositionBuilder, TransportExposure, test_support::StubTransport,
};
#[rustfmt::skip]
use std::{future::Future, os::unix::fs::symlink, pin::{Pin, pin}, sync::{Arc, atomic::{AtomicU64, Ordering}}, task::{Context, Poll, Waker}, time::Instant};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[rustfmt::skip]
struct Fixture { home: PathBuf, root: PathBuf }
#[rustfmt::skip]
impl Fixture { fn new() -> Self { let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed); let home = std::env::temp_dir().join(format!("boxology-session-{}-{suffix}", std::process::id())); let root = home.join("root"); fs::create_dir(&home).unwrap(); fs::create_dir(&root).unwrap(); Self { home, root: fs::canonicalize(root).unwrap() } } }
#[rustfmt::skip]
impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.home).unwrap(); } }
#[rustfmt::skip]
fn ready<F: Future>(future: F) -> F::Output { let mut future = pin!(future); match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) { Poll::Ready(value) => value, Poll::Pending => panic!("pending") } }
#[rustfmt::skip]
fn context() -> CallContext { CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None) }
#[rustfmt::skip]
fn append(session: &str, event: &str, sequence: u64, payload: &str) -> AppendRequest { typed(session, event, sequence, SessionEventKind::User, payload) }
#[rustfmt::skip]
fn typed(session: &str, event: &str, sequence: u64, kind: SessionEventKind, payload: &str) -> AppendRequest { AppendRequest { session_id: session.into(), expected_sequence: sequence, event: NewSessionEvent { event_id: event.into(), kind, payload_json: payload.into() } } }
#[rustfmt::skip]
fn wire(sequence: u64, event: &str, payload: &str) -> Vec<u8> { let mut bytes = serde_json::to_vec(&Record { schema: 1, version: 1, sequence, event_id: event.into(), kind: "user".into(), payload_json: payload.into() }).unwrap(); bytes.push(b'\n'); bytes }

#[rustfmt::skip]
struct Target(Vec<TransportExposure>);
#[rustfmt::skip]
impl ErasedCallTarget for Target { fn call<'a>(&'a self, capability: &'a CapabilityId, context: CallContext, input: SlotValue) -> Pin<Box<dyn Future<Output=Result<SlotValue, ErasedCallError>> + Send + 'a>> { self.0.iter().find(|item| item.descriptor().id() == capability).unwrap().dispatch(context, input) } }
#[rustfmt::skip]
fn assembled(root: PathBuf, fault: Option<Fault>) -> (Composition, boxology_generated_contract::SessionStoreHandle) { let descriptor = generated::implementation_descriptor(); let capabilities = descriptor.contract().capabilities().iter().map(|capability| capability.id().clone()).collect::<Vec<_>>(); let transport = Arc::new(StubTransport::new()); let mut builder = CompositionBuilder::new(); builder.add_box(descriptor, move |imports| { let mut service = SessionStoreService::new(root).unwrap(); service.fault = fault; generated::factory(service, imports) }); for capability in capabilities { builder.expose(BoxId::new("session-store").unwrap(), capability, transport.clone(), ExposureLevel::CodeOnly); } let composition = builder.start().unwrap(); let handle = boxology_generated_contract::SessionStoreHandle::from_erased(Arc::new(Target(transport.runtime().unwrap().exposures().to_vec()))); (composition, handle) }
#[rustfmt::skip]
fn app(handle: &boxology_generated_contract::SessionStoreHandle, ctx: CallContext, request: AppendRequest) -> AppendOutcome { let value = ready(handle.append(ctx, request)).unwrap(); assert_ne!(value.result.is_some(), value.failure.is_some()); value }
#[rustfmt::skip]
fn load(handle: &boxology_generated_contract::SessionStoreHandle, session: &str) -> LoadOutcome { ready(handle.load(context(), LoadRequest { session_id: session.into() })).unwrap() }

#[test]
#[rustfmt::skip]
fn generated_fake_preserves_typed_load_and_append() { use boxology_generated_contract::test_support::SessionStoreFake; let fake = SessionStoreFake::new().with_append(|_, request| async move { assert_eq!(request.event.payload_json, r#"{"x":1}"#); Ok(AppendOutcome { result: Some(AppendResult { sequence: 0, appended: true }), failure: None }) }).with_load(|_, _| async move { Ok(LoadOutcome { result: Some(LoadResult { events: vec![SessionEvent { sequence: 0, event_id: "e".into(), kind: SessionEventKind::User, payload_json: r#"{"x":1}"#.into() }], next_sequence: 1 }), failure: None }) }); assert!(app(&fake.handle(), context(), append("s", "e", 0, r#"{"x":1}"#)).result.unwrap().appended); assert_eq!(load(&fake.handle(), "s").result.unwrap().events[0].event_id, "e"); }

#[test]
#[rustfmt::skip]
fn adapter_restarts_resumes_replays_and_conflicts() { let fixture = Fixture::new(); { let (_composition, handle) = assembled(fixture.root.clone(), None); assert_eq!(load(&handle, "alpha").result.unwrap().next_sequence, 0); for (sequence, event, kind) in [(0, "user.0", SessionEventKind::User), (1, "assistant.0", SessionEventKind::Assistant), (2, "call.0", SessionEventKind::ToolCall), (3, "result.0", SessionEventKind::ToolResult)] { assert!(app(&handle, context(), typed("alpha", event, sequence, kind, r#"{ "exact": true }"#)).result.unwrap().appended); } } let (_composition, handle) = assembled(fixture.root.clone(), None); let loaded = load(&handle, "alpha").result.unwrap(); assert_eq!(loaded.next_sequence, 4); assert!(matches!(loaded.events[0].kind, SessionEventKind::User)); assert!(matches!(loaded.events[1].kind, SessionEventKind::Assistant)); assert!(matches!(loaded.events[2].kind, SessionEventKind::ToolCall)); assert!(matches!(loaded.events[3].kind, SessionEventKind::ToolResult)); assert!(!app(&handle, context(), typed("alpha", "user.0", 0, SessionEventKind::User, r#"{ "exact": true }"#)).result.unwrap().appended); for request in [typed("alpha", "user.0", 0, SessionEventKind::Assistant, r#"{ "exact": true }"#), append("alpha", "user.0", 0, r#"{"exact":true}"#), append("alpha", "e4", 3, "{}"), append("alpha", "user.0", 1, r#"{ "exact": true }"#)] { assert!(matches!(app(&handle, context(), request).failure.unwrap().class, SessionFailureClass::Conflict)); } assert!(app(&handle, context(), append("alpha", "e4", 4, "{}")).result.unwrap().appended); }

#[test]
#[rustfmt::skip]
fn boundaries_payload_and_corruption_fail_closed_without_leaks() { let fixture = Fixture::new(); let outside = fixture.home.join("outside"); fs::write(&outside, "canary").unwrap(); symlink(&outside, fixture.root.join("linked.jsonl")).unwrap(); fs::create_dir(fixture.root.join("directory.jsonl")).unwrap(); assert_eq!(SessionStoreService::new(fixture.root.join("..").join("root")).err().unwrap().code, "outside_root"); let (_composition, handle) = assembled(fixture.root.clone(), None); for request in [append("", "e", 0, "{}"), append("a/b", "e", 0, "{}"), append(&"s".repeat(65), "e", 0, "{}"), append("s", "", 0, "{}"), append("s", &"e".repeat(129), 0, "{}"), append("s", "e", 0, "[]"), append("s", "e", 0, "bad"), append("s", "e", 0, &format!(r#"{{"x":"{}"}}"#, "x".repeat(MAX_RECORD)))] { let failure = app(&handle, context(), request).failure.unwrap(); assert!(!failure.side_effect_possible); assert!(!failure.message.contains(fixture.root.to_str().unwrap())); } assert!(app(&handle, context(), append(&"s".repeat(64), &"e".repeat(128), 0, "{}")).result.is_some()); assert_eq!(load(&handle, "linked").failure.unwrap().code, "symlink"); assert_eq!(load(&handle, "directory").failure.unwrap().code, "not_file");
    let mut duplicate = wire(0, "same", "{}"); duplicate.extend(wire(1, "same", "{}"));
    for bytes in [b"{}\n".to_vec(), b"\n".to_vec(), wire(1, "gap", "{}"), duplicate, br#"{"schema":2,"version":1,"sequence":0,"event_id":"e","kind":"user","payload_json":"{}"}
"#.to_vec(), br#"{"schema":1,"version":1,"sequence":0,"event_id":"e","payload_json":"[]"}
"#.to_vec()] { fs::write(fixture.root.join("bad.jsonl"), bytes).unwrap(); assert!(matches!(load(&handle, "bad").failure.unwrap().class, SessionFailureClass::Corrupt)); }
    fs::write(fixture.root.join("large.jsonl"), vec![b'x'; MAX_FILE + 1]).unwrap(); assert_eq!(load(&handle, "large").failure.unwrap().code, "session_too_large"); fs::write(fixture.root.join("torn.jsonl"), vec![b'x'; MAX_RECORD + 1]).unwrap(); assert_eq!(load(&handle, "torn").failure.unwrap().code, "record_too_large"); assert_eq!(fs::read_to_string(outside).unwrap(), "canary"); }

#[test]
#[rustfmt::skip]
fn exact_event_and_file_limits_are_enforced() { let fixture = Fixture::new(); let mut bytes = Vec::new(); for sequence in 0..MAX_EVENTS { bytes.extend(wire(sequence as u64, &format!("e{sequence}"), "{}")); } fs::write(fixture.root.join("full.jsonl"), bytes).unwrap(); let (_composition, handle) = assembled(fixture.root.clone(), None); assert_eq!(load(&handle, "full").result.unwrap().next_sequence, MAX_EVENTS as u64); assert_eq!(app(&handle, context(), append("full", "overflow", MAX_EVENTS as u64, "{}")).failure.unwrap().code, "event_limit"); }

#[test]
#[rustfmt::skip]
fn oversized_valid_payload_is_rejected_before_file_creation() { let fixture = Fixture::new(); let (_composition, handle) = assembled(fixture.root.clone(), None); let payload = format!(r#"{{"value":"{}"}}"#, "x".repeat(MAX_RECORD)); let failure = app(&handle, context(), append("oversized", "e", 0, &payload)).failure.unwrap(); assert!(matches!(failure.class, SessionFailureClass::Resource)); assert_eq!(failure.code, "record_too_large"); assert!(!failure.side_effect_possible); assert!(!fixture.root.join("oversized.jsonl").exists()); }

#[test]
#[rustfmt::skip]
fn torn_and_ambiguous_writes_recover_deterministically() { let fixture = Fixture::new(); let (_composition, torn) = assembled(fixture.root.clone(), Some(Fault::TornWrite)); assert!(app(&torn, context(), append("s", "e0", 0, "{}")).failure.unwrap().side_effect_possible); drop(torn); let (_composition, clean) = assembled(fixture.root.clone(), None); assert_eq!(load(&clean, "s").result.unwrap().next_sequence, 0); assert!(app(&clean, context(), append("s", "e0", 0, "{}")).result.unwrap().appended); let (_composition, ambiguous) = assembled(fixture.root.clone(), Some(Fault::PreSync)); assert!(app(&ambiguous, context(), append("s", "e1", 1, r#"{"v":1}"#)).failure.unwrap().side_effect_possible); let (_composition, restarted) = assembled(fixture.root.clone(), None); assert!(!app(&restarted, context(), append("s", "e1", 1, r#"{"v":1}"#)).result.unwrap().appended); assert_eq!(load(&restarted, "s").result.unwrap().next_sequence, 2); }

#[test]
#[rustfmt::skip]
fn cancellation_deadline_and_sync_faults_are_typed() { let fixture = Fixture::new(); let (_composition, handle) = assembled(fixture.root.clone(), None); let token = CancelToken::new(); token.cancel(); assert!(!app(&handle, CallContext::new(Caller::Anonymous, None, token, TraceContext::empty(), None), append("cancel", "e", 0, "{}")).failure.unwrap().side_effect_possible); let expired = CallContext::new(Caller::Anonymous, Some(Deadline::at(Instant::now())), CancelToken::new(), TraceContext::empty(), None); assert_eq!(app(&handle, expired, append("expired", "e", 0, "{}")).failure.unwrap().code, "deadline_exceeded"); for (fault, session) in [(Fault::PreWriteCancel, "prewrite"), (Fault::FileSync, "sync"), (Fault::ParentSync, "parent")] { let (_composition, broken) = assembled(fixture.root.clone(), Some(fault)); let failure = app(&broken, context(), append(session, "e", 0, "{}")).failure.unwrap(); assert_eq!(failure.code, if fault == Fault::PreWriteCancel { "cancelled" } else { "local_io" }); assert!(failure.side_effect_possible); } assert!(!fixture.root.join("cancel.jsonl").exists()); assert!(!fixture.root.join("expired.jsonl").exists()); }

#[test]
#[rustfmt::skip]
fn created_logs_are_owner_only() { use std::os::unix::fs::PermissionsExt; let fixture = Fixture::new(); let (_composition, handle) = assembled(fixture.root.clone(), None); assert!(app(&handle, context(), append("private", "e", 0, "{}")).result.is_some()); assert_eq!(fs::metadata(fixture.root.join("private.jsonl")).unwrap().permissions().mode() & 0o777, 0o600); }
