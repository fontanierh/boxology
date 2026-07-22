use crate::api;
use crate::state::{self, ChoiceRecord, EventRecord, Paths, ReplyTarget, State};
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
    poll_inner(input, true)
}

pub(crate) fn poll_locked(input: &[u8]) -> Result<Value, AppError> {
    poll_inner(input, false)
}

fn poll_inner(input: &[u8], acquire_consumer: bool) -> Result<Value, AppError> {
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
    let _consumer = if acquire_consumer {
        Some(state::ConsumerLock::acquire(&paths)?)
    } else {
        None
    };
    let current = state::read(&paths)?;
    ensure_pairing(&current)?;
    if let Some(event) = oldest_unhandled(&current) {
        return Ok(poll_result(Some(event), false, &current, false));
    }
    if inbox_full(&current) {
        return Err(AppError::new(
            "inbox_full",
            "inbound storage is full",
            ExitClass::Policy,
        ));
    }
    let token = api::load_token()?;
    let api = api::for_commands(token)?;
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
    let callback_ids: Vec<String> = updates
        .iter()
        .filter_map(|update| accepted_callback_id(update, &current))
        .collect();
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
    let callback_warning = callback_ids
        .iter()
        .any(|callback_id| api.answer_callback(callback_id).is_err());
    let state = state::read(&paths)?;
    Ok(poll_result(
        oldest_unhandled(&state),
        true,
        &state,
        callback_warning,
    ))
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
        let ask_id = event.ask_id.clone();
        event.handled = true;
        if let Some(ask_id) = ask_id
            && let Some(ask) = state.asks.iter_mut().find(|ask| ask.ask_id == ask_id)
        {
            ask.state = "answered".into();
        }
        Ok(
            json!({"event_id": request.event_id, "handled": true, "already_handled": already_handled}),
        )
    })
}

fn project(update: &api::Update, state: &State) -> Option<EventRecord> {
    if let Some(callback) = project_callback(update, state) {
        return Some(callback);
    }
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
    let (reply_to, ask_id, lifecycle_key, kind) = match reply_to {
        Some(message_id) => {
            let outbound = state
                .outbound
                .iter()
                .find(|outbound| outbound.message_id == Some(message_id))?;
            let ask = outbound.ask_id.as_ref().and_then(|ask_id| {
                state
                    .asks
                    .iter()
                    .find(|ask| &ask.ask_id == ask_id && ask.state == "open")
            });
            (
                Some(ReplyTarget {
                    ask_id: outbound.ask_id.clone(),
                    outbound_message_id: Some(message_id),
                }),
                ask.map(|ask| ask.ask_id.clone()),
                ask.map(|ask| ask.lifecycle_key.clone()),
                if ask.is_some() { "ask_reply" } else { "text" },
            )
        }
        None => (None, None, None, "text"),
    };
    Some(EventRecord {
        event_id: format!("tg:{}:{}", update.update_id, message.message_id),
        update_id: update.update_id,
        kind: kind.to_string(),
        text: text.clone(),
        received_at: state::now(),
        handled: false,
        reply_to,
        choice: None,
        ask_id,
        lifecycle_key,
    })
}

fn project_callback(update: &api::Update, state: &State) -> Option<EventRecord> {
    let callback = update.callback_query.as_ref()?;
    let message = callback.message.as_ref()?;
    let user = &callback.from;
    let data = callback.data.as_deref()?;
    if user.is_bot
        || message.chat.kind != "private"
        || data.len() > 64
        || state.pairing.as_ref()?.user_id != user.id
        || state.pairing.as_ref()?.chat_id != message.chat.id
    {
        return None;
    }
    let ask = state
        .asks
        .iter()
        .find(|ask| ask.message_id == Some(message.message_id) && ask.state == "open")?;
    let choice = ask
        .choices
        .iter()
        .find(|choice| matches_callback(choice, data))?
        .clone();
    Some(EventRecord {
        event_id: format!("tg:{}:{}", update.update_id, message.message_id),
        update_id: update.update_id,
        kind: "ask_choice".into(),
        text: String::new(),
        received_at: state::now(),
        handled: false,
        reply_to: Some(ReplyTarget {
            ask_id: Some(ask.ask_id.clone()),
            outbound_message_id: Some(message.message_id),
        }),
        ask_id: Some(ask.ask_id.clone()),
        lifecycle_key: Some(ask.lifecycle_key.clone()),
        choice: Some(choice),
    })
}

fn accepted_callback_id(update: &api::Update, state: &State) -> Option<String> {
    project_callback(update, state)?;
    update
        .callback_query
        .as_ref()
        .map(|callback| callback.id.clone())
}

fn matches_callback(choice: &ChoiceRecord, data: &str) -> bool {
    state::decode_hex(&choice.salt)
        .is_some_and(|salt| state::digest(&salt, data.as_bytes()) == choice.token_digest)
}

pub(crate) fn oldest_unhandled(state: &State) -> Option<EventRecord> {
    state.events.iter().find(|event| !event.handled).cloned()
}

fn poll_result(
    event: Option<EventRecord>,
    fetched: bool,
    state: &State,
    callback_warning: bool,
) -> Value {
    let receipt = json!({
        "fetched": fetched,
        "next_offset": state.next_offset,
        "telegram_confirmed_before": state.confirmed_before
    });
    let mut result = match event {
        Some(event) => {
            json!({"event": event_value(&event, state), "receipt": {"locally_durable": true, "telegram_confirmed": event.update_id < state.confirmed_before, "fetched": fetched}})
        }
        None => json!({"event": Value::Null, "receipt": receipt}),
    };
    if callback_warning {
        result["warnings"] = json!(["callback was stored but its Telegram UI receipt failed"]);
    }
    result
}

pub(crate) fn event_value(event: &EventRecord, _state: &State) -> Value {
    let mut value = json!({
        "event_id": event.event_id,
        "kind": event.kind,
        "received_at": event.received_at,
        "reply_to": event.reply_to.as_ref().map(|target| json!({"ask_id": target.ask_id, "outbound_message_id": target.outbound_message_id})).unwrap_or(json!({"ask_id": Value::Null, "outbound_message_id": Value::Null}))
    });
    match event.kind.as_str() {
        "ask_choice" => {
            if let Some(choice) = &event.choice {
                value["ask_id"] = json!(event.ask_id);
                value["lifecycle_key"] = json!(event.lifecycle_key);
                value["choice"] = json!({"kind": choice.kind, "key": choice.key});
            }
        }
        _ => {
            value["text"] = json!(event.text);
            if event.kind == "ask_reply" {
                value["ask_id"] = json!(event.ask_id);
                value["lifecycle_key"] = json!(event.lifecycle_key);
            }
        }
    }
    value
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
