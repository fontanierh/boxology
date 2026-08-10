use super::api;
use super::state::{self, BotFingerprint, EventRecord, Pairing, Paths};
use super::{ENABLED_VARIABLE, ExitClass, SCHEMA, TelegramService, generated, test_guard};
use boxology_contract::{
    BoxId, CallContext, Caller, CancelToken, CapabilityId, ErasedCallError, ErasedCallTarget,
    ExposureLevel, SlotValue, TraceContext,
};
use boxology_runtime::{
    Composition, CompositionBuilder, TransportExposure, test_support::StubTransport,
};
use serde_json::{Value, json};
use std::fs;
use std::future::Future;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::pin::{Pin, pin};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll, Waker};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fake {
    origin: String,
    requests: Arc<Mutex<Vec<Vec<u8>>>>,
    join: Option<JoinHandle<()>>,
}

impl Fake {
    fn new(responses: Vec<Option<String>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fake listener");
        let origin = format!("http://{}", listener.local_addr().expect("fake address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let join = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("fake request");
                let request = read_request(&mut stream);
                captured.lock().expect("fake requests").push(request);
                if let Some(body) = response {
                    let wire = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(wire.as_bytes()).expect("fake response");
                }
            }
        });
        Self {
            origin,
            requests,
            join: Some(join),
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().expect("fake requests").len()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            join.join().expect("fake server");
        }
    }
}

struct Context {
    _guard: std::sync::MutexGuard<'static, ()>,
    root: PathBuf,
    fake: Fake,
}

impl Context {
    fn new(responses: Vec<Option<String>>) -> Self {
        let guard = test_guard();
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp directory")
            .join(format!("boxology-telegram-test-{suffix}"));
        fs::create_dir(&root).expect("test state home");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .expect("test permissions");
        }
        set_env(&root);
        let fake = Fake::new(responses);
        api::set_test_origin(Some(fake.origin.clone()));
        Self {
            _guard: guard,
            root,
            fake,
        }
    }

    fn replace_fake(&mut self, responses: Vec<Option<String>>) {
        api::set_test_origin(None);
        let old = std::mem::replace(&mut self.fake, Fake::new(responses));
        drop(old);
        api::set_test_origin(Some(self.fake.origin.clone()));
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        api::set_test_origin(None);
        unsafe {
            std::env::remove_var(ENABLED_VARIABLE);
            std::env::remove_var("BOXOLOGY_TELEGRAM_BOT_TOKEN");
            std::env::remove_var("BOXOLOGY_TELEGRAM_BOT_TOKEN_FILE");
            std::env::remove_var("BOXOLOGY_TELEGRAM_HOME");
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn set_env(root: &PathBuf) {
    unsafe {
        std::env::set_var(ENABLED_VARIABLE, "1");
        std::env::set_var("BOXOLOGY_TELEGRAM_BOT_TOKEN", "999:fake-token");
        std::env::set_var("BOXOLOGY_TELEGRAM_HOME", root);
    }
}

fn ok(result: &str) -> Value {
    let value: Value = serde_json::from_str(result).expect("JSON envelope");
    assert_eq!(value["ok"], true, "{result}");
    value["data"].clone()
}

fn response(result: &Value) -> Option<String> {
    Some(json!({"ok": true, "result": result}).to_string())
}

fn raw(body: &str) -> Option<String> {
    Some(body.to_string())
}

fn run(args: &[&str], request: Value) -> (String, ExitClass) {
    super::execute(
        &args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>(),
        &serde_json::to_vec(&request).expect("request"),
    )
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .expect("fake timeout");
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(size) => {
                bytes.extend_from_slice(&chunk[..size]);
                if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let header = String::from_utf8_lossy(&bytes[..header_end]);
                    let length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length:"))
                        .and_then(|length| length.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if bytes.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
        }
    }
    bytes
}

fn request_body(request: &[u8]) -> Value {
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("request headers");
    serde_json::from_slice(&request[header_end + 4..]).expect("JSON request body")
}

fn call_context() -> CallContext {
    CallContext::new(
        Caller::Anonymous,
        None,
        CancelToken::new(),
        TraceContext::empty(),
        None,
    )
}

fn run_ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut TaskContext::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("Telegram in-process call unexpectedly pending"),
    }
}

struct ExposureTarget(Vec<TransportExposure>);

impl ErasedCallTarget for ExposureTarget {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        self.0
            .iter()
            .find(|exposure| exposure.descriptor().id() == capability)
            .expect("capability is exposed")
            .dispatch(context, input)
    }
}

fn assembled_telegram(
    capability_names: &[&str],
) -> (Composition, boxology_generated_contract::TelegramHandle) {
    let descriptor = generated::implementation_descriptor();
    let capabilities = capability_names
        .iter()
        .map(|name| {
            descriptor
                .contract()
                .capabilities()
                .iter()
                .find(|capability| capability.name().as_str() == *name)
                .unwrap_or_else(|| panic!("missing generated capability {name}"))
                .id()
                .clone()
        })
        .collect::<Vec<_>>();
    let transport = Arc::new(StubTransport::new());
    let mut builder = CompositionBuilder::new();
    builder.add_box(descriptor, |imports| {
        generated::factory(TelegramService, imports)
    });
    for capability in capabilities {
        builder.expose(
            BoxId::new("telegram").unwrap(),
            capability,
            transport.clone(),
            ExposureLevel::CodeOnly,
        );
    }
    let composition = builder.start().expect("Telegram composition starts");
    let runtime = transport.runtime().expect("stub transport starts");
    assert_eq!(runtime.exposures().len(), capability_names.len());
    let telegram = boxology_generated_contract::TelegramHandle::from_erased(Arc::new(
        ExposureTarget(runtime.exposures().to_vec()),
    ));
    (composition, telegram)
}

