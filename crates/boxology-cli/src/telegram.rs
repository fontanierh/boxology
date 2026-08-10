use super::{ExposureTarget, LocalBinding, context, ready};
use boxology_contract::{BoxId, CallError, ExposureLevel};
use boxology_runtime::{Composition, CompositionBuilder};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    future::Future,
    io::Write,
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use telegram_contract as contract;
use telegram_implementation::cli::Backend as _;
use telegram_implementation::{AppError, ENABLED_VARIABLE, ExitClass, TelegramService, cli};

const MAX_INPUT: usize = 65_536;

/// Live local Telegram box assembled behind its generated typed handle.
pub struct TelegramComposition {
    _composition: Composition,
    backend: HandleBackend,
}

impl TelegramComposition {
    /// Assembles every Telegram capability behind one generated handle.
    pub fn start() -> Result<Self, String> {
        let descriptor = telegram_implementation::generated::implementation_descriptor();
        let capabilities = descriptor.contract().capabilities();
        let binding = Arc::new(LocalBinding::default());
        let mut builder = CompositionBuilder::new();
        builder.add_box(descriptor, |imports| {
            telegram_implementation::generated::factory(TelegramService::default(), imports)
        });
        for capability in capabilities {
            builder.expose(
                BoxId::new("telegram").expect("Telegram box id is valid"),
                capability.id().clone(),
                binding.clone(),
                ExposureLevel::CodeOnly,
            );
        }
        let composition = builder.start().map_err(|error| error.to_string())?;
        let runtime = binding
            .runtime()
            .ok_or_else(|| "Telegram in-process binding did not start".to_owned())?;
        if runtime.exposures().len() != capabilities.len() {
            return Err("Telegram composition did not expose every capability".into());
        }
        let handle = contract::TelegramHandle::from_erased(Arc::new(ExposureTarget(
            runtime.exposures().to_vec(),
        )));
        Ok(Self {
            _composition: composition,
            backend: HandleBackend(handle),
        })
    }

    fn execute(&self, enabled: bool, args: &[String], input: &[u8]) -> (String, ExitClass) {
        cli::execute(&self.backend, enabled, args, input)
    }
}

struct HandleBackend(contract::TelegramHandle);

fn invoke<T>(
    future: impl Future<Output = Result<T, CallError<contract::SendTextError>>>,
) -> Result<T, AppError> {
    match ready(future) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) | Err(_) => Err(AppError::invariant()),
    }
}

macro_rules! backend {
    ($($method:ident($request:ty) -> $outcome:ty;)*) => {
        impl cli::Backend for HandleBackend {$ (
            fn $method(&self, request: $request) -> Result<$outcome, AppError> {
                invoke(self.0.$method(context(), request))
            }
        )*}
    };
}

backend! {
    send(contract::SendRequest) -> contract::DeliveryOutcome;
    ask(contract::AskRequest) -> contract::AskOutcome;
    reply(contract::ReplyRequest) -> contract::DeliveryOutcome;
    resolve_send(contract::ResolveSendRequest) -> contract::ResolveSendOutcome;
    pair_begin(contract::PairBeginRequest) -> contract::PairBeginOutcome;
    pair_complete(contract::PairCompleteRequest) -> contract::PairCompleteOutcome;
    pair_revoke(contract::PairRevokeRequest) -> contract::PairRevokeOutcome;
    poll(contract::PollRequest) -> contract::PollOutcome;
    ack(contract::AckRequest) -> contract::AckOutcome;
    status(contract::StatusRequest) -> contract::StatusOutcome;
    listen_start(contract::ListenStartRequest) -> contract::ListenStartOutcome;
}

