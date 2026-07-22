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
    let state = match state::Paths::from_env().and_then(|paths| state::read(&paths)) {
        Ok(state) => state,
        Err(error) => return failure("status", error),
    };
    let data = serde_json::json!({
        "probe": false,
        "enabled": enabled(),
        "paired": state.pairing.is_some(),
        "next_offset": state.next_offset,
        "inbox": {"unhandled": state.events.iter().filter(|event| !event.handled).count()}
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
        unsafe { env::remove_var(ENABLED_VARIABLE) };
        let (_, exit) = execute(&["send".into()], br#"{"schema":1}"#);
        assert_eq!(exit, ExitClass::Authorization);
    }
}
