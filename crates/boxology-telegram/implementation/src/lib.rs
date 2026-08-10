use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::io;

#[allow(dead_code)]
mod api;
mod ask;
mod listen;
mod outbound;
mod pairing;
mod receive;
mod state;

boxology::contract! {
    pub struct SendRequest {
        pub text: String,
        pub dedup_key: String,
    }

    pub struct AskAlternative {
        pub key: String,
        pub label: String,
        pub text: String,
    }

    pub struct AskRequest {
        pub summary: String,
        pub recommendation: String,
        pub alternatives: Option<Vec<AskAlternative>>,
        pub lifecycle_key: String,
        pub dedup_key: String,
    }

    pub enum FailureClass {
        Input,
        Authorization,
        Conflict,
        Local,
        Policy,
        Transient,
        Permanent,
        Ambiguous,
        Invariant,
    }

    pub struct OperationError {
        pub code: String,
        pub message: String,
        pub retryable: bool,
        pub retry_after_seconds: Option<u64>,
        pub class: FailureClass,
    }

    pub struct DeliveryReceipt {
        pub dedup_key: String,
        pub message_id: i64,
        pub deduplicated: bool,
    }

    pub struct DeliveryOutcome {
        pub delivery: Option<DeliveryReceipt>,
        pub error: Option<OperationError>,
    }

    pub struct AskReceipt {
        pub ask_id: String,
        pub lifecycle_key: String,
        pub delivery: DeliveryReceipt,
    }

    pub struct AskOutcome {
        pub ask: Option<AskReceipt>,
        pub error: Option<OperationError>,
    }

    #[error]
    pub enum SendTextError {
        Input,
        Authorization,
        Conflict,
        Local,
        Policy,
        Transient,
        Permanent,
        Ambiguous,
        Invariant,
    }

    #[capability]
    pub async fn send_text(text: String) -> Result<i64, SendTextError>;

    #[capability(idempotency = inherent)]
    pub async fn send(request: SendRequest) -> Result<DeliveryOutcome, SendTextError>;

    #[capability(idempotency = inherent)]
    pub async fn ask(request: AskRequest) -> Result<AskOutcome, SendTextError>;
}

pub struct TelegramService;

#[boxology::implementation]
impl TelegramService {
    pub async fn send_text(
        &self,
        _context: boxology::CallContext,
        text: String,
    ) -> Result<i64, SendTextError> {
        if !enabled() {
            return Err(SendTextError::Authorization);
        }
        let dedup_key = state::hex(&state::random_bytes::<16>().map_err(map_send_text_error)?);
        outbound::send_typed(outbound::SendCommand { text, dedup_key })
            .map(|receipt| receipt.message_id)
            .map_err(map_send_text_error)
    }

    pub async fn send(
        &self,
        _context: boxology::CallContext,
        request: SendRequest,
    ) -> Result<DeliveryOutcome, SendTextError> {
        let result = if enabled() {
            outbound::send_typed(outbound::SendCommand {
                text: request.text,
                dedup_key: request.dedup_key,
            })
        } else {
            Err(AppError::authorization())
        };
        Ok(match result {
            Ok(receipt) => DeliveryOutcome {
                delivery: Some(receipt.into()),
                error: None,
            },
            Err(error) => DeliveryOutcome {
                delivery: None,
                error: Some(operation_error(error)),
            },
        })
    }

    pub async fn ask(
        &self,
        _context: boxology::CallContext,
        request: AskRequest,
    ) -> Result<AskOutcome, SendTextError> {
        let result = if enabled() {
            ask::run_typed(request)
        } else {
            Err(AppError::authorization())
        };
        Ok(match result {
            Ok(receipt) => AskOutcome {
                ask: Some(receipt),
                error: None,
            },
            Err(error) => AskOutcome {
                ask: None,
                error: Some(operation_error(error)),
            },
        })
    }
}

impl From<outbound::SendReceipt> for DeliveryReceipt {
    fn from(receipt: outbound::SendReceipt) -> Self {
        Self {
            dedup_key: receipt.dedup_key,
            message_id: receipt.message_id,
            deduplicated: receipt.deduplicated,
        }
    }
}

fn operation_error(error: AppError) -> OperationError {
    OperationError {
        code: error.code.into(),
        message: error.message.into(),
        retryable: error.retryable,
        retry_after_seconds: error.retry_after,
        class: match error.exit {
            ExitClass::Success | ExitClass::Invariant => FailureClass::Invariant,
            ExitClass::Input => FailureClass::Input,
            ExitClass::Authorization => FailureClass::Authorization,
            ExitClass::Conflict => FailureClass::Conflict,
            ExitClass::Local => FailureClass::Local,
            ExitClass::Policy => FailureClass::Policy,
            ExitClass::Transient => FailureClass::Transient,
            ExitClass::Permanent => FailureClass::Permanent,
            ExitClass::Ambiguous => FailureClass::Ambiguous,
        },
    }
}

