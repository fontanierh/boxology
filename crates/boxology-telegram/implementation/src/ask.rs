use crate::outbound;
use crate::state::{self, AskRecord, ChoiceRecord, Paths};
use crate::{AppError, AskAlternative, AskReceipt, AskRequest, ExitClass};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) fn run_typed(request: AskRequest) -> Result<AskReceipt, AppError> {
    validate_summary(&request.summary)?;
    validate_text(&request.recommendation, 1_024, "recommendation")?;
    validate_key(&request.lifecycle_key, 128, "lifecycle key")?;
    validate_key(&request.dedup_key, 128, "deduplication key")?;
    let alternatives = request.alternatives.unwrap_or_default();
    if alternatives.len() > 4 {
        return Err(AppError::input(
            "too_many_alternatives",
            "at most four alternatives are allowed",
        ));
    }
    for alternative in &alternatives {
        if alternative.key.is_empty()
            || alternative.key.len() > 32
            || !alternative
                .key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(AppError::input(
                "invalid_alternative",
                "alternative key is invalid",
            ));
        }
        if alternatives
            .iter()
            .filter(|other| other.key == alternative.key)
            .count()
            != 1
        {
            return Err(AppError::input(
                "duplicate_alternative",
                "alternative keys must be unique",
            ));
        }
        if alternative.label.is_empty() || alternative.label.chars().count() > 40 {
            return Err(AppError::input(
                "invalid_alternative",
                "alternative label is invalid",
            ));
        }
        validate_text(&alternative.text, 1_024, "alternative text")?;
    }
    let ask_id = ask_id(&request.lifecycle_key, &request.dedup_key);
    let choices = choices(&ask_id, &alternatives)?;
    let rendered = render(&request.summary, &request.recommendation, &alternatives);
    if rendered.chars().count() > 4096 {
        return Err(AppError::input(
            "ask_too_large",
            "rendered ask exceeds Telegram text limit",
        ));
    }
    let buttons = buttons(&ask_id, &alternatives);
    let paths = Paths::from_env()?;
    let state = state::read(&paths)?;
    let chat_id = state.pairing.ok_or_else(outbound::not_paired)?.chat_id;
    if state.asks.iter().any(|ask| {
        ask.lifecycle_key == request.lifecycle_key
            && ask.state == "open"
            && ask.dedup_key != request.dedup_key
    }) {
        return Err(AppError::new(
            "lifecycle_open",
            "another ask uses this lifecycle",
            ExitClass::Policy,
        ));
    }
    state::update(&paths, |state| {
        state.prune_completed();
        if state.asks.iter().all(|ask| ask.ask_id != ask_id) {
            if state.asks.len() >= 256 {
                return Err(AppError::new(
                    "asks_full",
                    "ask storage is full",
                    ExitClass::Policy,
                ));
            }
            state.asks.push(AskRecord {
                ask_id: ask_id.clone(),
                lifecycle_key: request.lifecycle_key.clone(),
                dedup_key: request.dedup_key.clone(),
                message_id: None,
                state: "open".into(),
                choices: choices.clone(),
            });
        }
        Ok(())
    })?;
    let delivery = outbound::deliver_ask(
        &paths,
        &request.dedup_key,
        &rendered,
        buttons,
        chat_id,
        &ask_id,
    )?;
    Ok(AskReceipt {
        ask_id,
        lifecycle_key: request.lifecycle_key,
        delivery: delivery.into(),
    })
}
fn choices(ask_id: &str, alternatives: &[AskAlternative]) -> Result<Vec<ChoiceRecord>, AppError> {
    let salt = state::random_bytes::<16>()?;
    let mut choices = vec![choice(ask_id, "recommendation", None, &salt)];
    choices.extend(
        alternatives
            .iter()
            .map(|alternative| choice(ask_id, "alternative", Some(&alternative.key), &salt)),
    );
    choices.push(choice(ask_id, "need_context", None, &salt));
    Ok(choices)
}
fn choice(ask_id: &str, kind: &str, key: Option<&str>, salt: &[u8]) -> ChoiceRecord {
    let token = token(ask_id, kind, key);
    ChoiceRecord {
        kind: kind.into(),
        key: key.map(str::to_string),
        token_digest: state::digest(salt, token.as_bytes()),
        salt: state::hex(salt),
    }
}
pub(crate) fn token(ask_id: &str, kind: &str, key: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ask_id.as_bytes());
    hasher.update([0]);
    hasher.update(kind.as_bytes());
    hasher.update([0]);
    hasher.update(key.unwrap_or_default().as_bytes());
    format!("tg1:{}", state::hex(&hasher.finalize()[..24]))
}
fn buttons(ask_id: &str, alternatives: &[AskAlternative]) -> Value {
    let rows = std::iter::once(json!([{"text": "Recommendation", "callback_data": token(ask_id, "recommendation", None)}]))
        .chain(alternatives.iter().map(|alternative| json!([{"text": alternative.label, "callback_data": token(ask_id, "alternative", Some(&alternative.key))}])))
        .chain(std::iter::once(json!([{"text": "Need context", "callback_data": token(ask_id, "need_context", None)}])))
        .collect::<Vec<_>>();
    json!({"inline_keyboard": rows})
}
fn render(summary: &str, recommendation: &str, alternatives: &[AskAlternative]) -> String {
    let mut text = format!("{summary}\n\nRecommendation: {recommendation}");
    if !alternatives.is_empty() {
        text.push_str("\n\nAlternatives:");
        alternatives.iter().for_each(|alternative| {
            text.push_str(&format!("\n- {}: {}", alternative.label, alternative.text));
        });
    }
    text.push_str("\n\nNeed context: request more detail before choosing.");
    text
}

fn ask_id(lifecycle: &str, dedup: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(lifecycle.as_bytes());
    hasher.update([0]);
    hasher.update(dedup.as_bytes());
    format!("ask:{}", state::hex(&hasher.finalize()[..16]))
}

fn validate_summary(summary: &str) -> Result<(), AppError> {
    if summary.is_empty() || summary.split_whitespace().count() > 120 {
        return Err(AppError::input(
            "invalid_summary",
            "summary must contain 1 to 120 words",
        ));
    }
    validate_text(summary, 4096, "summary")
}

fn validate_text(text: &str, max: usize, name: &'static str) -> Result<(), AppError> {
    if text.is_empty() || text.len() > max || text.chars().any(char::is_control) {
        return Err(AppError::input("invalid_text", name));
    }
    Ok(())
}

fn validate_key(key: &str, max: usize, name: &'static str) -> Result<(), AppError> {
    if key.is_empty() || key.len() > max || key.chars().any(char::is_whitespace) {
        return Err(AppError::input("invalid_key", name));
    }
    Ok(())
}
