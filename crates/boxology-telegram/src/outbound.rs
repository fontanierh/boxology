use crate::api;
use crate::state::{self, EventRecord, OutboundRecord, Paths};
use crate::{AppError, ExitClass, SCHEMA, api_error, parse};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendRequest {
    schema: u8,
    text: String,
    dedup_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyRequest {
    schema: u8,
    event_id: String,
    text: String,
    dedup_key: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveRequest {
    schema: u8,
    dedup_key: String,
    resolution: Resolution,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolution {
    kind: String,
    message_id: Option<i64>,
}

enum Start {
    Send { deduplicated: bool },
    Existing { message_id: i64 },
}

struct Delivery<'a> {
    kind: &'a str,
    dedup_key: &'a str,
    text: &'a str,
    reply_to: Option<i64>,
    buttons: Option<Value>,
    chat_id: i64,
    event: Option<EventRecord>,
    ask_id: Option<&'a str>,
}

pub(crate) fn send(input: &[u8]) -> Result<Value, AppError> {
    let request: SendRequest = parse(input)?;
    check_schema(request.schema)?;
    validate_text(&request.text)?;
    validate_key(&request.dedup_key)?;
    let paths = Paths::from_env()?;
    let chat_id = state::read(&paths)?.pairing.ok_or_else(not_paired)?.chat_id;
    deliver(
        &paths,
        Delivery {
            kind: "send",
            dedup_key: &request.dedup_key,
            text: &request.text,
            reply_to: None,
            buttons: None,
            chat_id,
            event: None,
            ask_id: None,
        },
    )
}

pub(crate) fn reply(input: &[u8]) -> Result<Value, AppError> {
    let request: ReplyRequest = parse(input)?;
    check_schema(request.schema)?;
    validate_text(&request.text)?;
    validate_key(&request.dedup_key)?;
    validate_event_id(&request.event_id)?;
    let paths = Paths::from_env()?;
    let state = state::read(&paths)?;
    let event = state
        .events
        .iter()
        .find(|event| event.event_id == request.event_id)
        .cloned()
        .ok_or_else(|| {
            AppError::new("unknown_event", "event is not available", ExitClass::Policy)
        })?;
    let message_id = event_message_id(&event)?;
    let ask_id = event.ask_id.clone();
    let chat_id = state.pairing.ok_or_else(not_paired)?.chat_id;
    deliver(
        &paths,
        Delivery {
            kind: "reply",
            dedup_key: &request.dedup_key,
            text: &request.text,
            reply_to: Some(message_id),
            buttons: None,
            chat_id,
            event: Some(event),
            ask_id: ask_id.as_deref(),
        },
    )
}

pub(crate) fn resolve(input: &[u8]) -> Result<Value, AppError> {
    let request: ResolveRequest = parse(input)?;
    check_schema(request.schema)?;
    validate_key(&request.dedup_key)?;
    let paths = Paths::from_env()?;
    state::update(&paths, |state| {
        let record = state
            .outbound
            .iter_mut()
            .find(|record| record.dedup_key == request.dedup_key)
            .ok_or_else(|| {
                AppError::new(
                    "unknown_delivery",
                    "outbound record is not available",
                    ExitClass::Policy,
                )
            })?;
        match (
            request.resolution.kind.as_str(),
            request.resolution.message_id,
        ) {
            ("delivered", Some(message_id)) if message_id > 0 => {
                record.state = "delivered".into();
                record.message_id = Some(message_id);
                let event_id = record.event_id.clone();
                if let Some(event_id) = event_id.as_deref()
                    && let Some(event) = state
                        .events
                        .iter_mut()
                        .find(|event| event.event_id == event_id)
                {
                    let ask_id = event.ask_id.clone();
                    event.handled = true;
                    if let Some(ask_id) = ask_id
                        && let Some(ask) = state.asks.iter_mut().find(|ask| ask.ask_id == ask_id)
                    {
                        ask.state = "answered".into();
                    }
                }
                Ok(
                    json!({"dedup_key": request.dedup_key, "resolved": "delivered", "message_id": message_id}),
                )
            }
            ("not_delivered", None) => {
                record.state = "retryable".into();
                record.message_id = None;
                Ok(json!({"dedup_key": request.dedup_key, "resolved": "not_delivered"}))
            }
            _ => Err(AppError::input(
                "invalid_resolution",
                "delivery resolution is invalid",
            )),
        }
    })
}

fn deliver(paths: &Paths, delivery: Delivery<'_>) -> Result<Value, AppError> {
    let Delivery {
        kind,
        dedup_key,
        text,
        reply_to,
        buttons,
        chat_id,
        event,
        ask_id,
    } = delivery;
    let payload_hash = payload_hash(kind, dedup_key, text, reply_to, buttons.as_ref());
    let start = state::update(paths, |state| {
        state.prune_completed();
        if let Some(record) = state
            .outbound
            .iter_mut()
            .find(|record| record.dedup_key == dedup_key)
        {
            if record.kind != kind || record.payload_hash != payload_hash {
                return Err(AppError::new(
                    "dedup_mismatch",
                    "deduplication key has a different payload",
                    ExitClass::Policy,
                ));
            }
            return match record.state.as_str() {
                "delivered" => record
                    .message_id
                    .map(|message_id| Start::Existing { message_id })
                    .ok_or_else(|| {
                        AppError::new(
                            "corrupt_delivery",
                            "outbound state is invalid",
                            ExitClass::Invariant,
                        )
                    }),
                "in_flight" | "ambiguous" => Err(AppError::new(
                    "delivery_ambiguous",
                    "outbound delivery requires explicit resolution",
                    ExitClass::Ambiguous,
                )),
                "retryable" => {
                    record.state = "in_flight".into();
                    Ok(Start::Send { deduplicated: true })
                }
                _ => Err(AppError::new(
                    "corrupt_delivery",
                    "outbound state is invalid",
                    ExitClass::Invariant,
                )),
            };
        }
        if state.outbound.len() >= 1_024 {
            return Err(AppError::new(
                "outbound_full",
                "outbound storage is full",
                ExitClass::Policy,
            ));
        }
        state.outbound.push(OutboundRecord {
            dedup_key: dedup_key.into(),
            kind: kind.into(),
            payload_hash: payload_hash.clone(),
            state: "in_flight".into(),
            message_id: None,
            event_id: event.as_ref().map(|event| event.event_id.clone()),
            ask_id: ask_id.map(str::to_string),
        });
        Ok(Start::Send {
            deduplicated: false,
        })
    })?;
    if let Start::Existing { message_id } = start {
        return Ok(
            json!({"dedup_key": dedup_key, "delivery": "delivered", "message_id": message_id, "deduplicated": true}),
        );
    }
    let token = match api::load_token() {
        Ok(token) => token,
        Err(error) => return Err(mark_retryable(paths, dedup_key, error)),
    };
    let api = match api::for_commands(token) {
        Ok(api) => api,
        Err(error) => return Err(mark_retryable(paths, dedup_key, error)),
    };
    let sent = match api.send_message(chat_id, text, reply_to, buttons) {
        Ok(sent) if sent.message_id > 0 => sent,
        Ok(_) => return Err(mark_ambiguous(paths, dedup_key)),
        Err(error) => {
            if error.ambiguous {
                return Err(mark_ambiguous(paths, dedup_key));
            }
            return Err(mark_retryable(paths, dedup_key, api_error(error)));
        }
    };
    let handled_ask_id = event.as_ref().and_then(|event| event.ask_id.clone());
    state::update(paths, |state| {
        let record = state
            .outbound
            .iter_mut()
            .find(|record| record.dedup_key == dedup_key)
            .ok_or_else(|| {
                AppError::new(
                    "corrupt_delivery",
                    "outbound state is invalid",
                    ExitClass::Invariant,
                )
            })?;
        record.state = "delivered".into();
        record.message_id = Some(sent.message_id);
        if let Some(event) = event.as_ref()
            && let Some(source) = state
                .events
                .iter_mut()
                .find(|old| old.event_id == event.event_id)
        {
            source.handled = true;
        }
        if let Some(ask_id) = handled_ask_id.as_deref()
            && let Some(ask) = state.asks.iter_mut().find(|ask| ask.ask_id == ask_id)
        {
            ask.state = "answered".into();
        }
        Ok(())
    })?;
    Ok(
        json!({"dedup_key": dedup_key, "delivery": "delivered", "message_id": sent.message_id, "deduplicated": matches!(start, Start::Send { deduplicated: true })}),
    )
}

pub(crate) fn deliver_ask(
    paths: &Paths,
    dedup_key: &str,
    text: &str,
    buttons: Value,
    chat_id: i64,
    ask_id: &str,
) -> Result<Value, AppError> {
    let result = deliver(
        paths,
        Delivery {
            kind: "ask",
            dedup_key,
            text,
            reply_to: None,
            buttons: Some(buttons),
            chat_id,
            event: None,
            ask_id: Some(ask_id),
        },
    )?;
    if let Some(message_id) = result.get("message_id").and_then(Value::as_i64) {
        state::update(paths, |state| {
            if let Some(ask) = state.asks.iter_mut().find(|ask| ask.ask_id == ask_id) {
                ask.message_id = Some(message_id);
            }
            Ok(())
        })?;
    }
    Ok(result)
}

fn mark_retryable(paths: &Paths, key: &str, error: AppError) -> AppError {
    let _ = state::update(paths, |state| {
        if let Some(record) = state
            .outbound
            .iter_mut()
            .find(|record| record.dedup_key == key)
        {
            record.state = "retryable".into();
        }
        Ok(())
    });
    error
}

fn mark_ambiguous(paths: &Paths, key: &str) -> AppError {
    let _ = state::update(paths, |state| {
        if let Some(record) = state
            .outbound
            .iter_mut()
            .find(|record| record.dedup_key == key)
        {
            record.state = "ambiguous".into();
        }
        Ok(())
    });
    AppError::new(
        "delivery_ambiguous",
        "outbound delivery requires explicit resolution",
        ExitClass::Ambiguous,
    )
}

fn payload_hash(
    kind: &str,
    dedup_key: &str,
    text: &str,
    reply_to: Option<i64>,
    buttons: Option<&Value>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(dedup_key.as_bytes());
    hasher.update([0]);
    hasher.update(text.as_bytes());
    hasher.update([0]);
    hasher.update(reply_to.unwrap_or_default().to_string().as_bytes());
    if let Some(buttons) = buttons {
        hasher.update(serde_json::to_vec(buttons).unwrap_or_default());
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn event_message_id(event: &EventRecord) -> Result<i64, AppError> {
    event
        .event_id
        .rsplit(':')
        .next()
        .and_then(|id| id.parse().ok())
        .filter(|id: &i64| *id > 0)
        .ok_or_else(|| {
            AppError::new(
                "event_correlation",
                "event cannot be replied to",
                ExitClass::Policy,
            )
        })
}

fn validate_text(text: &str) -> Result<(), AppError> {
    if text.is_empty() || text.chars().count() > 4096 {
        return Err(AppError::input(
            "invalid_text",
            "Telegram text length is out of bounds",
        ));
    }
    Ok(())
}

fn validate_key(key: &str) -> Result<(), AppError> {
    if key.is_empty() || key.len() > 128 || key.chars().any(char::is_whitespace) {
        return Err(AppError::input(
            "invalid_dedup_key",
            "deduplication key is invalid",
        ));
    }
    Ok(())
}

fn validate_event_id(event_id: &str) -> Result<(), AppError> {
    if event_id.len() > 128 || !event_id.starts_with("tg:") {
        return Err(AppError::input(
            "invalid_event_id",
            "event identifier is invalid",
        ));
    }
    Ok(())
}

fn check_schema(schema: u8) -> Result<(), AppError> {
    (schema == SCHEMA)
        .then_some(())
        .ok_or_else(|| AppError::input("unsupported_schema", "unsupported schema"))
}

pub(crate) fn not_paired() -> AppError {
    AppError::new(
        "not_paired",
        "Telegram pairing is required",
        ExitClass::Policy,
    )
}
