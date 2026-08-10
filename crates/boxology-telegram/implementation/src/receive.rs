use crate::api;
use crate::state::{self, ChoiceRecord, EventRecord, Paths, ReplyTarget, State};
use crate::{AppError, ExitClass, api_error};
#[cfg(test)]
use serde_json::{Value, json};

pub(crate) struct PollCommand {
    pub(crate) timeout_seconds: Option<u64>,
}

pub(crate) struct PollResult {
    pub(crate) event: Option<EventRecord>,
    pub(crate) fetched: bool,
    pub(crate) telegram_confirmed: Option<bool>,
    pub(crate) next_offset: i64,
    pub(crate) telegram_confirmed_before: i64,
    pub(crate) callback_warning: bool,
}

pub(crate) fn poll_typed(command: PollCommand) -> Result<PollResult, AppError> {
    poll_command(command, true)
}

pub(crate) fn poll_typed_locked(command: PollCommand) -> Result<PollResult, AppError> {
    poll_command(command, false)
}

fn poll_command(command: PollCommand, acquire_consumer: bool) -> Result<PollResult, AppError> {
    let timeout = command.timeout_seconds.unwrap_or(30);
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
    let mut current = state::read(&paths)?;
    ensure_pairing(&current)?;
    if inbox_full(&current) {
        state::update(&paths, |state| {
            state.prune_handled();
            Ok(())
        })?;
        current = state::read(&paths)?;
    }
    if let Some(event) = oldest_unhandled(&current) {
        return Ok(poll_receipt(Some(event), false, &current, false));
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
    validate_update_order(&updates, current.next_offset)?;
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
    Ok(poll_receipt(
        oldest_unhandled(&state),
        true,
        &state,
        callback_warning,
    ))
}

pub(crate) struct AckCommand {
    pub(crate) event_id: String,
}

pub(crate) struct AckReceipt {
    pub(crate) event_id: String,
    pub(crate) handled: bool,
    pub(crate) already_handled: bool,
}

pub(crate) fn ack_typed(request: AckCommand) -> Result<AckReceipt, AppError> {
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
        state.prune_handled();
        Ok(AckReceipt {
            event_id: request.event_id,
            handled: true,
            already_handled,
        })
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
        || message.forward_origin.is_some()
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
    if callback.id.len() > 256
        || user.is_bot
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

fn poll_receipt(
    event: Option<EventRecord>,
    fetched: bool,
    state: &State,
    callback_warning: bool,
) -> PollResult {
    let telegram_confirmed = event
        .as_ref()
        .map(|event| event.update_id < state.confirmed_before);
    PollResult {
        event,
        fetched,
        telegram_confirmed,
        next_offset: state.next_offset,
        telegram_confirmed_before: state.confirmed_before,
        callback_warning,
    }
}

#[cfg(test)]
pub(crate) fn event_value(event: &EventRecord) -> Value {
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

fn validate_update_order(updates: &[api::Update], requested_offset: i64) -> Result<(), AppError> {
    if updates
        .iter()
        .any(|update| update.update_id < requested_offset)
        || updates
            .windows(2)
            .any(|pair| pair[0].update_id >= pair[1].update_id)
    {
        return Err(AppError::new(
            "update_order",
            "Telegram returned invalid update offsets",
            ExitClass::Transient,
        ));
    }
    Ok(())
}