fn map_send_text_error(error: AppError) -> SendTextError {
    match error.exit {
        ExitClass::Success | ExitClass::Invariant => SendTextError::Invariant,
        ExitClass::Input => SendTextError::Input,
        ExitClass::Authorization => SendTextError::Authorization,
        ExitClass::Conflict => SendTextError::Conflict,
        ExitClass::Local => SendTextError::Local,
        ExitClass::Policy => SendTextError::Policy,
        ExitClass::Transient => SendTextError::Transient,
        ExitClass::Permanent => SendTextError::Permanent,
        ExitClass::Ambiguous => SendTextError::Ambiguous,
    }
}

#[doc(hidden)]
pub mod generated {
    include!("../../generated/adapter/adapter.rs");
}

pub const SCHEMA: u8 = 1;
pub const ENABLED_VARIABLE: &str = "BOXOLOGY_TELEGRAM_ENABLED";
const MAX_INPUT: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExitClass {
    Success = 0,
    Input = 2,
    Authorization = 3,
    Conflict = 4,
    Local = 5,
    Policy = 6,
    Transient = 7,
    Permanent = 8,
    Ambiguous = 9,
    Invariant = 10,
}

#[derive(Debug)]
pub struct AppError {
    pub code: &'static str,
    pub message: &'static str,
    pub retryable: bool,
    pub exit: ExitClass,
    pub retry_after: Option<u64>,
}

impl AppError {
    pub const fn new(code: &'static str, message: &'static str, exit: ExitClass) -> Self {
        Self {
            code,
            message,
            retryable: false,
            exit,
            retry_after: None,
        }
    }

    pub const fn input(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            retryable: false,
            exit: ExitClass::Input,
            retry_after: None,
        }
    }

    pub const fn authorization() -> Self {
        Self {
            code: "telegram_disabled",
            message: "Telegram requires BOXOLOGY_TELEGRAM_ENABLED=1",
            retryable: false,
            exit: ExitClass::Authorization,
            retry_after: None,
        }
    }

    pub const fn unsupported() -> Self {
        Self {
            code: "unsupported",
            message: "operation is not available",
            retryable: false,
            exit: ExitClass::Policy,
            retry_after: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusRequest {
    schema: u8,
    probe: bool,
}

#[derive(Serialize)]
struct Envelope<'a> {
    schema: u8,
    ok: bool,
    command: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorBody<'a>>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_seconds: Option<u64>,
}

pub fn execute(args: &[String], input: &[u8]) -> (String, ExitClass) {
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
        return status(input);
    }
    if !enabled() {
        return failure(command, AppError::authorization());
    }
    if command == "pair" {
        return match subcommand {
            Some("begin") | Some("complete") | Some("revoke") => {
                match pairing::run(subcommand.expect("pair subcommand"), input) {
                    Ok(data) => success("pair", data),
                    Err(error) => failure("pair", error),
                }
            }
            _ => failure(
                "pair",
                AppError::input("invalid_subcommand", "invalid pair operation"),
            ),
        };
    }
    let result = match command {
        "poll" => receive::poll(input),
        "ack" => receive::ack(input),
        "send" => outbound::send(input),
        "reply" => outbound::reply(input),
        "resolve-send" => outbound::resolve(input),
        "ask" => ask::run(input),
        _ => return failure(command, AppError::unsupported()),
    };
    match result {
        Ok(data) => success(command, data),
        Err(error) => failure(command, error),
    }
}

