use crate::api;
use crate::state::{self, BotFingerprint, Pairing, Paths, PendingPair};
use crate::{AppError, ExitClass, api_error};
use getrandom::fill;
use sha2::{Digest, Sha256};

pub(crate) struct BeginCommand {
    pub nonce_ttl_seconds: Option<u64>,
}

pub(crate) struct BotIdentity {
    pub id: i64,
    pub username: String,
}

pub(crate) struct BeginReceipt {
    pub deep_link: String,
    pub expires_at: i64,
    pub bot: BotIdentity,
}

pub(crate) struct CompleteCommand {
    pub timeout_seconds: Option<u64>,
}

#[derive(Clone, Copy)]
pub(crate) enum Confirmation {
    Delivered,
    Ambiguous,
    NotAttempted,
}

pub(crate) struct CompleteReceipt {
    pub user_id: i64,
    pub chat_id: i64,
    pub paired_at: i64,
    pub confirmation: Confirmation,
}

pub(crate) struct RevokeReceipt {
    pub pairing_revoked: bool,
}

pub(crate) fn begin_typed(command: BeginCommand) -> Result<BeginReceipt, AppError> {
    let ttl = command.nonce_ttl_seconds.unwrap_or(600);
    if !(60..=600).contains(&ttl) {
        return Err(AppError::input(
            "invalid_nonce_ttl",
            "pairing lifetime is out of bounds",
        ));
    }
    let paths = Paths::from_env()?;
    if state::read(&paths)?.pairing.is_some() {
        return Err(AppError::new(
            "already_paired",
            "a pairing already exists",
            ExitClass::Policy,
        ));
    }
    let token = api::load_token()?;
    let api = api::for_commands(token)?;
    let bot = api.get_me().map_err(api_error)?;
    let username = bot.username.clone().ok_or_else(|| {
        AppError::new(
            "bot_invalid",
            "Telegram bot identity is invalid",
            ExitClass::Permanent,
        )
    })?;
    if !bot.is_bot
        || bot.id <= 0
        || username.is_empty()
        || username.len() > 64
        || !valid_username(&username)
    {
        return Err(AppError::new(
            "bot_invalid",
            "Telegram bot identity is invalid",
            ExitClass::Permanent,
        ));
    }
    let webhook = api.webhook_info().map_err(api_error)?;
    if !webhook.url.is_empty() {
        return Err(AppError::new(
            "webhook_configured",
            "Telegram webhook blocks polling",
            ExitClass::Conflict,
        ));
    }
    let now = state::now();
    let nonce = random_bytes::<16>()?;
    let salt = random_bytes::<16>()?;
    let payload = hex(&nonce);
    let pending = PendingPair {
        digest: digest(&salt, payload.as_bytes()),
        salt: hex(&salt),
        expires_at: now.saturating_add(ttl as i64),
    };
    state::update(&paths, |state| {
        if state.pairing.is_some() {
            return Err(AppError::new(
                "already_paired",
                "a pairing already exists",
                ExitClass::Policy,
            ));
        }
        if state.bot.as_ref().is_some_and(|old| old.id != bot.id) {
            return Err(AppError::new(
                "bot_mismatch",
                "configured bot differs from local state",
                ExitClass::Policy,
            ));
        }
        state.bot = Some(BotFingerprint {
            id: bot.id,
            username: username.clone(),
        });
        state.pending_pair = Some(pending.clone());
        Ok(())
    })?;
    Ok(BeginReceipt {
        deep_link: format!("https://t.me/{username}?start={payload}"),
        expires_at: pending.expires_at,
        bot: BotIdentity {
            id: bot.id,
            username,
        },
    })
}