#[test]
fn pairing_is_explicit_private_and_durable() {
    let mut context = Context::new(vec![
        response(&json!({"id": 7, "is_bot": true, "username": "fake_bot"})),
        response(&json!({"url": ""})),
    ]);
    let (begin, exit) = run(&["pair", "begin"], json!({"schema": SCHEMA}));
    assert_eq!(exit, ExitClass::Success);
    let link = ok(&begin)["deep_link"]
        .as_str()
        .expect("deep link")
        .to_string();
    let payload = link.rsplit("?start=").next().expect("payload");
    context.replace_fake(vec![response(&json!([{"update_id": 10, "message": {"message_id": 1, "from": {"id": 42, "is_bot": false}, "chat": {"id": 42, "type": "private"}, "text": format!("/start {payload}")}}])), response(&json!({"message_id": 2}))]);
    let (complete, exit) = run(
        &["pair", "complete"],
        json!({"schema": SCHEMA, "timeout_seconds": 0}),
    );
    assert_eq!(exit, ExitClass::Success, "{complete}");
    assert_eq!(ok(&complete)["chat_id"], 42);
    let paths = Paths::from_env().expect("paths");
    let state = state::read(&paths).expect("state");
    assert_eq!(state.pairing.map(|pair| pair.user_id), Some(42));
    assert_eq!(state.next_offset, 11);
}

#[test]
fn pairing_rejects_invalid_private_user_chat_ids() {
    let mut context = Context::new(vec![
        response(&json!({"id": 7, "is_bot": true, "username": "fake_bot"})),
        response(&json!({"url": ""})),
    ]);
    let (begin, exit) = run(&["pair", "begin"], json!({"schema": SCHEMA}));
    assert_eq!(exit, ExitClass::Success, "{begin}");
    let payload = ok(&begin)["deep_link"]
        .as_str()
        .unwrap()
        .rsplit("?start=")
        .next()
        .unwrap()
        .to_string();
    context.replace_fake(vec![
        response(&json!([
            {"update_id": 10, "message": {"message_id": 1, "from": {"id": 0, "is_bot": false}, "chat": {"id": 0, "type": "private"}, "text": format!("/start {payload}")}},
            {"update_id": 11, "message": {"message_id": 2, "from": {"id": 42, "is_bot": false}, "chat": {"id": 99, "type": "private"}, "text": format!("/start {payload}")}},
            {"update_id": 12, "message": {"message_id": 3, "from": {"id": 42, "is_bot": false}, "chat": {"id": 42, "type": "private"}, "text": format!("/start {payload}")}}
        ])),
        response(&json!({"message_id": 4})),
    ]);
    let (complete, exit) = run(
        &["pair", "complete"],
        json!({"schema": SCHEMA, "timeout_seconds": 0}),
    );
    assert_eq!(exit, ExitClass::Success, "{complete}");
    let state = state::read(&Paths::from_env().unwrap()).unwrap();
    assert_eq!(
        state.pairing.map(|pair| (pair.user_id, pair.chat_id)),
        Some((42, 42))
    );
    assert_eq!(state.next_offset, 13);
}

#[test]
fn unauthorized_pairing_content_is_erased_but_offset_advances() {
    let mut context = Context::new(vec![
        response(&json!({"id": 7, "is_bot": true, "username": "fake_bot"})),
        response(&json!({"url": ""})),
    ]);
    let (begin, _) = run(&["pair", "begin"], json!({"schema": SCHEMA}));
    let payload = ok(&begin)["deep_link"]
        .as_str()
        .unwrap()
        .rsplit("?start=")
        .next()
        .unwrap()
        .to_string();
    context.replace_fake(vec![response(&json!([{"update_id": 20, "message": {"message_id": 1, "from": {"id": 99, "is_bot": false}, "chat": {"id": 99, "type": "private"}, "text": "secret unauthorized"}}, {"update_id": 21, "message": {"message_id": 2, "from": {"id": 7, "is_bot": true}, "chat": {"id": 7, "type": "private"}, "text": format!("/start {payload}")}}]))]);
    let (complete, exit) = run(
        &["pair", "complete"],
        json!({"schema": SCHEMA, "timeout_seconds": 0}),
    );
    assert_eq!(exit, ExitClass::Policy);
    assert!(!complete.contains("secret unauthorized"));
    let state = state::read(&Paths::from_env().unwrap()).unwrap();
    assert_eq!(state.next_offset, 22);
    assert!(state.events.is_empty());
}

#[test]
fn poll_ack_restart_and_consumer_lock_preserve_order() {
    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    state::update(&paths, |state| {
        state.pairing = Some(Pairing {
            user_id: 42,
            chat_id: 42,
            paired_at: 1,
        });
        state.bot = Some(BotFingerprint {
            id: 7,
            username: "fake_bot".into(),
        });
        Ok(())
    })
    .unwrap();
    context.replace_fake(vec![response(&json!([{"update_id": 20, "message": {"message_id": 3, "from": {"id": 42, "is_bot": false}, "chat": {"id": 42, "type": "private"}, "text": "first"}}, {"update_id": 25, "message": {"message_id": 4, "from": {"id": 99, "is_bot": false}, "chat": {"id": 99, "type": "private"}, "text": "discard"}}]))]);
    let (poll, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Success, "{poll}");
    assert_eq!(ok(&poll)["event"]["text"], "first");
    assert_eq!(state::read(&paths).unwrap().next_offset, 26);
    let (repeat, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Success);
    assert_eq!(ok(&repeat)["event"]["event_id"], "tg:20:3");
    let (ack, exit) = run(&["ack"], json!({"schema": SCHEMA, "event_id": "tg:20:3"}));
    assert_eq!(exit, ExitClass::Success);
    assert_eq!(ok(&ack)["handled"], true);
    context.replace_fake(vec![response(&json!([]))]);
    let (empty, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Success);
    assert!(ok(&empty)["event"].is_null());
    let lock = state::ConsumerLock::acquire(&paths).unwrap();
    let (_, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Conflict);
    drop(lock);
    assert_eq!(context.fake.request_count(), 1);
}