/// Runs the installed Telegram binding and writes its line-oriented JSON output.
pub fn run_telegram(args: &[String], input: &[u8], output: &mut dyn Write) -> ExitClass {
    let enabled = std::env::var(ENABLED_VARIABLE).is_ok_and(|value| value == "1");
    let composition = match TelegramComposition::start() {
        Ok(composition) => composition,
        Err(_) => return emit_app_error(output, AppError::invariant()),
    };
    if args == ["listen"] {
        listen(
            &composition.backend,
            enabled,
            input,
            output,
            &SystemLoop::new(),
        )
    } else {
        let (line, exit) = composition.execute(enabled, args, input);
        if writeln!(output, "{line}")
            .and_then(|_| output.flush())
            .is_err()
        {
            ExitClass::Local
        } else {
            exit
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListenRequest {
    schema: u8,
    long_poll_seconds: Option<u64>,
    heartbeat_seconds: Option<u64>,
}

trait LoopRuntime {
    fn elapsed(&self) -> Duration;
    fn unix_time(&self) -> i64;
    fn sleep(&self, duration: Duration);
    fn stopped(&self) -> bool;
}

struct SystemLoop {
    started: Instant,
    stopped: Arc<std::sync::atomic::AtomicBool>,
}

impl SystemLoop {
    fn new() -> Self {
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            let _ = signal_hook::flag::register(signal, Arc::clone(&stopped));
        }
        Self {
            started: Instant::now(),
            stopped,
        }
    }
}

impl LoopRuntime for SystemLoop {
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
    fn unix_time(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_secs() as i64)
    }
    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
}

fn listen(
    backend: &HandleBackend,
    enabled: bool,
    input: &[u8],
    output: &mut dyn Write,
    runtime: &impl LoopRuntime,
) -> ExitClass {
    if !enabled {
        return emit_app_error(output, AppError::authorization());
    }
    let request: ListenRequest = match parse(input) {
        Ok(request) => request,
        Err(error) => return emit_app_error(output, error),
    };
    if request.schema != 1 {
        return emit_app_error(
            output,
            AppError::input("unsupported_schema", "unsupported schema"),
        );
    }
    let long_poll = request.long_poll_seconds.unwrap_or(30);
    let heartbeat = request.heartbeat_seconds.unwrap_or(60);
    if !(1..=50).contains(&long_poll) || !(10..=300).contains(&heartbeat) {
        return emit_app_error(
            output,
            AppError::input("invalid_listen_limits", "listen limits are out of bounds"),
        );
    }
    let started = match backend.listen_start(contract::ListenStartRequest {}) {
        Ok(outcome) => match (outcome.startup, outcome.error) {
            (Some(startup), None) => startup,
            (None, Some(error)) => return fatal_operation(output, error),
            _ => return fatal_app(output, AppError::invariant()),
        },
        Err(error) => return fatal_app(output, error),
    };
    if emit(output, json!({"kind":"startup","paired":true,"next_offset":started.next_offset,"unhandled":started.unhandled})).is_err() {
        return ExitClass::Local;
    }
    let mut emitted = BTreeSet::new();
    let mut last_heartbeat = runtime.elapsed();
    let mut backoff = 1_u64;
    loop {
        if runtime.stopped() {
            return stop(output, "signal");
        }
        let polled = match backend.poll(contract::PollRequest {
            timeout_seconds: Some(long_poll),
        }) {
            Ok(outcome) => outcome,
            Err(error) => return fatal_app(output, error),
        };
        match (polled.result, polled.error) {
            (Some(result), None) => {
                backoff = 1;
                if result.receipt.callback_receipt_failed && emit(output, json!({"kind":"warning","code":"callback_receipt_failed","message":["callback was stored but its Telegram UI receipt failed"]})).is_err() { return ExitClass::Local; }
                if let Some(event) = result.event {
                    let event_id = event.event_id.clone();
                    if emitted.insert(event_id) {
                        let event = match cli::event_value(event) {
                            Ok(event) => event,
                            Err(()) => return fatal_app(output, AppError::invariant()),
                        };
                        if emit(output, json!({"kind":"event","event":event})).is_err() {
                            return ExitClass::Local;
                        }
                    }
                    if runtime.elapsed().saturating_sub(last_heartbeat)
                        < Duration::from_secs(heartbeat)
                    {
                        runtime.sleep(Duration::from_millis(250));
                    }
                }
            }
            (None, Some(error)) if matches!(error.class, contract::FailureClass::Transient) => {
                if emit(output, json!({"kind":"warning","code":error.code,"message":"Telegram receive is temporarily unavailable","retryable":true})).is_err() { return ExitClass::Local; }
                let wait = error.retry_after_seconds.unwrap_or(backoff).clamp(1, 30);
                runtime.sleep(Duration::from_secs(wait));
                backoff = backoff.saturating_mul(2).min(30);
            }
            (None, Some(error)) if error.code == "inbox_full" => {
                if emit(output, json!({"kind":"warning","code":"inbox_full","message":"inbound storage is full","retryable":true})).is_err() { return ExitClass::Local; }
                runtime.sleep(Duration::from_secs(1));
            }
            (None, Some(error)) => return fatal_operation(output, error),
            _ => return fatal_app(output, AppError::invariant()),
        }
        if runtime.elapsed().saturating_sub(last_heartbeat) >= Duration::from_secs(heartbeat) {
            let status = match backend.status(contract::StatusRequest { probe: false }) {
                Ok(outcome) => match (outcome.status, outcome.error) {
                    (Some(status), None) => status,
                    (None, Some(error)) => return fatal_operation(output, error),
                    _ => return fatal_app(output, AppError::invariant()),
                },
                Err(error) => return fatal_app(output, error),
            };
            let Some(local) = status.local.filter(|_| status.probe.is_none()) else {
                return fatal_app(output, AppError::invariant());
            };
            if emit(output, json!({"kind":"heartbeat","at":runtime.unix_time(),"unhandled":local.inbox.unhandled,"inbox_full":local.inbox.full})).is_err() { return ExitClass::Local; }
            last_heartbeat = runtime.elapsed();
        }
    }
}

fn parse<T: for<'de> Deserialize<'de>>(input: &[u8]) -> Result<T, AppError> {
    if input.len() > MAX_INPUT {
        return Err(AppError::input(
            "input_too_large",
            "request exceeds input limit",
        ));
    }
    serde_json::from_slice(input)
        .map_err(|_| AppError::input("invalid_json", "request must be one valid JSON object"))
}

fn emit(output: &mut dyn Write, data: Value) -> std::io::Result<()> {
    serde_json::to_writer(
        &mut *output,
        &json!({"schema":1,"ok":true,"command":"listen","data":data}),
    )?;
    output.write_all(b"\n")?;
    output.flush()
}

fn emit_app_error(output: &mut dyn Write, error: AppError) -> ExitClass {
    emit_error(
        output,
        error.code,
        error.message,
        error.retryable,
        error.retry_after,
        error.exit,
    )
}

fn emit_error(
    output: &mut dyn Write,
    code: &str,
    message: &str,
    retryable: bool,
    retry_after: Option<u64>,
    exit: ExitClass,
) -> ExitClass {
    let mut body = json!({"code":code,"message":message,"retryable":retryable});
    if let Some(after) = retry_after {
        body["retry_after_seconds"] = json!(after);
    }
    let _ = serde_json::to_writer(
        &mut *output,
        &json!({"schema":1,"ok":false,"command":"listen","error":body}),
    );
    let _ = output.write_all(b"\n");
    let _ = output.flush();
    exit
}

fn fatal_operation(output: &mut dyn Write, error: contract::OperationError) -> ExitClass {
    let (line, exit) = cli::operation_failure("listen", error);
    let _ = writeln!(output, "{line}");
    let _ = output.flush();
    let _ = emit(output, json!({"kind":"stopped","reason":"fatal_error"}));
    exit
}

fn fatal_app(output: &mut dyn Write, error: AppError) -> ExitClass {
    let exit = emit_app_error(output, error);
    let _ = emit(output, json!({"kind":"stopped","reason":"fatal_error"}));
    exit
}

fn stop(output: &mut dyn Write, reason: &str) -> ExitClass {
    let _ = emit(output, json!({"kind":"stopped","reason":reason}));
    ExitClass::Success
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    use contract::test_support::TelegramFake;
    use std::sync::{Mutex, atomic::{AtomicU16, AtomicUsize}};

    macro_rules! responder {
        ($seen:ident, $bit:expr, |$request:ident| $value:expr) => {{
            let seen = Arc::clone(&$seen);
            move |_context, $request| {
                seen.fetch_or($bit, Ordering::Relaxed);
                let value = $value;
                async move { Ok(value) }
            }
        }};
    }

    fn operation(code: &str, class: contract::FailureClass) -> contract::OperationError {
        contract::OperationError { code: code.into(), message: "safe".into(), retryable: matches!(class, contract::FailureClass::Transient), retry_after_seconds: None, class }
    }

    #[test]
    fn every_one_shot_command_crosses_its_generated_handle() {
        let seen = Arc::new(AtomicU16::new(0));
        let fake = TelegramFake::new()
            .with_send(responder!(seen, 1, |r| { assert_eq!((r.text.as_str(), r.dedup_key.as_str()), ("hi", "s")); contract::DeliveryOutcome { delivery: Some(contract::DeliveryReceipt { dedup_key: r.dedup_key, message_id: 4, deduplicated: false }), error: None } }))
            .with_ask(responder!(seen, 2, |r| { assert_eq!((r.summary.as_str(), r.recommendation.as_str(), r.lifecycle_key.as_str(), r.dedup_key.as_str()), ("sum", "rec", "life", "a")); assert_eq!(r.alternatives.as_ref().unwrap()[0].key, "one"); contract::AskOutcome { ask: Some(contract::AskReceipt { ask_id: "ask".into(), lifecycle_key: r.lifecycle_key, delivery: contract::DeliveryReceipt { dedup_key: r.dedup_key, message_id: 5, deduplicated: false } }), error: None } }))
            .with_reply(responder!(seen, 4, |r| { assert_eq!((r.event_id.as_str(), r.text.as_str(), r.dedup_key.as_str()), ("e", "reply", "r")); contract::DeliveryOutcome { delivery: Some(contract::DeliveryReceipt { dedup_key: r.dedup_key, message_id: 6, deduplicated: true }), error: None } }))
            .with_resolve_send(responder!(seen, 8, |r| { assert!(matches!(r.resolution.kind, contract::ResolutionKind::Delivered)); assert_eq!(r.resolution.message_id, Some(6)); contract::ResolveSendOutcome { resolution: Some(contract::ResolveSendReceipt { dedup_key: r.dedup_key, resolved: contract::ResolutionKind::Delivered, message_id: Some(6) }), error: None } }))
            .with_pair_begin(responder!(seen, 16, |r| { assert_eq!(r.nonce_ttl_seconds, Some(40)); contract::PairBeginOutcome { pairing: Some(contract::PairBeginReceipt { deep_link: "link".into(), expires_at: 40, bot: contract::TelegramBotIdentity { id: 1, username: "bot".into() } }), error: None } }))
            .with_pair_complete(responder!(seen, 32, |r| { assert_eq!(r.timeout_seconds, Some(7)); contract::PairCompleteOutcome { pairing: Some(contract::PairCompleteReceipt { user_id: 1, chat_id: 1, paired_at: 7, confirmation: contract::PairConfirmation::Delivered }), error: None } }))
            .with_pair_revoke(responder!(seen, 64, |_r| contract::PairRevokeOutcome { revocation: Some(contract::PairRevokeReceipt { pairing_revoked: true }), error: None }))
            .with_poll(responder!(seen, 128, |r| { assert_eq!(r.timeout_seconds, Some(9)); contract::PollOutcome { result: Some(contract::PollResult { event: None, receipt: receipt(false) }), error: None } }))
            .with_ack(responder!(seen, 256, |r| { assert_eq!(r.event_id, "e"); contract::AckOutcome { acknowledgement: Some(contract::AckReceipt { event_id: r.event_id, handled: true, already_handled: false }), error: None } }))
            .with_status(responder!(seen, 512, |r| { assert!(!r.probe); contract::StatusOutcome { status: Some(local_status()), error: None } }));
        let backend = HandleBackend(fake.handle());
        let cases = [
            (vec!["send"], json!({"schema":1,"text":"hi","dedup_key":"s"})),
            (vec!["ask"], json!({"schema":1,"summary":"sum","recommendation":"rec","alternatives":[{"key":"one","label":"One","text":"text"}],"lifecycle_key":"life","dedup_key":"a"})),
            (vec!["reply"], json!({"schema":1,"event_id":"e","text":"reply","dedup_key":"r"})),
            (vec!["resolve-send"], json!({"schema":1,"dedup_key":"s","resolution":{"kind":"delivered","message_id":6}})),
            (vec!["pair","begin"], json!({"schema":1,"nonce_ttl_seconds":40})),
            (vec!["pair","complete"], json!({"schema":1,"timeout_seconds":7})),
            (vec!["pair","revoke"], json!({"schema":1})),
            (vec!["poll"], json!({"schema":1,"timeout_seconds":9})),
            (vec!["ack"], json!({"schema":1,"event_id":"e"})),
            (vec!["status"], json!({"schema":1,"probe":false})),
        ];
        for (args, input) in cases {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            let (line, exit) = cli::execute(&backend, true, &args, &serde_json::to_vec(&input).unwrap());
            assert_eq!(exit, ExitClass::Success, "{line}");
            let envelope: Value = serde_json::from_str(&line).unwrap();
            assert_eq!((envelope["schema"].as_u64(), envelope["ok"].as_bool(), envelope["command"].as_str()), (Some(1), Some(true), Some(args[0].as_str())));
        }
        assert_eq!(seen.load(Ordering::Relaxed), 1023);
        let before = seen.load(Ordering::Relaxed);
        assert_eq!(cli::execute(&backend, false, &["send".into()], b"not json").1, ExitClass::Authorization);
        assert_eq!(cli::execute(&backend, true, &["send".into()], &vec![b' '; MAX_INPUT + 1]).1, ExitClass::Input);
        assert_eq!(seen.load(Ordering::Relaxed), before);
    }

    #[test]
    fn listener_preserves_order_retries_heartbeat_and_redaction() {
        let poll = Arc::new(AtomicUsize::new(0));
        let poll_copy = Arc::clone(&poll);
        let fake = TelegramFake::new()
            .with_listen_start(|_, _| async { Ok(contract::ListenStartOutcome { startup: Some(contract::ListenStartReceipt { next_offset: 3, unhandled: 1 }), error: None }) })
            .with_poll(move |_, request| { assert_eq!(request.timeout_seconds, Some(2)); let call = poll_copy.fetch_add(1, Ordering::Relaxed); async move { Ok(match call {
                0 | 1 => contract::PollOutcome { result: Some(contract::PollResult { event: Some(text_event()), receipt: contract::PollReceipt { callback_receipt_failed: call == 0, ..receipt(true) } }), error: None },
                2 => contract::PollOutcome { result: None, error: Some(contract::OperationError { message: "secret transport detail".into(), retry_after_seconds: Some(2), ..operation("temporary", contract::FailureClass::Transient) }) },
                3 => contract::PollOutcome { result: None, error: Some(operation("inbox_full", contract::FailureClass::Local)) },
                _ => contract::PollOutcome { result: None, error: Some(operation("fatal_safe", contract::FailureClass::Permanent)) },
            }) } })
            .with_status(|_, request| { assert!(!request.probe); async { Ok(contract::StatusOutcome { status: Some(local_status()), error: None }) } });
        let runtime = TestLoop::default();
        let mut output = Vec::new();
        let exit = listen(&HandleBackend(fake.handle()), true, br#"{"schema":1,"long_poll_seconds":2,"heartbeat_seconds":10}"#, &mut output, &runtime);
        assert_eq!(exit, ExitClass::Permanent);
        let lines = String::from_utf8(output).unwrap();
        assert!(!lines.contains("secret"));
        let kinds = lines.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).map(|line| line.pointer("/data/kind").and_then(Value::as_str).unwrap_or("error").to_owned()).collect::<Vec<_>>();
        assert_eq!(kinds, ["startup", "warning", "event", "heartbeat", "heartbeat", "warning", "heartbeat", "warning", "heartbeat", "error", "stopped"]);
        assert!(lines.contains("\"at\":123") && lines.contains("\"unhandled\":7") && lines.contains("\"inbox_full\":true"));
        assert_eq!(*runtime.sleeps.lock().unwrap(), [Duration::from_millis(250), Duration::from_millis(250), Duration::from_secs(2), Duration::from_secs(1)]);
    }

    #[test]
    fn real_assembly_is_inert_and_outer_failures_are_redacted() {
        TelegramComposition::start().expect("all Telegram exposures assemble without contact");
        let fake = TelegramFake::new().with_send(|_, _| async { Err(contract::SendTextError::Transient) });
        let unavailable = contract::TelegramHandle::from_erased(Arc::new(Unavailable));
        for backend in [HandleBackend(fake.handle()), HandleBackend(unavailable)] {
            let (line, exit) = cli::execute(&backend, true, &["send".into()], br#"{"schema":1,"text":"hi","dedup_key":"s"}"#);
            assert_eq!(exit, ExitClass::Invariant);
            assert_eq!(line, r#"{"schema":1,"ok":false,"command":"send","error":{"code":"invalid_backend_outcome","message":"Telegram capability returned an invalid outcome","retryable":false}}"#);
            assert!(!line.contains("secret"));
        }
    }

    struct Unavailable;
    impl boxology_contract::ErasedCallTarget for Unavailable {
        fn call<'a>(&'a self, _: &'a boxology_contract::CapabilityId, _: boxology_contract::CallContext, _: boxology_contract::SlotValue) -> std::pin::Pin<Box<dyn Future<Output = Result<boxology_contract::SlotValue, boxology_contract::ErasedCallError>> + Send + 'a>> {
            Box::pin(std::future::ready(Err(boxology_contract::ErasedCallError::Unavailable(boxology_contract::Detail::new("secret transport detail")))))
        }
    }

    fn receipt(event: bool) -> contract::PollReceipt { contract::PollReceipt { fetched: event, locally_durable: event.then_some(true), telegram_confirmed: event.then_some(true), next_offset: 4, telegram_confirmed_before: 3, callback_receipt_failed: false } }
    fn text_event() -> contract::InboundEvent { contract::InboundEvent { event_id: "e".into(), kind: contract::InboundEventKind::Text, text: Some("hello".into()), received_at: 2, reply_to: None, ask_id: None, lifecycle_key: None, choice: None } }
    fn local_status() -> contract::StatusResult { contract::StatusResult { probe: None, local: Some(contract::LocalStatus { enabled: true, paired: true, next_offset: 4, telegram_confirmed_before: 3, consumer_locked: true, inbox: contract::InboxStatus { unhandled: 7, bytes: 9, full: true }, asks: contract::AskStatus { active: 0, total: 0 }, outbound: contract::OutboundStatus { ambiguous: 0, total: 0 }, pending_pair: false, last_receive_at: None, last_error_code: None }) } }

    #[derive(Default)]
    struct TestLoop { now: Mutex<u64>, sleeps: Mutex<Vec<Duration>> }
    impl LoopRuntime for TestLoop {
        fn elapsed(&self) -> Duration { Duration::from_secs(*self.now.lock().unwrap()) }
        fn unix_time(&self) -> i64 { 123 }
        fn sleep(&self, duration: Duration) { self.sleeps.lock().unwrap().push(duration); *self.now.lock().unwrap() += 10; }
        fn stopped(&self) -> bool { false }
    }
}