fn status(input: &[u8]) -> (String, ExitClass) {
    let request: StatusRequest = match parse::<StatusRequest>(input) {
        Ok(request) if request.schema == SCHEMA => request,
        Ok(_) => {
            return failure(
                "status",
                AppError::input("unsupported_schema", "unsupported schema"),
            );
        }
        Err(error) => return failure("status", error),
    };
    if request.probe {
        if !enabled() {
            return failure("status", AppError::authorization());
        }
        let token = match api::load_token() {
            Ok(token) => token,
            Err(error) => return failure("status", error),
        };
        let api = match api::for_commands(token) {
            Ok(api) => api,
            Err(error) => return failure("status", error),
        };
        let bot = match api.get_me().map_err(api_error) {
            Ok(bot) => bot,
            Err(error) => return failure("status", error),
        };
        let webhook = match api.webhook_info().map_err(api_error) {
            Ok(webhook) => webhook,
            Err(error) => return failure("status", error),
        };
        let local = match state::Paths::from_env().and_then(|paths| state::read(&paths)) {
            Ok(state) => state,
            Err(error) => return failure("status", error),
        };
        let bot_matches = local.bot.is_some_and(|stored| stored.id == bot.id);
        return success(
            "status",
            serde_json::json!({"probe": true, "api_reachable": true, "bot_matches": bot_matches, "webhook_configured": !webhook.url.is_empty(), "get_updates_compatible": webhook.url.is_empty()}),
        );
    }
    let paths = match state::Paths::from_env() {
        Ok(paths) => paths,
        Err(error) => return failure("status", error),
    };
    let state = match state::read(&paths) {
        Ok(state) => state,
        Err(error) => return failure("status", error),
    };
    let data = serde_json::json!({
        "probe": false,
        "enabled": enabled(),
        "paired": state.pairing.is_some(),
        "next_offset": state.next_offset,
        "telegram_confirmed_before": state.confirmed_before,
        "consumer_locked": state::consumer_locked(&paths).unwrap_or(false),
        "inbox": {"unhandled": state.events.iter().filter(|event| !event.handled).count(), "bytes": serde_json::to_vec(&state.events).map_or(0, |bytes| bytes.len()), "full": state.events.len() >= 1000},
        "asks": {"active": state.asks.iter().filter(|ask| ask.state == "open").count(), "total": state.asks.len()},
        "outbound": {"ambiguous": state.outbound.iter().filter(|record| record.state == "ambiguous").count(), "total": state.outbound.len()},
        "pending_pair": state.pending_pair.is_some(),
        "last_receive_at": state.last_receive_at,
        "last_error_code": state.last_error_code
    });
    success("status", data)
}

pub(crate) fn enabled() -> bool {
    env::var(ENABLED_VARIABLE).is_ok_and(|value| value == "1")
}

pub fn run_listen(input: &[u8]) -> ExitClass {
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    listen::run(input, &mut output)
}

pub(crate) fn api_error(error: api::ApiError) -> AppError {
    AppError {
        code: error.code,
        message: match error.exit {
            ExitClass::Conflict => "Telegram polling is unavailable",
            ExitClass::Permanent => "Telegram rejected the request",
            ExitClass::Ambiguous => "Telegram delivery is ambiguous",
            _ => "Telegram is temporarily unavailable",
        },
        retryable: matches!(error.exit, ExitClass::Transient),
        exit: error.exit,
        retry_after: error.retry_after,
    }
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

pub(crate) fn success(command: &str, data: Value) -> (String, ExitClass) {
    let envelope = Envelope {
        schema: SCHEMA,
        ok: true,
        command,
        data: Some(data),
        error: None,
    };
    (
        serde_json::to_string(&envelope).expect("envelope serialization"),
        ExitClass::Success,
    )
}

pub(crate) fn failure(command: &str, error: AppError) -> (String, ExitClass) {
    let envelope = Envelope {
        schema: SCHEMA,
        ok: false,
        command,
        data: None,
        error: Some(ErrorBody {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
            retry_after_seconds: error.retry_after,
        }),
    };
    (
        serde_json::to_string(&envelope).expect("envelope serialization"),
        error.exit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_local_and_reports_the_exact_lease() {
        let _guard = test_guard();
        unsafe { env::remove_var(ENABLED_VARIABLE) };
        let (output, exit) = execute(&["status".into()], br#"{"schema":1,"probe":false}"#);
        assert_eq!(exit, ExitClass::Success);
        assert!(output.contains("\"enabled\":false"));
        unsafe { env::set_var(ENABLED_VARIABLE, "1") };
        let (output, _) = execute(&["status".into()], br#"{"schema":1,"probe":false}"#);
        assert!(output.contains("\"enabled\":true"));
        unsafe { env::remove_var(ENABLED_VARIABLE) };
    }

    #[test]
    fn network_commands_fail_closed_without_lease() {
        let _guard = test_guard();
        unsafe { env::remove_var(ENABLED_VARIABLE) };
        let (_, exit) = execute(&["send".into()], br#"{"schema":1}"#);
        assert_eq!(exit, ExitClass::Authorization);
    }

    #[test]
    fn typed_errors_preserve_every_stable_failure_class() {
        let cases = [
            (ExitClass::Input, SendTextError::Input),
            (ExitClass::Authorization, SendTextError::Authorization),
            (ExitClass::Conflict, SendTextError::Conflict),
            (ExitClass::Local, SendTextError::Local),
            (ExitClass::Policy, SendTextError::Policy),
            (ExitClass::Transient, SendTextError::Transient),
            (ExitClass::Permanent, SendTextError::Permanent),
            (ExitClass::Ambiguous, SendTextError::Ambiguous),
            (ExitClass::Invariant, SendTextError::Invariant),
        ];
        for (exit, expected) in cases {
            assert_eq!(
                map_send_text_error(AppError::new("test", "test", exit)),
                expected
            );
        }
    }
}

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
