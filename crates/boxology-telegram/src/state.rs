use crate::{AppError, ExitClass, SCHEMA};
use getrandom::fill;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_STATE: u64 = 8 * 1024 * 1024;
const MODE_PRIVATE: u32 = 0o600;
const MODE_HOME: u32 = 0o700;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct State {
    pub schema: u8,
    #[serde(default)]
    pub bot: Option<BotFingerprint>,
    #[serde(default)]
    pub pairing: Option<Pairing>,
    #[serde(default)]
    pub pending_pair: Option<PendingPair>,
    #[serde(default)]
    pub next_offset: i64,
    #[serde(default)]
    pub confirmed_before: i64,
    #[serde(default)]
    pub events: Vec<EventRecord>,
    #[serde(default)]
    pub asks: Vec<AskRecord>,
    #[serde(default)]
    pub outbound: Vec<OutboundRecord>,
    #[serde(default)]
    pub last_receive_at: Option<i64>,
    #[serde(default)]
    pub last_error_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct BotFingerprint {
    pub id: i64,
    pub username: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Pairing {
    pub user_id: i64,
    pub chat_id: i64,
    pub paired_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct PendingPair {
    pub digest: String,
    pub salt: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct EventRecord {
    pub event_id: String,
    pub update_id: i64,
    pub kind: String,
    pub text: String,
    pub received_at: i64,
    pub handled: bool,
    #[serde(default)]
    pub reply_to: Option<ReplyTarget>,
    #[serde(default)]
    pub ask_id: Option<String>,
    #[serde(default)]
    pub lifecycle_key: Option<String>,
    #[serde(default)]
    pub choice: Option<ChoiceRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ReplyTarget {
    pub ask_id: Option<String>,
    pub outbound_message_id: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ChoiceRecord {
    pub kind: String,
    #[serde(default)]
    pub key: Option<String>,
    pub token_digest: String,
    pub salt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AskRecord {
    pub ask_id: String,
    pub lifecycle_key: String,
    pub dedup_key: String,
    pub message_id: Option<i64>,
    pub state: String,
    #[serde(default)]
    pub choices: Vec<ChoiceRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct OutboundRecord {
    pub dedup_key: String,
    pub kind: String,
    pub payload_hash: String,
    pub state: String,
    pub message_id: Option<i64>,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub ask_id: Option<String>,
}

impl State {
    pub(crate) fn prune_completed(&mut self) {
        while self.asks.len() >= 256 {
            let Some(index) = self.asks.iter().position(|ask| {
                ask.state != "open"
                    && !self.events.iter().any(|event| {
                        !event.handled && event.ask_id.as_deref() == Some(ask.ask_id.as_str())
                    })
                    && !self.outbound.iter().any(|outbound| {
                        outbound.ask_id.as_deref() == Some(ask.ask_id.as_str())
                            && matches!(outbound.state.as_str(), "in_flight" | "ambiguous")
                    })
            }) else {
                break;
            };
            self.asks.remove(index);
        }
        while self.outbound.len() >= 1_024 {
            let Some(index) = self.outbound.iter().position(|outbound| {
                outbound.state == "delivered"
                    && !self.events.iter().any(|event| {
                        !event.handled
                            && outbound.event_id.as_deref() == Some(event.event_id.as_str())
                    })
                    && !self.asks.iter().any(|ask| {
                        ask.state == "open"
                            && outbound.ask_id.as_deref() == Some(ask.ask_id.as_str())
                    })
            }) else {
                break;
            };
            self.outbound.remove(index);
        }
    }

    pub(crate) fn prune_handled(&mut self) {
        while self.events.len() >= 1_000
            || serde_json::to_vec(&self.events).is_ok_and(|bytes| bytes.len() >= 8 * 1024 * 1024)
        {
            let Some(index) = self.events.iter().position(|event| {
                event.handled
                    && !self.outbound.iter().any(|outbound| {
                        outbound.event_id.as_deref() == Some(event.event_id.as_str())
                            && matches!(outbound.state.as_str(), "in_flight" | "ambiguous")
                    })
                    && event.ask_id.as_ref().is_none_or(|ask_id| {
                        self.asks
                            .iter()
                            .all(|ask| &ask.ask_id != ask_id || ask.state != "open")
                    })
            }) else {
                break;
            };
            self.events.remove(index);
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            ..Self::empty()
        }
    }
}

impl State {
    fn empty() -> Self {
        Self {
            schema: SCHEMA,
            bot: None,
            pairing: None,
            pending_pair: None,
            next_offset: 0,
            confirmed_before: 0,
            events: Vec::new(),
            asks: Vec::new(),
            outbound: Vec::new(),
            last_receive_at: None,
            last_error_code: None,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), AppError> {
        if self.schema != SCHEMA || self.next_offset < 0 || self.confirmed_before < 0 {
            return Err(invalid_state());
        }
        if self.confirmed_before > self.next_offset {
            return Err(invalid_state());
        }
        if self.events.len() > 1_000 || self.asks.len() > 256 || self.outbound.len() > 1_024 {
            return Err(AppError::new(
                "state_limit",
                "local state exceeds its bound",
                ExitClass::Local,
            ));
        }
        if self.pairing.is_some() && self.bot.is_none() {
            return Err(invalid_state());
        }
        if self.pending_pair.is_some() && self.bot.is_none() {
            return Err(invalid_state());
        }
        if self.pairing.is_some() && self.pending_pair.is_some() {
            return Err(invalid_state());
        }
        if let Some(bot) = &self.bot
            && (bot.id <= 0 || !valid_username(&bot.username))
        {
            return Err(invalid_state());
        }
        if let Some(pairing) = &self.pairing
            && (pairing.user_id <= 0
                || pairing.chat_id <= 0
                || pairing.user_id != pairing.chat_id
                || pairing.paired_at < 0)
        {
            return Err(invalid_state());
        }
        if let Some(pending) = &self.pending_pair
            && (!valid_hex(&pending.digest, 64)
                || !valid_hex(&pending.salt, 32)
                || pending.expires_at <= 0)
        {
            return Err(invalid_state());
        }
        if self.last_receive_at.is_some_and(|value| value < 0)
            || self
                .last_error_code
                .as_deref()
                .is_some_and(|value| !valid_code(value))
        {
            return Err(invalid_state());
        }

        let mut event_ids = HashSet::new();
        let mut update_ids = HashSet::new();
        for event in &self.events {
            if !event_ids.insert(event.event_id.as_str())
                || !update_ids.insert(event.update_id)
                || !valid_event(event, self.next_offset)
            {
                return Err(invalid_state());
            }
        }
        let mut ask_ids = HashSet::new();
        let mut lifecycle_keys = HashSet::new();
        for ask in &self.asks {
            if !ask_ids.insert(ask.ask_id.as_str()) || !valid_ask(ask) {
                return Err(invalid_state());
            }
            if ask.state == "open" && !lifecycle_keys.insert(ask.lifecycle_key.as_str()) {
                return Err(invalid_state());
            }
        }
        let mut outbound_keys = HashSet::new();
        for outbound in &self.outbound {
            if !outbound_keys.insert(outbound.dedup_key.as_str()) || !valid_outbound(outbound) {
                return Err(invalid_state());
            }
            let live = matches!(
                outbound.state.as_str(),
                "in_flight" | "ambiguous" | "retryable"
            );
            if live
                && (outbound
                    .event_id
                    .as_deref()
                    .is_some_and(|event_id| !event_ids.contains(event_id))
                    || outbound
                        .ask_id
                        .as_deref()
                        .is_some_and(|ask_id| !ask_ids.contains(ask_id)))
            {
                return Err(invalid_state());
            }
        }
        for event in &self.events {
            if !event.handled
                && event
                    .ask_id
                    .as_deref()
                    .is_some_and(|ask_id| !ask_ids.contains(ask_id))
            {
                return Err(invalid_state());
            }
        }
        Ok(())
    }
}

fn invalid_state() -> AppError {
    AppError::new(
        "corrupt_state",
        "local state is invalid",
        ExitClass::Invariant,
    )
}

fn valid_username(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_key(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_whitespace)
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_ask_id(value: &str) -> bool {
    value
        .strip_prefix("ask:")
        .is_some_and(|value| valid_hex(value, 32))
}

fn event_parts(value: &str) -> Option<(i64, i64)> {
    let mut parts = value.split(':');
    let prefix = parts.next()?;
    let update_id = parts.next()?.parse().ok()?;
    let message_id = parts.next()?.parse().ok()?;
    (prefix == "tg" && parts.next().is_none() && update_id >= 0 && message_id > 0)
        .then_some((update_id, message_id))
}

fn valid_event(event: &EventRecord, next_offset: i64) -> bool {
    let Some((update_id, message_id)) = event_parts(&event.event_id) else {
        return false;
    };
    if update_id != event.update_id
        || event.update_id >= next_offset
        || event.received_at < 0
        || event.text.chars().count() > 4096
        || event
            .ask_id
            .as_deref()
            .is_some_and(|ask_id| !valid_ask_id(ask_id))
        || event
            .lifecycle_key
            .as_deref()
            .is_some_and(|key| !valid_key(key, 128))
    {
        return false;
    }
    if let Some(target) = &event.reply_to
        && (target.outbound_message_id.is_none_or(|id| id <= 0)
            || target
                .ask_id
                .as_deref()
                .is_some_and(|ask_id| !valid_ask_id(ask_id)))
    {
        return false;
    }
    let kind_valid = match event.kind.as_str() {
        "text" => {
            !event.text.is_empty()
                && event.ask_id.is_none()
                && event.lifecycle_key.is_none()
                && event.choice.is_none()
        }
        "ask_reply" => {
            !event.text.is_empty()
                && event.ask_id.is_some()
                && event.lifecycle_key.is_some()
                && event.reply_to.is_some()
                && event.choice.is_none()
        }
        "ask_choice" => {
            event.text.is_empty()
                && event.ask_id.is_some()
                && event.lifecycle_key.is_some()
                && event.reply_to.is_some()
                && event.choice.as_ref().is_some_and(valid_choice)
        }
        _ => false,
    };
    kind_valid && message_id > 0
}

fn valid_choice(choice: &ChoiceRecord) -> bool {
    let key_valid = match choice.kind.as_str() {
        "recommendation" | "need_context" => choice.key.is_none(),
        "alternative" => choice.key.as_deref().is_some_and(|key| valid_key(key, 32)),
        _ => false,
    };
    key_valid && valid_hex(&choice.token_digest, 64) && valid_hex(&choice.salt, 32)
}

fn valid_ask(ask: &AskRecord) -> bool {
    if !valid_ask_id(&ask.ask_id)
        || !valid_key(&ask.lifecycle_key, 128)
        || !valid_key(&ask.dedup_key, 128)
        || !matches!(ask.state.as_str(), "open" | "answered")
        || ask.message_id.is_some_and(|id| id <= 0)
        || !(2..=6).contains(&ask.choices.len())
        || ask.choices.iter().any(|choice| !valid_choice(choice))
    {
        return false;
    }
    let mut choices = HashSet::new();
    let mut recommendations = 0;
    let mut context = 0;
    let mut alternatives = 0;
    for choice in &ask.choices {
        if !choices.insert((choice.kind.as_str(), choice.key.as_deref())) {
            return false;
        }
        match choice.kind.as_str() {
            "recommendation" => recommendations += 1,
            "need_context" => context += 1,
            "alternative" => alternatives += 1,
            _ => return false,
        }
    }
    recommendations == 1 && context == 1 && alternatives <= 4
}

fn valid_outbound(outbound: &OutboundRecord) -> bool {
    if !valid_key(&outbound.dedup_key, 128)
        || !valid_hex(&outbound.payload_hash, 64)
        || outbound
            .event_id
            .as_deref()
            .is_some_and(|event_id| event_parts(event_id).is_none())
        || outbound
            .ask_id
            .as_deref()
            .is_some_and(|ask_id| !valid_ask_id(ask_id))
    {
        return false;
    }
    match outbound.kind.as_str() {
        "send" if outbound.event_id.is_none() && outbound.ask_id.is_none() => {}
        "reply" if outbound.event_id.is_some() => {}
        "ask" if outbound.ask_id.is_some() && outbound.event_id.is_none() => {}
        _ => return false,
    }
    match outbound.state.as_str() {
        "delivered" => outbound.message_id.is_some_and(|id| id > 0),
        "in_flight" | "ambiguous" | "retryable" => outbound.message_id.is_none(),
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Paths {
    pub root: PathBuf,
    state: PathBuf,
    state_lock: PathBuf,
    consumer_lock: PathBuf,
}

impl Paths {
    pub(crate) fn from_env() -> Result<Self, AppError> {
        let root = if let Some(path) = std::env::var_os("BOXOLOGY_TELEGRAM_HOME") {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(AppError::new(
                    "unsafe_state_home",
                    "state home must be absolute",
                    ExitClass::Local,
                ));
            }
            path
        } else if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
            PathBuf::from(path).join("boxology/telegram-coordinator")
        } else {
            let home = std::env::var_os("HOME").ok_or_else(|| {
                AppError::new(
                    "state_home_missing",
                    "state home is unavailable",
                    ExitClass::Local,
                )
            })?;
            PathBuf::from(home).join(".local/state/boxology/telegram-coordinator")
        };
        Ok(Self::at(root))
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn for_test(root: PathBuf) -> Self {
        Self::at(root)
    }

    fn at(root: PathBuf) -> Self {
        Self {
            state: root.join("state.json"),
            state_lock: root.join("state.lock"),
            consumer_lock: root.join("consumer.lock"),
            root,
        }
    }

    pub(crate) fn prepare(&self) -> Result<(), AppError> {
        ensure_home(&self.root)?;
        ensure_lock_file(&self.state_lock)?;
        ensure_lock_file(&self.consumer_lock)
    }
}

pub(crate) fn read(paths: &Paths) -> Result<State, AppError> {
    paths.prepare()?;
    let lock = StateLock::acquire(&paths.state_lock)?;
    let state = read_unlocked(&paths.state);
    drop(lock);
    state
}

pub(crate) fn open_protected(
    path: &Path,
    read: bool,
    write: bool,
    create: bool,
) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(read).write(write);
    if create {
        options.create(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(NO_FOLLOW);
    }
    options.open(path)
}

pub(crate) fn validate_ancestors(path: &Path) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let mut current = PathBuf::new();
    for component in parent.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "parent path component",
                ));
            }
            Component::Normal(part) => current.push(part),
        }
        if current == Path::new("/") || current.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "unsafe path ancestor",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) fn consumer_locked(paths: &Paths) -> Result<bool, AppError> {
    paths.prepare()?;
    let file = open_protected(&paths.consumer_lock, true, true, false).map_err(local_io)?;
    validate_file(&file.metadata().map_err(local_io)?)?;
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            Ok(false)
        }
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(error)) => Err(local_io(error)),
    }
}
pub(crate) fn update<T>(
    paths: &Paths,
    change: impl FnOnce(&mut State) -> Result<T, AppError>,
) -> Result<T, AppError> {
    paths.prepare()?;
    let _lock = StateLock::acquire(&paths.state_lock)?;
    let mut state = read_unlocked(&paths.state)?;
    let result = change(&mut state)?;
    state.validate()?;
    write_unlocked(&paths.root, &paths.state, &state)?;
    Ok(result)
}

#[allow(dead_code)]
pub(crate) struct ConsumerLock(File);

#[allow(dead_code)]
impl ConsumerLock {
    pub(crate) fn acquire(paths: &Paths) -> Result<Self, AppError> {
        paths.prepare()?;
        let file = open_protected(&paths.consumer_lock, true, true, false).map_err(local_io)?;
        validate_file(&file.metadata().map_err(local_io)?)?;
        file.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => AppError::new(
                "consumer_locked",
                "another local consumer holds the lock",
                ExitClass::Conflict,
            ),
            std::fs::TryLockError::Error(error) => local_io(error),
        })?;
        Ok(Self(file))
    }
}

impl Drop for ConsumerLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

struct StateLock(File);

impl StateLock {
    fn acquire(path: &Path) -> Result<Self, AppError> {
        let file = open_protected(path, true, true, false).map_err(local_io)?;
        validate_file(&file.metadata().map_err(local_io)?)?;
        file.lock().map_err(local_io)?;
        Ok(Self(file))
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn read_unlocked(path: &Path) -> Result<State, AppError> {
    validate_ancestors(path).map_err(|_| {
        AppError::new(
            "unsafe_state_file",
            "state path has an unsafe ancestor",
            ExitClass::Local,
        )
    })?;
    let mut file = match open_protected(path, true, false, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(State::default());
        }
        Err(error) => return Err(local_io(error)),
    };
    let metadata = file.metadata().map_err(local_io)?;
    validate_file(&metadata)?;
    if metadata.len() > MAX_STATE {
        return Err(AppError::new(
            "state_too_large",
            "local state exceeds its bound",
            ExitClass::Local,
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes).map_err(local_io)?;
    let state: State = serde_json::from_slice(&bytes).map_err(|_| {
        AppError::new(
            "corrupt_state",
            "local state is invalid",
            ExitClass::Invariant,
        )
    })?;
    state.validate()?;
    Ok(state)
}

#[allow(dead_code)]
fn write_unlocked(root: &Path, path: &Path, state: &State) -> Result<(), AppError> {
    let bytes = serde_json::to_vec(state).map_err(|_| {
        AppError::new(
            "state_encode",
            "local state could not be encoded",
            ExitClass::Local,
        )
    })?;
    if bytes.len() as u64 > MAX_STATE {
        return Err(AppError::new(
            "state_too_large",
            "local state exceeds its bound",
            ExitClass::Local,
        ));
    }
    let temp = root.join(format!("state.json.{}.tmp", unique_suffix()?));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(NO_FOLLOW);
    }
    let mut file = options.open(&temp).map_err(local_io)?;
    set_private(&file)?;
    file.write_all(&bytes).map_err(local_io)?;
    file.sync_all().map_err(local_io)?;
    drop(file);
    fs::rename(&temp, path).map_err(local_io)?;
    File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(local_io)
}

fn ensure_home(path: &Path) -> Result<(), AppError> {
    validate_ancestors(path).map_err(|_| {
        AppError::new(
            "unsafe_state_home",
            "state home has an unsafe ancestor",
            ExitClass::Local,
        )
    })?;
    let existing = fs::symlink_metadata(path);
    if matches!(&existing, Err(error) if error.kind() == std::io::ErrorKind::NotFound) {
        fs::create_dir_all(path).map_err(local_io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(MODE_HOME)).map_err(local_io)?;
        }
        validate_ancestors(path).map_err(|_| {
            AppError::new(
                "unsafe_state_home",
                "state home has an unsafe ancestor",
                ExitClass::Local,
            )
        })?;
    }
    let metadata = fs::symlink_metadata(path).map_err(local_io)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(AppError::new(
            "unsafe_state_home",
            "state home is not a private directory",
            ExitClass::Local,
        ));
    }
    validate_owner_and_mode(&metadata, MODE_HOME)
}

fn ensure_lock_file(path: &Path) -> Result<(), AppError> {
    validate_ancestors(path).map_err(|_| {
        AppError::new(
            "unsafe_state_file",
            "lock path has an unsafe ancestor",
            ExitClass::Local,
        )
    })?;
    let existed = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            validate_file(&metadata)?;
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(local_io(error)),
    };
    let file = open_protected(path, true, true, true).map_err(local_io)?;
    if !existed {
        set_private(&file)?;
    }
    let metadata = file.metadata().map_err(local_io)?;
    validate_file(&metadata)?;
    Ok(())
}

