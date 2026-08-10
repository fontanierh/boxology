use crate::{AppError, ExitClass, SCHEMA};
use boxology_generated_contract as contract;
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_INPUT: usize = 65_536;

macro_rules! backend_methods {
    ($emit:ident) => {
        $emit! {
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
    };
}
macro_rules! declare_backend {($($method:ident($request:ty) -> $outcome:ty;)*) => {
    #[doc(hidden)] pub trait Backend {$(fn $method(&self, request: $request) -> Result<$outcome, AppError>;)*}
}}
backend_methods!(declare_backend);
pub(crate) use backend_methods;

macro_rules! requests {
    ($($name:ident { $($field:ident: $kind:ty),* $(,)? })*) => {$(
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $name { schema: u8, $($field: $kind),* }
        impl Request for $name { fn schema(&self) -> u8 { self.schema } }
    )*};
}

trait Request {
    fn schema(&self) -> u8;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Alternative {
    key: String,
    label: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolution {
    kind: String,
    message_id: Option<i64>,
}

requests! {
    SendJson { text: String, dedup_key: String }
    AskJson { summary: String, recommendation: String, alternatives: Option<Vec<Alternative>>, lifecycle_key: String, dedup_key: String }
    ReplyJson { event_id: String, text: String, dedup_key: String }
    ResolveJson { dedup_key: String, resolution: Resolution }
    PairBeginJson { nonce_ttl_seconds: Option<u64> }
    PairCompleteJson { timeout_seconds: Option<u64> }
    PairRevokeJson {}
    PollJson { timeout_seconds: Option<u64> }
    AckJson { event_id: String }
    StatusJson { probe: bool }
}

pub fn execute(
    backend: &impl Backend,
    enabled: bool,
    args: &[String],
    input: &[u8],
) -> (String, ExitClass) {
    let (command, subcommand) = match args {
        [command] => (command.as_str(), None),
        [command, subcommand] if command == "pair" => (command.as_str(), Some(subcommand.as_str())),
        _ => {
            return failure(
                "unknown",
                AppError::input("invalid_command", "invalid command"),
            );
        }
    };
    if command == "status" {
        return status(backend, input);
    }
    if !enabled {
        return failure(command, AppError::authorization());
    }
    if command == "pair" {
        return match subcommand {
            Some("begin") => call::<PairBeginJson, _>(input, |r| {
                backend.pair_begin(contract::PairBeginRequest {
                    nonce_ttl_seconds: r.nonce_ttl_seconds,
                })
            })
            .map_or_else(
                |e| failure("pair", e),
                |o| select("pair", o.pairing, o.error, pair_begin),
            ),
            Some("complete") => call::<PairCompleteJson, _>(input, |r| {
                backend.pair_complete(contract::PairCompleteRequest {
                    timeout_seconds: r.timeout_seconds,
                })
            })
            .map_or_else(
                |e| failure("pair", e),
                |o| select("pair", o.pairing, o.error, pair_complete),
            ),
            Some("revoke") => call::<PairRevokeJson, _>(input, |_| {
                backend.pair_revoke(contract::PairRevokeRequest {})
            })
            .map_or_else(
                |e| failure("pair", e),
                |o| {
                    select("pair", o.revocation, o.error, |r| {
                        Ok(json!({"pairing_revoked": r.pairing_revoked}))
                    })
                },
            ),
            _ => failure(
                "pair",
                AppError::input("invalid_subcommand", "invalid pair operation"),
            ),
        };
    }
    match command {
        "send" => call::<SendJson, _>(input, |r| backend.send(contract::SendRequest { text: r.text, dedup_key: r.dedup_key })).map_or_else(|e| failure(command, e), |o| select(command, o.delivery, o.error, delivery)),
        "ask" => call::<AskJson, _>(input, |r| backend.ask(contract::AskRequest { summary: r.summary, recommendation: r.recommendation, alternatives: r.alternatives.map(|values| values.into_iter().map(|a| contract::AskAlternative { key: a.key, label: a.label, text: a.text }).collect()), lifecycle_key: r.lifecycle_key, dedup_key: r.dedup_key })).map_or_else(|e| failure(command, e), |o| select(command, o.ask, o.error, ask)),
        "reply" => call::<ReplyJson, _>(input, |r| backend.reply(contract::ReplyRequest { event_id: r.event_id, text: r.text, dedup_key: r.dedup_key })).map_or_else(|e| failure(command, e), |o| select(command, o.delivery, o.error, delivery)),
        "resolve-send" => call::<ResolveJson, _>(input, |r| backend.resolve_send(contract::ResolveSendRequest { dedup_key: r.dedup_key, resolution: contract::DeliveryResolution { kind: match r.resolution.kind { kind if kind == "delivered" => contract::ResolutionKind::Delivered, kind if kind == "not_delivered" => contract::ResolutionKind::NotDelivered, tag => contract::ResolutionKind::Unknown { tag, payload: boxology_contract::OpaquePayload::new(boxology_contract::OpaqueTree::Null) } }, message_id: r.resolution.message_id } })).map_or_else(|e| failure(command, e), |o| select(command, o.resolution, o.error, resolution)),
        "poll" => call::<PollJson, _>(input, |r| backend.poll(contract::PollRequest { timeout_seconds: r.timeout_seconds })).map_or_else(|e| failure(command, e), |o| select(command, o.result, o.error, poll)),
        "ack" => call::<AckJson, _>(input, |r| backend.ack(contract::AckRequest { event_id: r.event_id })).map_or_else(|e| failure(command, e), |o| select(command, o.acknowledgement, o.error, |r| Ok(json!({"event_id": r.event_id, "handled": r.handled, "already_handled": r.already_handled})))),
        _ => failure(command, AppError::unsupported()),
    }
}

fn status(backend: &impl Backend, input: &[u8]) -> (String, ExitClass) {
    call::<StatusJson, _>(input, |r| {
        let probe = r.probe;
        backend
            .status(contract::StatusRequest { probe })
            .map(|outcome| (probe, outcome))
    })
    .map_or_else(
        |e| failure("status", e),
        |(probe, o)| select("status", o.status, o.error, |r| status_value(probe, r)),
    )
}

fn call<R: Request + for<'de> Deserialize<'de>, O>(
    input: &[u8],
    f: impl FnOnce(R) -> Result<O, AppError>,
) -> Result<O, AppError> {
    let request: R = parse(input)?;
    if request.schema() != SCHEMA {
        return Err(AppError::input("unsupported_schema", "unsupported schema"));
    }
    f(request).map_err(|_| AppError::invariant())
}

pub(crate) fn parse<T: for<'de> Deserialize<'de>>(input: &[u8]) -> Result<T, AppError> {
    if input.len() > MAX_INPUT {
        return Err(AppError::input(
            "input_too_large",
            "request exceeds input limit",
        ));
    }
    serde_json::from_slice(input)
        .map_err(|_| AppError::input("invalid_json", "request must be one valid JSON object"))
}

fn select<T>(
    command: &str,
    value: Option<T>,
    error: Option<contract::OperationError>,
    project: impl FnOnce(T) -> Result<Value, ()>,
) -> (String, ExitClass) {
    match (value, error) {
        (Some(value), None) => {
            project(value).map_or_else(|_| invariant(command), |value| success(command, value))
        }
        (None, Some(error)) => operation_failure(command, error),
        _ => invariant(command),
    }
}

fn delivery(r: contract::DeliveryReceipt) -> Result<Value, ()> {
    Ok(
        json!({"dedup_key": r.dedup_key, "deduplicated": r.deduplicated, "delivery": "delivered", "message_id": r.message_id}),
    )
}
fn ask(r: contract::AskReceipt) -> Result<Value, ()> {
    Ok(
        json!({"ask_id": r.ask_id, "lifecycle_key": r.lifecycle_key, "dedup_key": r.delivery.dedup_key, "delivery": "delivered", "message_id": r.delivery.message_id, "deduplicated": r.delivery.deduplicated}),
    )
}
fn pair_begin(r: contract::PairBeginReceipt) -> Result<Value, ()> {
    Ok(
        json!({"deep_link": r.deep_link, "expires_at": r.expires_at, "bot": {"id": r.bot.id, "username": r.bot.username}}),
    )
}
fn pair_complete(r: contract::PairCompleteReceipt) -> Result<Value, ()> {
    let confirmation = match r.confirmation {
        contract::PairConfirmation::Delivered => "delivered",
        contract::PairConfirmation::Ambiguous => "ambiguous",
        contract::PairConfirmation::NotAttempted => "not_attempted",
        contract::PairConfirmation::Unknown { .. } => return Err(()),
    };
    let mut value = json!({"paired": true, "user_id": r.user_id, "chat_id": r.chat_id, "paired_at": r.paired_at, "confirmation": confirmation});
    if confirmation == "ambiguous" {
        value["warnings"] = json!(["pairing confirmation delivery is ambiguous"]);
    }
    Ok(value)
}
fn resolution(r: contract::ResolveSendReceipt) -> Result<Value, ()> {
    match (r.resolved, r.message_id) {
        (contract::ResolutionKind::Delivered, Some(message_id)) => {
            Ok(json!({"dedup_key": r.dedup_key, "resolved": "delivered", "message_id": message_id}))
        }
        (contract::ResolutionKind::NotDelivered, None) => {
            Ok(json!({"dedup_key": r.dedup_key, "resolved": "not_delivered"}))
        }
        _ => Err(()),
    }
}

fn poll(r: contract::PollResult) -> Result<Value, ()> {
    let mut value = match r.event {
        Some(event) => {
            if r.receipt.locally_durable != Some(true) {
                return Err(());
            }
            let confirmed = r.receipt.telegram_confirmed.ok_or(())?;
            json!({"event": event_value(event)?, "receipt": {"locally_durable": true, "telegram_confirmed": confirmed, "fetched": r.receipt.fetched}})
        }
        None => {
            if r.receipt.locally_durable.is_some() || r.receipt.telegram_confirmed.is_some() {
                return Err(());
            }
            json!({"event": Value::Null, "receipt": {"fetched": r.receipt.fetched, "next_offset": r.receipt.next_offset, "telegram_confirmed_before": r.receipt.telegram_confirmed_before}})
        }
    };
    if r.receipt.callback_receipt_failed {
        value["warnings"] = json!(["callback was stored but its Telegram UI receipt failed"]);
    }
    Ok(value)
}

fn event_value(e: contract::InboundEvent) -> Result<Value, ()> {
    let reply = e
        .reply_to
        .map(|r| json!({"ask_id": r.ask_id, "outbound_message_id": r.outbound_message_id}))
        .unwrap_or_else(|| json!({"ask_id": Value::Null, "outbound_message_id": Value::Null}));
    let base = json!({"event_id": e.event_id, "received_at": e.received_at, "reply_to": reply});
    match (e.kind, e.text, e.ask_id, e.lifecycle_key, e.choice) {
        (contract::InboundEventKind::Text, Some(text), None, None, None) => Ok(
            json!({"event_id": base["event_id"], "kind": "text", "received_at": base["received_at"], "reply_to": base["reply_to"], "text": text}),
        ),
        (contract::InboundEventKind::AskReply, Some(text), Some(ask_id), Some(lifecycle), None) => {
            Ok(
                json!({"event_id": base["event_id"], "kind": "ask_reply", "received_at": base["received_at"], "reply_to": base["reply_to"], "text": text, "ask_id": ask_id, "lifecycle_key": lifecycle}),
            )
        }
        (
            contract::InboundEventKind::AskChoice,
            None,
            Some(ask_id),
            Some(lifecycle),
            Some(choice),
        ) => Ok(
            json!({"event_id": base["event_id"], "kind": "ask_choice", "received_at": base["received_at"], "reply_to": base["reply_to"], "ask_id": ask_id, "lifecycle_key": lifecycle, "choice": {"kind": choice.kind, "key": choice.key}}),
        ),
        _ => Err(()),
    }
}

fn status_value(probe: bool, r: contract::StatusResult) -> Result<Value, ()> {
    match (probe, r.local, r.probe) {
        (false, Some(l), None) => Ok(
            json!({"probe": false, "enabled": l.enabled, "paired": l.paired, "next_offset": l.next_offset, "telegram_confirmed_before": l.telegram_confirmed_before, "consumer_locked": l.consumer_locked, "inbox": {"unhandled": l.inbox.unhandled, "bytes": l.inbox.bytes, "full": l.inbox.full}, "asks": {"active": l.asks.active, "total": l.asks.total}, "outbound": {"ambiguous": l.outbound.ambiguous, "total": l.outbound.total}, "pending_pair": l.pending_pair, "last_receive_at": l.last_receive_at, "last_error_code": l.last_error_code}),
        ),
        (true, None, Some(p)) => Ok(
            json!({"probe": true, "api_reachable": p.api_reachable, "bot_matches": p.bot_matches, "webhook_configured": p.webhook_configured, "get_updates_compatible": p.get_updates_compatible}),
        ),
        _ => Err(()),
    }
}

fn operation_failure(command: &str, error: contract::OperationError) -> (String, ExitClass) {
    let exit = match error.class {
        contract::FailureClass::Input => ExitClass::Input,
        contract::FailureClass::Authorization => ExitClass::Authorization,
        contract::FailureClass::Conflict => ExitClass::Conflict,
        contract::FailureClass::Local => ExitClass::Local,
        contract::FailureClass::Policy => ExitClass::Policy,
        contract::FailureClass::Transient => ExitClass::Transient,
        contract::FailureClass::Permanent => ExitClass::Permanent,
        contract::FailureClass::Ambiguous => ExitClass::Ambiguous,
        contract::FailureClass::Invariant => ExitClass::Invariant,
        contract::FailureClass::Unknown { .. } => return invariant(command),
    };
    failure_parts(
        command,
        &error.code,
        &error.message,
        error.retryable,
        error.retry_after_seconds,
        exit,
    )
}

fn invariant(command: &str) -> (String, ExitClass) {
    failure(command, AppError::invariant())
}
fn success(command: &str, data: Value) -> (String, ExitClass) {
    let command = serde_json::to_string(command).expect("command serialization");
    let data = serde_json::to_string(&data).expect("data serialization");
    (
        format!(r#"{{"schema":{SCHEMA},"ok":true,"command":{command},"data":{data}}}"#),
        ExitClass::Success,
    )
}
fn failure(command: &str, error: AppError) -> (String, ExitClass) {
    failure_parts(
        command,
        error.code,
        error.message,
        error.retryable,
        error.retry_after,
        error.exit,
    )
}
fn failure_parts(
    command: &str,
    code: &str,
    message: &str,
    retryable: bool,
    retry_after: Option<u64>,
    exit: ExitClass,
) -> (String, ExitClass) {
    let command = serde_json::to_string(command).expect("command serialization");
    let code = serde_json::to_string(code).expect("code serialization");
    let message = serde_json::to_string(message).expect("message serialization");
    let after = retry_after.map_or_else(String::new, |value| {
        format!(r#","retry_after_seconds":{value}"#)
    });
    (
        format!(
            r#"{{"schema":{SCHEMA},"ok":false,"command":{command},"error":{{"code":{code},"message":{message},"retryable":{retryable}{after}}}}}"#
        ),
        exit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Fake(RefCell<Vec<String>>, Cell<u8>);
    macro_rules! methods {
        ($($name:ident($this:ident, $request:ident: $input:ty) -> $output:ty $body:block)*) => {
            impl Backend for Fake {$(
                fn $name(&self, $request: $input) -> Result<$output, AppError> {
                    self.0.borrow_mut().push(format!("{}:{:?}", stringify!($name), $request));
                    let $this = self; $body
                }
            )*}
        };
    }
    methods! {
        send(s, r: contract::SendRequest) -> contract::DeliveryOutcome { match s.1.get() { 1 => Ok(contract::DeliveryOutcome { delivery: Some(contract::DeliveryReceipt { dedup_key: r.dedup_key, message_id: 1, deduplicated: false }), error: Some(error(contract::FailureClass::Input)) }), 2 => Err(AppError::new("secret_call_code", "secret call detail", ExitClass::Transient)), 3 => Ok(contract::DeliveryOutcome { delivery: None, error: Some(error(contract::FailureClass::Unknown { tag: "future".into(), payload: boxology_contract::OpaquePayload::new(boxology_contract::OpaqueTree::Null) })) }), _ => Ok(contract::DeliveryOutcome { delivery: Some(contract::DeliveryReceipt { dedup_key: r.dedup_key, message_id: r.text.len() as i64, deduplicated: false }), error: None }) } }
        ask(_s, r: contract::AskRequest) -> contract::AskOutcome { Ok(contract::AskOutcome { ask: Some(contract::AskReceipt { ask_id: format!("{}:{}:{:?}", r.summary, r.recommendation, r.alternatives), lifecycle_key: r.lifecycle_key, delivery: contract::DeliveryReceipt { dedup_key: r.dedup_key, message_id: r.alternatives.unwrap_or_default().len() as i64, deduplicated: false } }), error: None }) }
        reply(_s, r: contract::ReplyRequest) -> contract::DeliveryOutcome { Ok(contract::DeliveryOutcome { delivery: Some(contract::DeliveryReceipt { dedup_key: r.dedup_key, message_id: (r.event_id.len() + r.text.len()) as i64, deduplicated: true }), error: None }) }
        resolve_send(_s, r: contract::ResolveSendRequest) -> contract::ResolveSendOutcome { match r.resolution.kind { contract::ResolutionKind::Unknown { tag, .. } => { assert_eq!(tag, "future"); Ok(contract::ResolveSendOutcome { resolution: None, error: Some(error(contract::FailureClass::Input)) }) }, kind => Ok(contract::ResolveSendOutcome { resolution: Some(contract::ResolveSendReceipt { dedup_key: r.dedup_key, resolved: kind, message_id: r.resolution.message_id }), error: None }) } }
        pair_begin(_s, r: contract::PairBeginRequest) -> contract::PairBeginOutcome { Ok(contract::PairBeginOutcome { pairing: Some(contract::PairBeginReceipt { deep_link: "link".into(), expires_at: r.nonce_ttl_seconds.unwrap_or_default() as i64, bot: contract::TelegramBotIdentity { id: 7, username: "bot".into() } }), error: None }) }
        pair_complete(s, r: contract::PairCompleteRequest) -> contract::PairCompleteOutcome { Ok(contract::PairCompleteOutcome { pairing: Some(contract::PairCompleteReceipt { user_id: 8, chat_id: 9, paired_at: r.timeout_seconds.unwrap_or_default() as i64, confirmation: if s.1.get() == 4 { contract::PairConfirmation::Unknown { tag: "future".into(), payload: boxology_contract::OpaquePayload::new(boxology_contract::OpaqueTree::Null) } } else { contract::PairConfirmation::Delivered } }), error: None }) }
        pair_revoke(_s, _r: contract::PairRevokeRequest) -> contract::PairRevokeOutcome { Ok(contract::PairRevokeOutcome { revocation: Some(contract::PairRevokeReceipt { pairing_revoked: true }), error: None }) }
        poll(s, r: contract::PollRequest) -> contract::PollOutcome { if matches!(s.1.get(), 5 | 7) { let (kind, text, ask_id, lifecycle_key, choice) = match (s.1.get(), r.timeout_seconds) { (7, _) => (contract::InboundEventKind::Unknown { tag: "future".into(), payload: boxology_contract::OpaquePayload::new(boxology_contract::OpaqueTree::Null) }, Some("t".into()), None, None, None), (_, Some(2)) => (contract::InboundEventKind::AskReply, Some("t".into()), Some("a".into()), Some("l".into()), Some(contract::InboundChoice { kind: "bad".into(), key: None })), (_, Some(3)) => (contract::InboundEventKind::AskChoice, Some("bad".into()), Some("a".into()), Some("l".into()), Some(contract::InboundChoice { kind: "choice".into(), key: None })), _ => (contract::InboundEventKind::Text, Some("t".into()), Some("invalid".into()), None, None) }; Ok(contract::PollOutcome { result: Some(contract::PollResult { event: Some(contract::InboundEvent { event_id: "e".into(), kind, text, received_at: 1, reply_to: None, ask_id, lifecycle_key, choice }), receipt: contract::PollReceipt { fetched: false, locally_durable: Some(true), telegram_confirmed: Some(false), next_offset: 0, telegram_confirmed_before: 0, callback_receipt_failed: false } }), error: None }) } else { Ok(contract::PollOutcome { result: Some(contract::PollResult { event: None, receipt: contract::PollReceipt { fetched: false, locally_durable: None, telegram_confirmed: None, next_offset: r.timeout_seconds.unwrap_or_default() as i64, telegram_confirmed_before: 4, callback_receipt_failed: false } }), error: None }) } }
        ack(_s, r: contract::AckRequest) -> contract::AckOutcome { Ok(contract::AckOutcome { acknowledgement: Some(contract::AckReceipt { event_id: r.event_id, handled: true, already_handled: false }), error: None }) }
        status(s, r: contract::StatusRequest) -> contract::StatusOutcome { if s.1.get() == 6 { Ok(contract::StatusOutcome { status: None, error: Some(contract::OperationError { code: "telegram_disabled".into(), message: "Telegram requires BOXOLOGY_TELEGRAM_ENABLED=1".into(), retryable: false, retry_after_seconds: None, class: contract::FailureClass::Authorization }) }) } else { Ok(contract::StatusOutcome { status: Some(if r.probe { contract::StatusResult { local: None, probe: Some(contract::ProbeStatus { api_reachable: true, bot_matches: true, webhook_configured: false, get_updates_compatible: true }) } } else { contract::StatusResult { local: Some(contract::LocalStatus { enabled: false, paired: false, next_offset: 0, telegram_confirmed_before: 0, consumer_locked: false, inbox: contract::InboxStatus { unhandled: 0, bytes: 0, full: false }, asks: contract::AskStatus { active: 0, total: 0 }, outbound: contract::OutboundStatus { ambiguous: 0, total: 0 }, pending_pair: false, last_receive_at: None, last_error_code: None }), probe: None } }), error: None }) } }
        listen_start(_s, _r: contract::ListenStartRequest) -> contract::ListenStartOutcome { Ok(contract::ListenStartOutcome { startup: Some(contract::ListenStartReceipt { next_offset: 1, unhandled: 2 }), error: None }) }
    }

    fn error(class: contract::FailureClass) -> contract::OperationError {
        contract::OperationError {
            code: "invalid_resolution".into(),
            message: "delivery resolution is invalid".into(),
            retryable: false,
            retry_after_seconds: None,
            class,
        }
    }

    fn ok(fake: &Fake, args: &[&str], input: Value) -> Value {
        let args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (output, exit) = execute(fake, true, &args, &serde_json::to_vec(&input).unwrap());
        assert_eq!(exit, ExitClass::Success);
        serde_json::from_str::<Value>(&output).unwrap()["data"].clone()
    }

    #[test]
    fn every_command_maps_to_typed_backend_and_legacy_projection() {
        let fake = Fake(RefCell::new(Vec::new()), Cell::new(0));
        macro_rules! cases {($([$($arg:literal),+] $input:expr => $pointer:literal = $expected:expr;)*) => {$(
            let data = ok(&fake, &[$($arg),+], $input);
            assert_eq!(data.pointer($pointer), Some(&$expected));
        )*}}
        cases! {
            ["send"] json!({"schema":1,"text":"abc","dedup_key":"s"}) => "/message_id" = json!(3);
            ["ask"] json!({"schema":1,"summary":"sum","recommendation":"rec","alternatives":[{"key":"alt","label":"Alt","text":"text"}],"lifecycle_key":"life","dedup_key":"a"}) => "/message_id" = json!(1);
            ["reply"] json!({"schema":1,"event_id":"tg:1","text":"hi","dedup_key":"r"}) => "/message_id" = json!(6);
            ["resolve-send"] json!({"schema":1,"dedup_key":"x","resolution":{"kind":"delivered","message_id":12}}) => "/resolved" = json!("delivered");
            ["pair","begin"] json!({"schema":1,"nonce_ttl_seconds":60}) => "/expires_at" = json!(60);
            ["pair","complete"] json!({"schema":1,"timeout_seconds":5}) => "/paired_at" = json!(5);
            ["pair","revoke"] json!({"schema":1}) => "/pairing_revoked" = json!(true);
            ["poll"] json!({"schema":1,"timeout_seconds":9}) => "/receipt/next_offset" = json!(9);
            ["ack"] json!({"schema":1,"event_id":"tg:2"}) => "/event_id" = json!("tg:2");
            ["status"] json!({"schema":1,"probe":false}) => "/enabled" = json!(false);
            ["status"] json!({"schema":1,"probe":true}) => "/api_reachable" = json!(true);
        }
        assert_eq!(
            fake.0.borrow().join("|"),
            "send:SendRequest { text: \"abc\", dedup_key: \"s\" }|ask:AskRequest { summary: \"sum\", recommendation: \"rec\", alternatives: Some([AskAlternative { key: \"alt\", label: \"Alt\", text: \"text\" }]), lifecycle_key: \"life\", dedup_key: \"a\" }|reply:ReplyRequest { event_id: \"tg:1\", text: \"hi\", dedup_key: \"r\" }|resolve_send:ResolveSendRequest { dedup_key: \"x\", resolution: DeliveryResolution { kind: Delivered, message_id: Some(12) } }|pair_begin:PairBeginRequest { nonce_ttl_seconds: Some(60) }|pair_complete:PairCompleteRequest { timeout_seconds: Some(5) }|pair_revoke:PairRevokeRequest|poll:PollRequest { timeout_seconds: Some(9) }|ack:AckRequest { event_id: \"tg:2\" }|status:StatusRequest { probe: false }|status:StatusRequest { probe: true }"
        );
    }

    #[test]
    fn ordering_strictness_and_failure_projection_fail_closed() {
        let fake = Fake(RefCell::new(Vec::new()), Cell::new(0));
        let bad = |enabled, args: &[&str], input: &[u8]| {
            execute(
                &fake,
                enabled,
                &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                input,
            )
        };
        macro_rules! bad_cases {($($enabled:literal [$($arg:literal),*] $input:expr => $exit:ident;)*) => {$(
            assert_eq!(bad($enabled, &[$($arg),*], $input).1, ExitClass::$exit);
        )*}}
        bad_cases! {
            false ["send"] b"not json" => Authorization;
            false [] b"not json" => Input;
            false ["pair","bad"] b"not json" => Authorization;
            true ["pair","bad"] b"not json" => Input;
            true ["send"] br#"{"schema":1,"text":"x","dedup_key":"k","extra":1}"# => Input;
            true ["status"] &vec![b' '; MAX_INPUT + 1] => Input;
            true ["status"] br#"{"schema":1,"schema":1,"probe":false}"# => Input;
        }
        assert!(fake.0.borrow().is_empty());
        let expected = bad(false, &["status"], br#"{"schema":1,"probe":false}"#);
        fake.0.borrow_mut().clear();
        let mut exact = br#"{"schema":1,"probe":false}"#.to_vec();
        exact.resize(MAX_INPUT, b' ');
        assert_eq!(bad(false, &["status"], &exact), expected);
        assert_eq!(
            fake.0.borrow().as_slice(),
            &["status:StatusRequest { probe: false }"]
        );
        fake.1.set(6);
        assert_eq!(bad(false, &["status"], br#"{"schema":1,"probe":true}"#), (r#"{"schema":1,"ok":false,"command":"status","error":{"code":"telegram_disabled","message":"Telegram requires BOXOLOGY_TELEGRAM_ENABLED=1","retryable":false}}"#.into(), ExitClass::Authorization));
        fake.1.set(0);
        assert_eq!(bad(true, &["resolve-send"], br#"{"schema":1,"dedup_key":"x","resolution":{"kind":"future"}}"#), (r#"{"schema":1,"ok":false,"command":"resolve-send","error":{"code":"invalid_resolution","message":"delivery resolution is invalid","retryable":false}}"#.into(), ExitClass::Input));
        let calls = fake.0.borrow();
        assert!(
            calls
                .last()
                .unwrap()
                .contains("kind: Unknown { tag: \"future\", payload: OpaquePayload(<redacted>) }")
        );
        drop(calls);
        let invariant = r#"{"schema":1,"ok":false,"command":"send","error":{"code":"invalid_backend_outcome","message":"Telegram capability returned an invalid outcome","retryable":false}}"#;
        let send = br#"{"schema":1,"text":"x","dedup_key":"k"}"#;
        macro_rules! invariant_cases {($($mode:literal [$command:literal $(,$arg:literal)*] $input:expr;)*) => {$( fake.1.set($mode); assert_eq!(bad(true, &[$command $(,$arg)*], $input), (failure($command, AppError::invariant()).0, ExitClass::Invariant)); )*}}
        invariant_cases! {
            1 ["send"] send;
            2 ["send"] send;
            3 ["send"] send;
            4 ["pair","complete"] br#"{"schema":1}"#;
            5 ["poll"] br#"{"schema":1,"timeout_seconds":1}"#;
            5 ["poll"] br#"{"schema":1,"timeout_seconds":2}"#;
            5 ["poll"] br#"{"schema":1,"timeout_seconds":3}"#;
            7 ["poll"] br#"{"schema":1}"#;
        }
        fake.1.set(2);
        assert_eq!(
            bad(true, &["send"], send),
            (invariant.into(), ExitClass::Invariant)
        );
        macro_rules! exits {($($class:ident),*) => {$(
            assert_eq!(operation_failure("send", contract::OperationError { code: "x".into(), message: "m".into(), retryable: false, retry_after_seconds: None, class: contract::FailureClass::$class }).1, ExitClass::$class);
        )*}}
        exits!(
            Input,
            Authorization,
            Conflict,
            Local,
            Policy,
            Transient,
            Permanent,
            Ambiguous,
            Invariant
        );
    }
}
