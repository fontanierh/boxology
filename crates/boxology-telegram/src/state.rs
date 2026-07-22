use crate::{AppError, ExitClass, SCHEMA};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ReplyTarget {
    pub ask_id: Option<String>,
    pub outbound_message_id: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AskRecord {
    pub ask_id: String,
    pub lifecycle_key: String,
    pub dedup_key: String,
    pub message_id: Option<i64>,
    pub state: String,
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
            return Err(AppError::new(
                "corrupt_state",
                "local state is invalid",
                ExitClass::Invariant,
            ));
        }
        if self.events.len() > 1_000 || self.asks.len() > 256 || self.outbound.len() > 1_024 {
            return Err(AppError::new(
                "state_limit",
                "local state exceeds its bound",
                ExitClass::Local,
            ));
        }
        Ok(())
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

#[allow(dead_code)]
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
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths.consumer_lock)
            .map_err(local_io)?;
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
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(local_io)?;
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
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
        Err(error) => return Err(local_io(error)),
    };
    validate_file(path, &metadata)?;
    if metadata.len() > MAX_STATE {
        return Err(AppError::new(
            "state_too_large",
            "local state exceeds its bound",
            ExitClass::Local,
        ));
    }
    let mut file = File::open(path).map_err(local_io)?;
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
    let temp = root.join("state.json.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(local_io)?;
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
    if !path.exists() {
        fs::create_dir_all(path).map_err(local_io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(MODE_HOME)).map_err(local_io)?;
        }
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
    if let Ok(metadata) = fs::symlink_metadata(path) {
        validate_file(path, &metadata)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(local_io)?;
    set_private(&file)
}

fn validate_file(_path: &Path, metadata: &fs::Metadata) -> Result<(), AppError> {
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
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != mode {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("boxology-telegram-state-{nonce}"))
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
