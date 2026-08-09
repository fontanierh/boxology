use crate::state;
use crate::{AppError, ExitClass};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const ORIGIN: &str = "https://api.telegram.org";
const MAX_BODY: u64 = 4 * 1024 * 1024;
const MAX_TOKEN: usize = 512;

pub(crate) struct Api {
    client: Client,
    token: String,
    origin: String,
}

#[derive(Debug)]
pub(crate) struct ApiError {
    pub code: &'static str,
    pub retry_after: Option<u64>,
    pub exit: ExitClass,
    pub ambiguous: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct User {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Message {
    pub message_id: i64,
    pub from: Option<User>,
    pub chat: Chat,
    pub text: Option<String>,
    pub reply_to_message: Option<Box<Message>>,
    #[serde(default)]
    pub forward_origin: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CallbackQuery {
    pub id: String,
    pub from: User,
    pub message: Option<Message>,
    pub data: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct BotInfo {
    pub id: i64,
    pub is_bot: bool,
    pub username: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WebhookInfo {
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct SentMessage {
    pub message_id: i64,
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
    error_code: Option<u16>,
    parameters: Option<ApiParameters>,
}

#[derive(Deserialize)]
struct ApiParameters {
    retry_after: Option<u64>,
}

impl Api {
    pub(crate) fn production(token: String) -> Result<Self, AppError> {
        Self::build(token, ORIGIN.to_string())
    }

    #[cfg(test)]
    pub(crate) fn test(token: String, origin: String) -> Result<Self, AppError> {
        Self::build(token, origin)
    }

    fn build(token: String, origin: String) -> Result<Self, AppError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .https_only(origin == ORIGIN)
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| {
                AppError::new(
                    "api_client",
                    "Telegram client is unavailable",
                    ExitClass::Local,
                )
            })?;
        Ok(Self {
            client,
            token,
            origin,
        })
    }

    pub(crate) fn get_me(&self) -> Result<BotInfo, ApiError> {
        self.call("getMe", json!({}), false)
    }

    pub(crate) fn webhook_info(&self) -> Result<WebhookInfo, ApiError> {
        self.call("getWebhookInfo", json!({}), false)
    }

    pub(crate) fn get_updates(&self, offset: i64, timeout: u64) -> Result<Vec<Update>, ApiError> {
        self.call(
            "getUpdates",
            json!({"offset": offset, "timeout": timeout}),
            false,
        )
    }

    pub(crate) fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_to: Option<i64>,
        buttons: Option<Value>,
    ) -> Result<SentMessage, ApiError> {
        let mut params = json!({"chat_id": chat_id, "text": text});
        if let Some(message_id) = reply_to {
            params["reply_parameters"] = json!({"message_id": message_id});
        }
        if let Some(buttons) = buttons {
            params["reply_markup"] = buttons;
        }
        self.call("sendMessage", params, true)
    }

    pub(crate) fn answer_callback(&self, callback_id: &str) -> Result<bool, ApiError> {
        self.call(
            "answerCallbackQuery",
            json!({"callback_query_id": callback_id}),
            true,
        )
    }

    fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
        write: bool,
    ) -> Result<T, ApiError> {
        let url = format!("{}/bot{}/{}", self.origin, self.token, method);
        let response = self
            .client
            .post(url)
            .json(&params)
            .send()
            .map_err(|_| ApiError::transport(write))?;
        let status = response.status().as_u16();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_BODY)
        {
            return Err(ApiError::malformed(write));
        }
        let mut body = Vec::new();
        response
            .take(MAX_BODY + 1)
            .read_to_end(&mut body)
            .map_err(|_| ApiError::malformed(write))?;
        if body.len() as u64 > MAX_BODY {
            return Err(ApiError::malformed(write));
        }
        let envelope: Envelope<T> =
            serde_json::from_slice(&body).map_err(|_| ApiError::malformed(write))?;
        if envelope.ok {
            return envelope.result.ok_or_else(|| ApiError::malformed(write));
        }
        let error_code = envelope.error_code.unwrap_or(status);
        Err(ApiError::telegram(error_code, envelope.parameters, write))
    }
}

pub(crate) fn for_commands(token: String) -> Result<Api, AppError> {
    #[cfg(test)]
    if let Some(origin) = TEST_ORIGIN
        .get()
        .and_then(|value| value.lock().ok())
        .and_then(|value| value.clone())
    {
        return Api::test(token, origin);
    }
    Api::production(token)
}

#[cfg(test)]
static TEST_ORIGIN: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
    std::sync::OnceLock::new();

#[cfg(test)]
pub(crate) fn set_test_origin(origin: Option<String>) {
    let lock = TEST_ORIGIN.get_or_init(|| std::sync::Mutex::new(None));
    *lock.lock().expect("test origin lock") = origin;
}

pub(crate) fn load_token() -> Result<String, AppError> {
    let file = std::env::var_os("BOXOLOGY_TELEGRAM_BOT_TOKEN_FILE");
    let environment = std::env::var_os("BOXOLOGY_TELEGRAM_BOT_TOKEN");
    match (file, environment) {
        (Some(_), Some(_)) => Err(AppError::new(
            "token_sources",
            "configure one Telegram token source",
            ExitClass::Local,
        )),
        (None, None) => Err(AppError::new(
            "token_missing",
            "Telegram bot token is not configured",
            ExitClass::Local,
        )),
        (Some(path), None) => load_token_file(Path::new(&path)),
        (None, Some(token)) => validate_token(token.to_string_lossy().into_owned()),
    }
}

fn load_token_file(path: &Path) -> Result<String, AppError> {
    if !path.is_absolute() {
        return Err(AppError::new(
            "unsafe_token_file",
            "token file path must be absolute",
            ExitClass::Local,
        ));
    }
    state::validate_ancestors(path).map_err(|_| {
        AppError::new(
            "unsafe_token_file",
            "Telegram token file has an unsafe ancestor",
            ExitClass::Local,
        )
    })?;
    let path_metadata = fs::symlink_metadata(path).map_err(|_| {
        AppError::new(
            "token_file",
            "Telegram token file is unavailable",
            ExitClass::Local,
        )
    })?;
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(AppError::new(
            "unsafe_token_file",
            "Telegram token file is unsafe",
            ExitClass::Local,
        ));
    }
    let mut file = state::open_protected(path, true, false, false).map_err(|_| {
        AppError::new(
            "token_file",
            "Telegram token file is unavailable",
            ExitClass::Local,
        )
    })?;
    let metadata = file.metadata().map_err(|_| {
        AppError::new(
            "token_file",
            "Telegram token file is unavailable",
            ExitClass::Local,
        )
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(AppError::new(
            "unsafe_token_file",
            "Telegram token file is unsafe",
            ExitClass::Local,
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != effective_uid() || metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(AppError::new(
                "unsafe_token_file",
                "Telegram token file is unsafe",
                ExitClass::Local,
            ));
        }
    }
    if metadata.len() > (MAX_TOKEN + 2) as u64 {
        return Err(AppError::new(
            "token_invalid",
            "Telegram token is invalid",
            ExitClass::Local,
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes).map_err(|_| {
        AppError::new(
            "token_file",
            "Telegram token file is unavailable",
            ExitClass::Local,
        )
    })?;
    let token = String::from_utf8(bytes).map_err(|_| {
        AppError::new(
            "token_file",
            "Telegram token file is invalid",
            ExitClass::Local,
        )
    })?;
    validate_token(token)
}

#[cfg(unix)]
unsafe extern "C" {
    fn geteuid() -> u32;
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    unsafe { geteuid() }
}

fn validate_token(mut token: String) -> Result<String, AppError> {
    if token.ends_with('\n') {
        token.pop();
        if token.ends_with('\r') {
            token.pop();
        }
    }
    if token.is_empty() || token.len() > MAX_TOKEN || token.chars().any(char::is_whitespace) {
        return Err(AppError::new(
            "token_invalid",
            "Telegram token is invalid",
            ExitClass::Local,
        ));
    }
    Ok(token)
}

impl ApiError {
    fn transport(write: bool) -> Self {
        Self {
            code: "telegram_transport",
            retry_after: None,
            exit: if write {
                ExitClass::Ambiguous
            } else {
                ExitClass::Transient
            },
            ambiguous: write,
        }
    }

    fn malformed(write: bool) -> Self {
        Self {
            code: "telegram_response",
            retry_after: None,
            exit: if write {
                ExitClass::Ambiguous
            } else {
                ExitClass::Transient
            },
            ambiguous: write,
        }
    }

    fn telegram(code: u16, parameters: Option<ApiParameters>, _write: bool) -> Self {
        let retry_after = parameters.and_then(|parameters| parameters.retry_after);
        let exit = if code == 429 {
            ExitClass::Transient
        } else if code == 409 {
            ExitClass::Conflict
        } else {
            ExitClass::Permanent
        };
        Self {
            code: if code == 429 {
                "telegram_rate_limited"
            } else if code == 409 {
                "telegram_conflict"
            } else if code == 401 || code == 403 {
                "telegram_auth"
            } else {
                "telegram_rejected"
            },
            retry_after,
            exit,
            ambiguous: false,
        }
    }
}