fn paired_state(paths: &Paths) {
    state::update(paths, |state| {
        state.pairing = Some(Pairing {
            user_id: 42,
            chat_id: 42,
            paired_at: 1,
        });
        state.bot = Some(BotFingerprint {
            id: 7,
            username: "fake_bot".into(),
        });
        Ok(())
    })
    .unwrap();
}

fn unhandled_event(paths: &Paths, event_id: &str, update_id: i64) {
    state::update(paths, |state| {
        state.next_offset = update_id + 1;
        state.events.push(EventRecord {
            event_id: event_id.into(),
            update_id,
            kind: "text".into(),
            text: "incoming context".into(),
            received_at: 1,
            handled: false,
            reply_to: None,
            ask_id: None,
            lifecycle_key: None,
            choice: None,
        });
        Ok(())
    })
    .unwrap();
}

fn contract_error(
    code: &str,
    message: &str,
    class: boxology_generated_contract::FailureClass,
    retryable: bool,
    retry_after_seconds: Option<u64>,
) -> boxology_generated_contract::OperationError {
    boxology_generated_contract::OperationError {
        code: code.into(),
        message: message.into(),
        retryable,
        retry_after_seconds,
        class,
    }
}

#[test]
fn typed_send_seam_returns_exact_delivery_and_replay_receipts() {
    let mut context = Context::new(vec![]);
    paired_state(&Paths::from_env().unwrap());
    context.replace_fake(vec![response(&json!({"message_id": 69}))]);
    let command = super::outbound::SendCommand {
        text: "typed notice".into(),
        dedup_key: "typed-notice-1".into(),
    };

    assert_eq!(
        super::outbound::send_typed(command.clone()).unwrap(),
        super::outbound::SendReceipt {
            dedup_key: "typed-notice-1".into(),
            message_id: 69,
            deduplicated: false,
        }
    );
    assert_eq!(
        super::outbound::send_typed(command).unwrap(),
        super::outbound::SendReceipt {
            dedup_key: "typed-notice-1".into(),
            message_id: 69,
            deduplicated: true,
        }
    );
    assert_eq!(context.fake.request_count(), 1);
}

#[test]
fn generated_typed_handle_sends_text_through_the_assembled_box() {
    let context = Context::new(vec![
        response(&json!({"message_id": 573})),
        response(&json!({"message_id": 574})),
    ]);
    paired_state(&Paths::from_env().unwrap());
    let (composition, telegram) = assembled_telegram(&["send_text"]);
    assert_eq!(
        run_ready(telegram.send_text(call_context(), "typed dogfood".into())),
        Ok(573)
    );
    assert_eq!(
        run_ready(telegram.send_text(call_context(), "typed dogfood".into())),
        Ok(574),
        "each handle call must use a fresh internal deduplication key"
    );

    let requests = context.fake.requests.lock().expect("fake requests");
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        let body = request_body(request);
        assert_eq!(body["chat_id"], 42);
        assert_eq!(body["text"], "typed dogfood");
    }
    drop(requests);
    drop(composition);
}