pub(crate) fn complete_typed(command: CompleteCommand) -> Result<CompleteReceipt, AppError> {
    let timeout = command.timeout_seconds.unwrap_or(30);
    if timeout > 50 {
        return Err(AppError::input(
            "invalid_timeout",
            "poll timeout is out of bounds",
        ));
    }
    let paths = Paths::from_env()?;
    let _consumer = state::ConsumerLock::acquire(&paths)?;
    let before = state::read(&paths)?;
    let pending = before.pending_pair.clone().ok_or_else(|| {
        AppError::new(
            "pairing_not_started",
            "pairing has not started",
            ExitClass::Policy,
        )
    })?;
    if pending.expires_at <= state::now() {
        return Err(AppError::new(
            "pairing_expired",
            "pairing request expired",
            ExitClass::Policy,
        ));
    }
    let token = api::load_token()?;
    let api = api::for_commands(token)?;
    let updates = api
        .get_updates(before.next_offset, timeout)
        .map_err(api_error)?;
    validate_update_order(&updates, before.next_offset)?;
    let max_id = updates.last().map(|update| update.update_id);
    let selected = updates
        .iter()
        .find_map(|update| pairing_candidate(update, &pending));
    let next_offset = match max_id {
        Some(id) => id.checked_add(1).ok_or_else(|| {
            AppError::new(
                "offset_overflow",
                "Telegram offset is invalid",
                ExitClass::Invariant,
            )
        })?,
        None => before.next_offset,
    };
    let paired_at = selected.map(|_| state::now());
    state::update(&paths, |state| {
        if state
            .pending_pair
            .as_ref()
            .map(|value| value.digest.as_str())
            != Some(pending.digest.as_str())
        {
            return Err(AppError::new(
                "pairing_changed",
                "pairing changed during the operation",
                ExitClass::Policy,
            ));
        }
        state.next_offset = next_offset;
        if let Some((user_id, chat_id)) = selected {
            state.pairing = Some(Pairing {
                user_id,
                chat_id,
                paired_at: paired_at.expect("selected pairing has timestamp"),
            });
            state.pending_pair = None;
        }
        Ok(())
    })?;
    let Some((user_id, chat_id)) = selected else {
        return Err(AppError::new(
            "pairing_not_found",
            "no matching private pairing message was received",
            ExitClass::Policy,
        ));
    };
    let confirmation = match api.send_message(chat_id, "Telegram pairing confirmed.", None, None) {
        Ok(_) => Confirmation::Delivered,
        Err(error) if error.ambiguous => Confirmation::Ambiguous,
        Err(_) => Confirmation::NotAttempted,
    };
    Ok(CompleteReceipt {
        user_id,
        chat_id,
        paired_at: paired_at.expect("selected pairing has timestamp"),
        confirmation,
    })
}

pub(crate) fn revoke_typed() -> Result<RevokeReceipt, AppError> {
    let paths = Paths::from_env()?;
    let revoked = state::update(&paths, |state| {
        let revoked = state.pairing.take().is_some();
        state.pending_pair = None;
        state.events.clear();
        state.asks.clear();
        state.outbound.clear();
        Ok(revoked)
    })?;
    Ok(RevokeReceipt {
        pairing_revoked: revoked,
    })
}

fn pairing_candidate(update: &api::Update, pending: &PendingPair) -> Option<(i64, i64)> {
    let message = update.message.as_ref()?;
    let user = message.from.as_ref()?;
    if user.is_bot
        || user.id <= 0
        || message.chat.id <= 0
        || message.chat.id != user.id
        || message.chat.kind != "private"
        || message.text.as_deref()?.split_once(' ').is_none()
    {
        return None;
    }
    let (command, payload) = message.text.as_deref()?.split_once(' ')?;
    let salt = decode_hex(&pending.salt)?;
    (command == "/start" && digest(&salt, payload.as_bytes()) == pending.digest)
        .then_some((user.id, message.chat.id))
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

fn valid_username(username: &str) -> bool {
    username
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn random_bytes<const N: usize>() -> Result<[u8; N], AppError> {
    let mut bytes = [0_u8; N];
    fill(&mut bytes).map_err(|_| {
        AppError::new(
            "entropy",
            "secure randomness is unavailable",
            ExitClass::Local,
        )
    })?;
    Ok(bytes)
}

fn digest(salt: &[u8], value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(value);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    text.len()
        .is_multiple_of(2)
        .then(|| {
            text.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
                .collect::<Option<Vec<_>>>()
        })
        .flatten()
}