fn validate_file(metadata: &fs::Metadata) -> Result<(), AppError> {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(AppError::new(
            "unsafe_state_file",
            "state path is not a private file",
            ExitClass::Local,
        ));
    }
    validate_owner_and_mode(metadata, MODE_PRIVATE)
}

fn validate_owner_and_mode(metadata: &fs::Metadata, mode: u32) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != effective_uid() || metadata.permissions().mode() & 0o777 != mode {
            return Err(AppError::new(
                "unsafe_state_permissions",
                "state permissions are unsafe",
                ExitClass::Local,
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = (metadata, mode);
    Ok(())
}

fn set_private(file: &File) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(MODE_PRIVATE))
            .map_err(local_io)?;
    }
    Ok(())
}

fn local_io(_: std::io::Error) -> AppError {
    AppError::new(
        "local_state",
        "local state is unavailable",
        ExitClass::Local,
    )
}

#[allow(dead_code)]
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_secs() as i64)
}

#[allow(dead_code)]
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], AppError> {
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

#[allow(dead_code)]
pub(crate) fn digest(salt: &[u8], value: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(value);
    hex(&hasher.finalize())
}

#[allow(dead_code)]
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[allow(dead_code)]
pub(crate) fn decode_hex(text: &str) -> Option<Vec<u8>> {
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

fn unique_suffix() -> Result<String, AppError> {
    Ok(hex(&random_bytes::<16>()?))
}

#[cfg(unix)]
unsafe extern "C" {
    fn geteuid() -> u32;
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    unsafe { geteuid() }
}

#[cfg(target_os = "linux")]
const NO_FOLLOW: i32 = 0o400000;

#[cfg(target_os = "macos")]
const NO_FOLLOW: i32 = 0x0100;

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
const NO_FOLLOW: i32 = 0;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        fs::canonicalize(std::env::temp_dir())
            .expect("canonical temp directory")
            .join(format!("boxology-telegram-state-{nonce}"))
    }

    #[test]
    fn update_persists_state_and_consumer_lock_is_exclusive() {
        let root = test_root();
        let paths = Paths::for_test(root.clone());
        let result = update(&paths, |state| {
            state.next_offset = 4;
            Ok(state.next_offset)
        })
        .expect("state update");
        assert_eq!(result, 4);
        assert_eq!(read(&paths).expect("state read").next_offset, 4);

        let first = ConsumerLock::acquire(&paths).expect("first consumer lock");
        let second = match ConsumerLock::acquire(&paths) {
            Ok(_) => panic!("second lock must fail"),
            Err(error) => error,
        };
        assert_eq!(second.code, "consumer_locked");
        drop(first);
        fs::remove_dir_all(root).expect("remove test state");
    }
}