#[test]
fn generated_handles_send_replay_and_structured_ask_end_to_end() {
    let context = Context::new(vec![
        response(&json!({"message_id": 575})),
        response(&json!({"message_id": 576})),
    ]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    let (composition, telegram) = assembled_telegram(&["send", "ask"]);
    let send = boxology_generated_contract::SendRequest {
        text: "typed notice".into(),
        dedup_key: "typed-send-1".into(),
    };

    let first = run_ready(telegram.send(call_context(), send.clone())).unwrap();
    assert_eq!(first.error, None);
    assert_eq!(
        first.delivery,
        Some(boxology_generated_contract::DeliveryReceipt {
            dedup_key: "typed-send-1".into(),
            message_id: 575,
            deduplicated: false,
        })
    );
    let replay = run_ready(telegram.send(call_context(), send)).unwrap();
    assert_eq!(replay.error, None);
    assert!(replay.delivery.unwrap().deduplicated);

    let outcome = run_ready(telegram.ask(
        call_context(),
        boxology_generated_contract::AskRequest {
            summary: "Choose how to proceed with the release.".into(),
            recommendation: "Ship the typed path now.".into(),
            alternatives: Some(vec![boxology_generated_contract::AskAlternative {
                key: "pause".into(),
                label: "Pause".into(),
                text: "Wait for more evidence.".into(),
            }]),
            lifecycle_key: "release-choice".into(),
            dedup_key: "typed-ask-1".into(),
        },
    ))
    .unwrap();
    assert_eq!(outcome.error, None);
    let ask = outcome.ask.expect("successful ask receipt");
    assert_eq!(ask.lifecycle_key, "release-choice");
    assert_eq!(ask.delivery.message_id, 576);
    assert!(!ask.delivery.deduplicated);

    let requests = context.fake.requests.lock().expect("fake requests");
    assert_eq!(requests.len(), 2, "send replay must not write to Telegram");
    let sent = request_body(&requests[0]);
    assert_eq!(sent["chat_id"], 42);
    assert_eq!(sent["text"], "typed notice");
    let asked = request_body(&requests[1]);
    assert_eq!(asked["chat_id"], 42);
    assert!(
        asked["text"]
            .as_str()
            .unwrap()
            .contains("Pause: Wait for more evidence.")
    );
    assert_eq!(
        asked["reply_markup"]["inline_keyboard"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    drop(requests);

    let durable = state::read(&paths).unwrap();
    assert_eq!(durable.outbound.len(), 2);
    assert_eq!(durable.asks.len(), 1);
    assert_eq!(durable.asks[0].ask_id, ask.ask_id);
    assert_eq!(durable.asks[0].lifecycle_key, "release-choice");
    assert_eq!(durable.asks[0].dedup_key, "typed-ask-1");
    assert_eq!(durable.asks[0].message_id, Some(576));
    assert_eq!(durable.asks[0].choices.len(), 3);
    assert_eq!(durable.asks[0].choices[1].key.as_deref(), Some("pause"));
    drop(composition);
}

#[test]
fn generated_reply_correlates_marks_handled_and_replays_without_a_write() {
    let mut context = Context::new(vec![response(&json!({"message_id": 600}))]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    let (composition, telegram) = assembled_telegram(&["ask", "reply"]);
    let ask = run_ready(telegram.ask(
        call_context(),
        boxology_generated_contract::AskRequest {
            summary: "Choose the correlated reply path.".into(),
            recommendation: "Reply through the generated handle.".into(),
            alternatives: None,
            lifecycle_key: "reply-correlation".into(),
            dedup_key: "reply-correlation-ask".into(),
        },
    ))
    .unwrap()
    .ask
    .unwrap();
    context.replace_fake(vec![response(&json!([{"update_id": 41, "message": {
        "message_id": 91, "from": {"id": 42, "is_bot": false},
        "chat": {"id": 42, "type": "private"}, "text": "incoming context",
        "reply_to_message": {"message_id": 600, "chat": {"id": 42, "type": "private"}}
    }}]))]);
    let (polled, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Success, "{polled}");
    assert_eq!(ok(&polled)["event"]["ask_id"], ask.ask_id);
    context.replace_fake(vec![response(&json!({"message_id": 601}))]);
    let request = boxology_generated_contract::ReplyRequest {
        event_id: "tg:41:91".into(),
        text: "typed response".into(),
        dedup_key: "typed-reply-1".into(),
    };

    let first = run_ready(telegram.reply(call_context(), request.clone())).unwrap();
    assert_eq!(first.error, None);
    assert_eq!(
        first.delivery,
        Some(boxology_generated_contract::DeliveryReceipt {
            dedup_key: "typed-reply-1".into(),
            message_id: 601,
            deduplicated: false,
        })
    );
    let replay = run_ready(telegram.reply(call_context(), request)).unwrap();
    assert_eq!(replay.error, None);
    assert!(replay.delivery.unwrap().deduplicated);

    let requests = context.fake.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let body = request_body(&requests[0]);
    assert_eq!(body["reply_parameters"]["message_id"], 91);
    assert_eq!(body["text"], "typed response");
    drop(requests);
    let durable = state::read(&paths).unwrap();
    assert!(durable.events[0].handled);
    assert_eq!(durable.asks[0].state, "answered");
    drop(composition);
}

#[test]
fn generated_reply_projects_safe_policy_and_rate_limit_failures() {
    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    unhandled_event(&paths, "tg:42:92", 42);
    let (composition, telegram) = assembled_telegram(&["reply"]);

    let unknown = run_ready(telegram.reply(
        call_context(),
        boxology_generated_contract::ReplyRequest {
            event_id: "tg:43:93".into(),
            text: "safe response".into(),
            dedup_key: "unknown-reply-1".into(),
        },
    ))
    .unwrap();
    assert_eq!(unknown.delivery, None);
    assert_eq!(
        unknown.error,
        Some(contract_error(
            "unknown_event",
            "event is not available",
            boxology_generated_contract::FailureClass::Policy,
            false,
            None,
        ))
    );

    context.replace_fake(vec![raw(r#"{"ok":false,"error_code":429,"description":"secret rate detail","parameters":{"retry_after":3}}"#)]);
    let limited = run_ready(telegram.reply(
        call_context(),
        boxology_generated_contract::ReplyRequest {
            event_id: "tg:42:92".into(),
            text: "safe response".into(),
            dedup_key: "limited-reply-1".into(),
        },
    ))
    .unwrap();
    assert_eq!(limited.delivery, None);
    assert_eq!(
        limited.error,
        Some(contract_error(
            "telegram_rate_limited",
            "Telegram is temporarily unavailable",
            boxology_generated_contract::FailureClass::Transient,
            true,
            Some(3),
        ))
    );
    assert!(!state::read(&paths).unwrap().events[0].handled);
    assert_eq!(context.fake.request_count(), 1);
    drop(composition);
}

#[test]
fn generated_resolution_preserves_ambiguity_and_both_recovery_paths() {
    let mut context = Context::new(vec![None]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    let (composition, telegram) = assembled_telegram(&["send", "resolve_send"]);
    let first_request = boxology_generated_contract::SendRequest {
        text: "uncertain first delivery".into(),
        dedup_key: "ambiguous-typed-1".into(),
    };

    let first = run_ready(telegram.send(call_context(), first_request.clone())).unwrap();
    assert_eq!(first.delivery, None);
    assert_eq!(
        first.error,
        Some(contract_error(
            "delivery_ambiguous",
            "outbound delivery requires explicit resolution",
            boxology_generated_contract::FailureClass::Ambiguous,
            false,
            None,
        ))
    );
    assert_eq!(
        run_ready(telegram.send(call_context(), first_request.clone()))
            .unwrap()
            .error,
        first.error
    );
    assert_eq!(context.fake.request_count(), 1, "ambiguity must not retry");

    let before_invalid = fs::read(context.root.join("state.json")).unwrap();
    let invalid = run_ready(telegram.resolve_send(
        call_context(),
        boxology_generated_contract::ResolveSendRequest {
            dedup_key: "ambiguous-typed-1".into(),
            resolution: boxology_generated_contract::DeliveryResolution {
                kind: boxology_generated_contract::ResolutionKind::Delivered,
                message_id: None,
            },
        },
    ))
    .unwrap();
    assert_eq!(invalid.resolution, None);
    assert_eq!(
        invalid.error,
        Some(contract_error(
            "invalid_resolution",
            "delivery resolution is invalid",
            boxology_generated_contract::FailureClass::Input,
            false,
            None,
        ))
    );
    assert_eq!(
        fs::read(context.root.join("state.json")).unwrap(),
        before_invalid
    );

    let retryable = run_ready(telegram.resolve_send(
        call_context(),
        boxology_generated_contract::ResolveSendRequest {
            dedup_key: "ambiguous-typed-1".into(),
            resolution: boxology_generated_contract::DeliveryResolution {
                kind: boxology_generated_contract::ResolutionKind::NotDelivered,
                message_id: None,
            },
        },
    ))
    .unwrap();
    assert_eq!(retryable.error, None);
    assert_eq!(
        retryable.resolution,
        Some(boxology_generated_contract::ResolveSendReceipt {
            dedup_key: "ambiguous-typed-1".into(),
            resolved: boxology_generated_contract::ResolutionKind::NotDelivered,
            message_id: None,
        })
    );
    context.replace_fake(vec![response(&json!({"message_id": 602}))]);
    let retried = run_ready(telegram.send(call_context(), first_request)).unwrap();
    assert_eq!(retried.error, None);
    assert_eq!(retried.delivery.unwrap().message_id, 602);

    context.replace_fake(vec![None]);
    let supplied_request = boxology_generated_contract::SendRequest {
        text: "uncertain supplied delivery".into(),
        dedup_key: "ambiguous-typed-2".into(),
    };
    assert!(
        run_ready(telegram.send(call_context(), supplied_request.clone()))
            .unwrap()
            .delivery
            .is_none()
    );
    let supplied = run_ready(telegram.resolve_send(
        call_context(),
        boxology_generated_contract::ResolveSendRequest {
            dedup_key: "ambiguous-typed-2".into(),
            resolution: boxology_generated_contract::DeliveryResolution {
                kind: boxology_generated_contract::ResolutionKind::Delivered,
                message_id: Some(603),
            },
        },
    ))
    .unwrap();
    assert_eq!(supplied.error, None);
    assert_eq!(
        supplied.resolution,
        Some(boxology_generated_contract::ResolveSendReceipt {
            dedup_key: "ambiguous-typed-2".into(),
            resolved: boxology_generated_contract::ResolutionKind::Delivered,
            message_id: Some(603),
        })
    );
    let replay = run_ready(telegram.send(call_context(), supplied_request)).unwrap();
    assert_eq!(replay.delivery.unwrap().message_id, 603);
    assert_eq!(
        context.fake.request_count(),
        1,
        "supplied delivery must not call API"
    );
    let durable = state::read(&paths).unwrap();
    assert_eq!(durable.outbound[0].message_id, Some(602));
    assert_eq!(durable.outbound[1].message_id, Some(603));
    drop(composition);
}

#[test]
fn disabled_generated_commands_return_authorization_without_side_effects() {
    let context = Context::new(vec![]);
    unsafe { std::env::remove_var(ENABLED_VARIABLE) };
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    let before = fs::read(context.root.join("state.json")).unwrap();
    let (composition, telegram) = assembled_telegram(&["send", "ask", "reply", "resolve_send"]);
    let authorization = contract_error(
        "telegram_disabled",
        "Telegram requires BOXOLOGY_TELEGRAM_ENABLED=1",
        boxology_generated_contract::FailureClass::Authorization,
        false,
        None,
    );

    let outcome = run_ready(telegram.send(
        call_context(),
        boxology_generated_contract::SendRequest {
            text: "must not leave the process".into(),
            dedup_key: "disabled-send-1".into(),
        },
    ))
    .unwrap();
    assert_eq!(outcome.delivery, None);
    assert_eq!(outcome.error, Some(authorization.clone()));

    let outcome = run_ready(telegram.ask(
        call_context(),
        boxology_generated_contract::AskRequest {
            summary: "Must not leave the process".into(),
            recommendation: "Keep Telegram disabled".into(),
            alternatives: None,
            lifecycle_key: "disabled-lifecycle".into(),
            dedup_key: "disabled-ask-1".into(),
        },
    ))
    .unwrap();
    assert_eq!(outcome.ask, None);
    assert_eq!(outcome.error, Some(authorization.clone()));

    let reply = run_ready(telegram.reply(
        call_context(),
        boxology_generated_contract::ReplyRequest {
            event_id: "invalid-before-gate".into(),
            text: "".into(),
            dedup_key: "".into(),
        },
    ))
    .unwrap();
    assert_eq!(reply.delivery, None);
    assert_eq!(reply.error, Some(authorization.clone()));

    let resolution = run_ready(telegram.resolve_send(
        call_context(),
        boxology_generated_contract::ResolveSendRequest {
            dedup_key: "".into(),
            resolution: boxology_generated_contract::DeliveryResolution {
                kind: boxology_generated_contract::ResolutionKind::Delivered,
                message_id: None,
            },
        },
    ))
    .unwrap();
    assert_eq!(resolution.resolution, None);
    assert_eq!(resolution.error, Some(authorization));
    assert_eq!(context.fake.request_count(), 0);
    assert_eq!(fs::read(context.root.join("state.json")).unwrap(), before);
    drop(composition);
}

#[test]
fn send_deduplication_never_retries_ambiguous_delivery() {
    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    context.replace_fake(vec![response(&json!({"message_id": 70}))]);
    let request = json!({"schema": SCHEMA, "text": "notice", "dedup_key": "notice-1"});
    let (sent, exit) = run(&["send"], request.clone());
    assert_eq!(exit, ExitClass::Success, "{sent}");
    assert_eq!(
        sent,
        r#"{"schema":1,"ok":true,"command":"send","data":{"dedup_key":"notice-1","deduplicated":false,"delivery":"delivered","message_id":70}}"#
    );
    let (repeat, exit) = run(&["send"], request);
    assert_eq!(exit, ExitClass::Success);
    assert_eq!(
        repeat,
        r#"{"schema":1,"ok":true,"command":"send","data":{"dedup_key":"notice-1","deduplicated":true,"delivery":"delivered","message_id":70}}"#
    );
    assert_eq!(context.fake.request_count(), 1);
    drop(context);

    let mut ambiguous = Context::new(vec![]);
    paired_state(&Paths::from_env().unwrap());
    ambiguous.replace_fake(vec![None]);
    let request = json!({"schema": SCHEMA, "text": "uncertain", "dedup_key": "notice-2"});
    let (_, exit) = run(&["send"], request.clone());
    assert_eq!(exit, ExitClass::Ambiguous);
    let (_, exit) = run(&["send"], request.clone());
    assert_eq!(exit, ExitClass::Ambiguous);
    let (resolved, exit) = run(
        &["resolve-send"],
        json!({"schema": SCHEMA, "dedup_key": "notice-2", "resolution": {"kind": "not_delivered"}}),
    );
    assert_eq!(exit, ExitClass::Success, "{resolved}");
    ambiguous.replace_fake(vec![response(&json!({"message_id": 71}))]);
    let (retried, exit) = run(&["send"], request);
    assert_eq!(exit, ExitClass::Success, "{retried}");
    assert_eq!(ok(&retried)["message_id"], 71);
}

#[test]
fn ask_callbacks_and_custom_replies_are_correlated() {
    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    context.replace_fake(vec![response(&json!({"message_id": 80}))]);
    let ask_request = json!({"schema": SCHEMA, "summary": "The build is blocked by a policy choice. I recommend the native option. Please choose or request context. Delay keeps the implementation paused.", "recommendation": "Keep the compliant native option.", "alternatives": [{"key": "pause", "label": "Pause", "text": "Pause this slice."}], "lifecycle_key": "build-policy", "dedup_key": "ask-1"});
    let (ask, exit) = run(&["ask"], ask_request);
    assert_eq!(exit, ExitClass::Success, "{ask}");
    let ask_id = ok(&ask)["ask_id"].as_str().unwrap().to_string();
    assert_eq!(
        ask,
        format!(
            r#"{{"schema":1,"ok":true,"command":"ask","data":{{"ask_id":"{ask_id}","dedup_key":"ask-1","deduplicated":false,"delivery":"delivered","lifecycle_key":"build-policy","message_id":80}}}}"#
        )
    );
    let callback_data = super::ask::token(&ask_id, "recommendation", None);
    context.replace_fake(vec![response(&json!([{"update_id": 31, "callback_query": {"id": "callback-1", "from": {"id": 42, "is_bot": false}, "message": {"message_id": 80, "chat": {"id": 42, "type": "private"}}, "data": callback_data}}])), response(&json!(true))]);
    let (choice, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Success, "{choice}");
    assert_eq!(ok(&choice)["event"]["kind"], "ask_choice");
    assert_eq!(ok(&choice)["event"]["choice"]["kind"], "recommendation");
    assert!(!choice.contains(&callback_data));
    let (ack, exit) = run(&["ack"], json!({"schema": SCHEMA, "event_id": "tg:31:80"}));
    assert_eq!(exit, ExitClass::Success, "{ack}");
    assert_eq!(state::read(&paths).unwrap().asks[0].state, "answered");

    context.replace_fake(vec![response(&json!({"message_id": 81}))]);
    let ask_request = json!({"schema": SCHEMA, "summary": "A second decision needs your response now. I recommend continuing. Please reply with a choice or context. Delay pauses delivery.", "recommendation": "Continue.", "lifecycle_key": "reply-policy", "dedup_key": "ask-2"});
    let (ask, exit) = run(&["ask"], ask_request);
    assert_eq!(exit, ExitClass::Success, "{ask}");
    let ask_id = ok(&ask)["ask_id"].as_str().unwrap().to_string();
    context.replace_fake(vec![response(&json!([{"update_id": 32, "message": {"message_id": 90, "from": {"id": 42, "is_bot": false}, "chat": {"id": 42, "type": "private"}, "text": "custom context", "reply_to_message": {"message_id": 81, "chat": {"id": 42, "type": "private"}}}}]))]);
    let (reply_event, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Success, "{reply_event}");
    assert_eq!(ok(&reply_event)["event"]["kind"], "ask_reply");
    assert_eq!(ok(&reply_event)["event"]["ask_id"], ask_id);
    context.replace_fake(vec![response(&json!({"message_id": 91}))]);
    let (reply, exit) = run(
        &["reply"],
        json!({"schema": SCHEMA, "event_id": "tg:32:90", "text": "Thanks, continuing.", "dedup_key": "reply-2"}),
    );
    assert_eq!(exit, ExitClass::Success, "{reply}");
    assert_eq!(ok(&reply)["message_id"], 91);
    assert!(
        state::read(&paths)
            .unwrap()
            .events
            .iter()
            .find(|event| event.event_id == "tg:32:90")
            .unwrap()
            .handled
    );
}

#[test]
fn status_probe_is_constrained_and_listener_requires_lease() {
    let mut context = Context::new(vec![
        response(&json!({"id": 7, "is_bot": true, "username": "fake_bot"})),
        response(&json!({"url": ""})),
    ]);
    let (status, exit) = run(&["status"], json!({"schema": SCHEMA, "probe": true}));
    assert_eq!(exit, ExitClass::Success, "{status}");
    assert_eq!(ok(&status)["api_reachable"], true);
    assert_eq!(ok(&status)["get_updates_compatible"], true);
    assert_eq!(context.fake.request_count(), 2);
    assert!(!status.contains("fake-token"));
    unsafe {
        std::env::remove_var(ENABLED_VARIABLE);
    }
    let mut output = Vec::new();
    let exit = super::listen::run(
        br#"{"schema":1,"long_poll_seconds":1,"heartbeat_seconds":10}"#,
        &mut output,
    );
    assert_eq!(exit, ExitClass::Authorization);
    assert!(!String::from_utf8_lossy(&output).contains("fake-token"));
    context.replace_fake(vec![]);
}

#[test]
fn rate_limits_are_retryable_and_errors_are_redacted() {
    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    context.replace_fake(vec![raw(r#"{"ok":false,"error_code":429,"description":"secret rate detail","parameters":{"retry_after":3}}"#)]);
    let request = json!({"schema": SCHEMA, "text": "rate limited", "dedup_key": "rate-1"});
    let (rate, exit) = run(&["send"], request.clone());
    assert_eq!(exit, ExitClass::Transient, "{rate}");
    assert_eq!(
        serde_json::from_str::<Value>(&rate).unwrap()["error"]["retry_after_seconds"],
        3
    );
    assert!(!rate.contains("secret rate detail"));
    context.replace_fake(vec![raw(
        r#"{"ok":false,"error_code":403,"description":"secret permanent detail"}"#,
    )]);
    let (permanent, exit) = run(&["send"], request);
    assert_eq!(exit, ExitClass::Permanent);
    assert!(!permanent.contains("secret permanent detail"));
}

#[test]
fn malformed_receive_does_not_advance_offset_and_webhooks_are_refused() {
    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    context.replace_fake(vec![raw("not-json")]);
    let (_poll, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Transient);
    assert_eq!(state::read(&paths).unwrap().next_offset, 0);
    drop(context);

    let context = Context::new(vec![
        response(&json!({"id": 7, "is_bot": true, "username": "fake_bot"})),
        response(&json!({"url": "https://example.invalid/hook"})),
    ]);
    let (pair, exit) = run(&["pair", "begin"], json!({"schema": SCHEMA}));
    assert_eq!(exit, ExitClass::Conflict);
    assert!(!pair.contains("example.invalid"));
    drop(context);
}

#[test]
fn stale_update_offsets_cannot_regress_or_replay_state() {
    let mut context = Context::new(vec![
        response(&json!({"id": 7, "is_bot": true, "username": "fake_bot"})),
        response(&json!({"url": ""})),
    ]);
    let (begin, exit) = run(&["pair", "begin"], json!({"schema": SCHEMA}));
    assert_eq!(exit, ExitClass::Success, "{begin}");
    let payload = ok(&begin)["deep_link"]
        .as_str()
        .unwrap()
        .rsplit("?start=")
        .next()
        .unwrap()
        .to_string();
    let paths = Paths::from_env().unwrap();
    state::update(&paths, |state| {
        state.next_offset = 10;
        Ok(())
    })
    .unwrap();
    context.replace_fake(vec![response(&json!([{
        "update_id": 9,
        "message": {"message_id": 1, "from": {"id": 42, "is_bot": false}, "chat": {"id": 42, "type": "private"}, "text": format!("/start {payload}")}
    }]))]);
    let (_, exit) = run(
        &["pair", "complete"],
        json!({"schema": SCHEMA, "timeout_seconds": 0}),
    );
    assert_eq!(exit, ExitClass::Transient);
    let state = state::read(&paths).unwrap();
    assert_eq!(state.next_offset, 10);
    assert!(state.pairing.is_none());
    assert!(state.pending_pair.is_some());
    drop(context);

    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    state::update(&paths, |state| {
        state.next_offset = 10;
        Ok(())
    })
    .unwrap();
    context.replace_fake(vec![response(&json!([{
        "update_id": 9,
        "message": {"message_id": 2, "from": {"id": 42, "is_bot": false}, "chat": {"id": 42, "type": "private"}, "text": "replayed"}
    }]))]);
    let (output, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Transient);
    assert!(!output.contains("replayed"));
    let state = state::read(&paths).unwrap();
    assert_eq!(state.next_offset, 10);
    assert!(state.events.is_empty());
}

#[test]
fn listener_emits_startup_error_and_stopped_without_leaking_token() {
    let context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    context.fake.requests.lock().unwrap().clear();
    let mut context = context;
    context.replace_fake(vec![raw(
        r#"{"ok":false,"error_code":403,"description":"secret listener detail"}"#,
    )]);
    let mut output = Vec::new();
    let exit = super::listen::run(
        br#"{"schema":1,"long_poll_seconds":1,"heartbeat_seconds":10}"#,
        &mut output,
    );
    assert_eq!(exit, ExitClass::Permanent);
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("\"kind\":\"startup\""));
    assert!(text.contains("\"reason\":\"fatal_error\""));
    assert!(!text.contains("secret listener detail"));
    assert!(!text.contains("fake-token"));
}

#[test]
fn strict_input_and_private_token_file_checks_fail_closed() {
    let context = Context::new(vec![]);
    unsafe {
        std::env::remove_var(ENABLED_VARIABLE);
        std::env::remove_var("BOXOLOGY_TELEGRAM_BOT_TOKEN");
    }
    let duplicate = super::execute(
        &["status".into()],
        br#"{"schema":1,"schema":1,"probe":false}"#,
    );
    assert_eq!(duplicate.1, ExitClass::Input);
    let oversized = vec![b' '; 65_537];
    let (_, exit) = super::execute(&["status".into()], &oversized);
    assert_eq!(exit, ExitClass::Input);
    let token_path = context.root.join("token");
    fs::write(&token_path, b"999:file-token\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    unsafe {
        std::env::set_var("BOXOLOGY_TELEGRAM_BOT_TOKEN_FILE", &token_path);
    }
    assert_eq!(api::load_token().unwrap(), "999:file-token");
    unsafe {
        std::env::set_var("BOXOLOGY_TELEGRAM_BOT_TOKEN", "999:other");
    }
    assert_eq!(api::load_token().unwrap_err().code, "token_sources");
    unsafe {
        std::env::remove_var("BOXOLOGY_TELEGRAM_BOT_TOKEN_FILE");
    }
    drop(context);
}

#[cfg(unix)]
#[test]
fn protected_state_and_token_paths_reject_symlink_components() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let context = Context::new(vec![]);
    let real_state = context.root.join("real-state");
    fs::create_dir(&real_state).unwrap();
    fs::set_permissions(&real_state, fs::Permissions::from_mode(0o700)).unwrap();
    let state_alias = context.root.join("state-alias");
    symlink(&real_state, &state_alias).unwrap();
    unsafe {
        std::env::set_var(
            "BOXOLOGY_TELEGRAM_HOME",
            state_alias.join("telegram-coordinator"),
        );
    }
    let paths = Paths::from_env().unwrap();
    assert_eq!(state::read(&paths).unwrap_err().code, "unsafe_state_home");

    let real_token_dir = context.root.join("real-token-dir");
    fs::create_dir(&real_token_dir).unwrap();
    let real_token = real_token_dir.join("token");
    fs::write(&real_token, b"999:file-token\n").unwrap();
    fs::set_permissions(&real_token, fs::Permissions::from_mode(0o600)).unwrap();
    let token_alias = context.root.join("token-alias");
    symlink(&real_token_dir, &token_alias).unwrap();
    unsafe {
        std::env::set_var(
            "BOXOLOGY_TELEGRAM_BOT_TOKEN_FILE",
            token_alias.join("token"),
        );
        std::env::remove_var("BOXOLOGY_TELEGRAM_BOT_TOKEN");
    }
    assert_eq!(api::load_token().unwrap_err().code, "unsafe_token_file");

    let final_alias = context.root.join("token-final-alias");
    symlink(&real_token, &final_alias).unwrap();
    unsafe {
        std::env::set_var("BOXOLOGY_TELEGRAM_BOT_TOKEN_FILE", &final_alias);
    }
    assert!(api::load_token().is_err());
}

#[test]
fn corrupt_state_relationships_and_enums_fail_closed() {
    let context = Context::new(vec![]);
    let duplicate_event = json!({
        "event_id": "tg:1:1",
        "update_id": 1,
        "kind": "text",
        "text": "message",
        "received_at": 1,
        "handled": false
    });
    let cases = vec![
        json!({"schema": SCHEMA, "next_offset": 1, "confirmed_before": 2}),
        json!({"schema": SCHEMA, "next_offset": 2, "events": [{"event_id": "tg:1:1", "update_id": 1, "kind": "unknown", "text": "", "received_at": 1, "handled": false}]}),
        json!({"schema": SCHEMA, "next_offset": 2, "events": [duplicate_event.clone(), duplicate_event]}),
        json!({"schema": SCHEMA, "asks": [{"ask_id": format!("ask:{}", "0".repeat(32)), "lifecycle_key": "life", "dedup_key": "dedup", "message_id": null, "state": "closed", "choices": []}]}),
        json!({"schema": SCHEMA, "outbound": [{"dedup_key": "dedup", "kind": "reply", "payload_hash": "0".repeat(64), "state": "in_flight", "message_id": null, "event_id": "tg:1:1", "ask_id": null}]}),
    ];
    for value in cases {
        let path = context.root.join("state.json");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert_eq!(
            state::read(&Paths::from_env().unwrap()).unwrap_err().code,
            "corrupt_state"
        );
    }
}

#[test]
fn pairing_revocation_is_explicit_and_local() {
    let context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    state::update(&paths, |state| {
        state.next_offset = 2;
        state.events.push(EventRecord {
            event_id: "tg:1:1".into(),
            update_id: 1,
            kind: "text".into(),
            text: "private".into(),
            received_at: 1,
            handled: false,
            reply_to: None,
            ask_id: None,
            lifecycle_key: None,
            choice: None,
        });
        Ok(())
    })
    .unwrap();
    let (revoked, exit) = run(&["pair", "revoke"], json!({"schema": SCHEMA}));
    assert_eq!(exit, ExitClass::Success, "{revoked}");
    let state = state::read(&paths).unwrap();
    assert!(state.pairing.is_none());
    assert!(state.events.is_empty());
    assert_eq!(context.fake.request_count(), 0);
}

#[test]
fn remote_polling_conflict_is_not_retried_as_a_write() {
    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    context.replace_fake(vec![raw(
        r#"{"ok":false,"error_code":409,"description":"secret conflict"}"#,
    )]);
    let (output, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Conflict);
    assert!(!output.contains("secret conflict"));
    assert_eq!(context.fake.request_count(), 1);
}

#[test]
fn acknowledged_full_inbox_is_pruned_before_fetching_more() {
    let mut context = Context::new(vec![]);
    let paths = Paths::from_env().unwrap();
    paired_state(&paths);
    state::update(&paths, |state| {
        state.next_offset = 1_000;
        state.events = (0..1_000)
            .map(|index| EventRecord {
                event_id: format!("tg:{index}:1"),
                update_id: index,
                kind: "text".into(),
                text: "done".into(),
                received_at: 1,
                handled: true,
                reply_to: None,
                ask_id: None,
                lifecycle_key: None,
                choice: None,
            })
            .collect();
        Ok(())
    })
    .unwrap();
    context.replace_fake(vec![response(&json!([]))]);
    let (output, exit) = run(&["poll"], json!({"schema": SCHEMA, "timeout_seconds": 0}));
    assert_eq!(exit, ExitClass::Success, "{output}");
    assert!(ok(&output)["event"].is_null());
    assert!(state::read(&paths).unwrap().events.len() < 1_000);
}
