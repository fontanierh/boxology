use crate::api::{self, Api};
use crate::state::{self, EventRecord, Paths, ReplyTarget, State};
use crate::{AppError, ExitClass, SCHEMA, api_error, parse};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PollRequest {
    schema: u8,
    timeout_seconds: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckRequest {
    schema: u8,
    event_id: String,
}

pub(crate) fn poll(input: &[u8]) -> Result<Value, AppError> {
    let request: PollRequest = parse(input)?;
    check_schema(request.schema)?;
    let timeout = request.timeout_seconds.unwrap_or(30);
    if timeout > 50 {
        return Err(AppError::input(
            "invalid_timeout",
            "poll timeout is out of bounds",
        ));
    }
    let paths = Paths::from_env()?;
    let _consumer = state::ConsumerLock::acquire(&paths)?;
    let current = state::read(&paths)?;
    ensure_pairing(&current)?;
    if let Some(event) = oldest_unhandled(&current) {
        return Ok(poll_result(Some(event), false, &current));
    }
    if inbox_full(&current) {
        return Err(AppError::new(
            "inbox_full",
            "inbound storage is full",
            ExitClass::Policy,
        ));
    }
    let token = api::load_token()?;
    let api = Api::production(token)?;
    let updates = api
        .get_updates(current.next_offset, timeout)
        .map_err(api_error)?;
    validate_update_order(&updates)?;
    let max_id = updates.last().map(|update| update.update_id);
    let next_offset = match max_id {
        Some(id) => id.checked_add(1).ok_or_else(|| {
            AppError::new(
                "offset_overflow",
                "Telegram offset is invalid",
                ExitClass::Invariant,
            )
        })?,
        None => current.next_offset,
    };
    let before_offset = current.next_offset;
    state::update(&paths, |state| {
        if state.next_offset != before_offset {
            return Err(AppError::new(
                "state_changed",
                "inbound state changed during polling",
                ExitClass::Policy,
            ));
        }
        for update in &updates {
            if let Some(event) = project(update, state)
                && !state
                    .events
                    .iter()
                    .any(|old| old.event_id == event.event_id)
            {
                state.events.push(event);
            }
        }
        state.next_offset = next_offset;
        state.confirmed_before = state.confirmed_before.max(before_offset);
        state.last_receive_at = Some(state::now());
        Ok(())
    })?;
    let state = state::read(&paths)?;
    Ok(poll_result(oldest_unhandled(&state), true, &state))
}

pub(crate) fn ack(input: &[u8]) -> Result<Value, AppError> {
    let request: AckRequest = parse(input)?;
    check_schema(request.schema)?;
    if request.event_id.is_empty()
        || request.event_id.len() > 128
        || !request.event_id.starts_with("tg:")
    {
        return Err(AppError::input(
            "invalid_event_id",
            "event identifier is invalid",
        ));
    }
    let paths = Paths::from_env()?;
    state::update(&paths, |state| {
        let event = state
            .events
            .iter_mut()
            .find(|event| event.event_id == request.event_id)
            .ok_or_else(|| {
                AppError::new("unknown_event", "event is not available", ExitClass::Policy)
            })?;
        let already_handled = event.handled;
        event.handled = true;
        Ok(
            json!({"event_id": request.event_id, "handled": true, "already_handled": already_handled}),
        )
    })
}

fn project(update: &api::Update, state: &State) -> Option<EventRecord> {
    let message = update.message.as_ref()?;
    let user = message.from.as_ref()?;
    if user.is_bot
        || message.chat.kind != "private"
        || state.pairing.as_ref()?.user_id != user.id
        || state.pairing.as_ref()?.chat_id != message.chat.id
    {
        return None;
    }
    let text = message.text.as_ref()?;
    if text.is_empty() || text.chars().count() > 4096 {
        return None;
    }
    let reply_to = message
        .reply_to_message
        .as_ref()
        .map(|message| message.message_id);
    let reply_to = match reply_to {
        Some(message_id)
            if state
                .outbound
                .iter()
                .any(|outbound| outbound.message_id == Some(message_id)) =>
        {
            Some(ReplyTarget {
                ask_id: None,
                outbound_message_id: Some(message_id),
            })
        }
        Some(_) => return None,
        None => None,
    };
    Some(EventRecord {
        event_id: format!("tg:{}:{}", update.update_id, message.message_id),
        update_id: update.update_id,
        kind: "text".to_string(),
        text: text.clone(),
        received_at: state::now(),
        handled: false,
        reply_to,
        ask_id: None,
        lifecycle_key: None,
        choice: None,
    })
}

fn oldest_unhandled(state: &State) -> Option<EventRecord> {
    state.events.iter().find(|event| !event.handled).cloned()
}

fn poll_result(event: Option<EventRecord>, fetched: bool, state: &State) -> Value {
    let receipt = json!({
        "fetched": fetched,
        "next_offset": state.next_offset,
        "telegram_confirmed_before": state.confirmed_before
    });
    match event {
        Some(event) => {
            json!({"event": event_value(&event, state), "receipt": {"locally_durable": true, "telegram_confirmed": event.update_id < state.confirmed_before, "fetched": fetched}})
        }
        None => json!({"event": Value::Null, "receipt": receipt}),
    }
}

fn event_value(event: &EventRecord, _state: &State) -> Value {
    json!({
        "event_id": event.event_id,
        "kind": event.kind,
        "text": event.text,
        "received_at": event.received_at,
        "reply_to": event.reply_to.as_ref().map(|target| json!({"ask_id": target.ask_id, "outbound_message_id": target.outbound_message_id})).unwrap_or(json!({"ask_id": Value::Null, "outbound_message_id": Value::Null}))
    })
}

fn ensure_pairing(state: &State) -> Result<(), AppError> {
    state.pairing.as_ref().map(|_| ()).ok_or_else(|| {
        AppError::new(
            "not_paired",
            "Telegram pairing is required",
            ExitClass::Policy,
        )
    })
}

fn inbox_full(state: &State) -> bool {
    state.events.len() >= 1_000
        || serde_json::to_vec(&state.events).is_ok_and(|bytes| bytes.len() >= 8 * 1024 * 1024)
}

fn validate_update_order(updates: &[api::Update]) -> Result<(), AppError> {
    if updates.iter().any(|update| update.update_id < 0)
        || updates
            .windows(2)
            .any(|pair| pair[0].update_id >= pair[1].update_id)
    {
        return Err(AppError::new(
            "update_order",
            "Telegram returned unordered updates",
            ExitClass::Transient,
        ));
    }
    Ok(())
}

fn check_schema(schema: u8) -> Result<(), AppError> {
    (schema == SCHEMA)
        .then_some(())
        .ok_or_else(|| AppError::input("unsupported_schema", "unsupported schema"))
}
