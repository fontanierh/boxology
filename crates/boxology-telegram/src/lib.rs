use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;

#[allow(dead_code)]
mod api;
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
}

impl AppError {
    pub const fn new(code: &'static str, message: &'static str, exit: ExitClass) -> Self {
        Self {
            code,
            message,
            retryable: false,
            exit,
        }
    }

    pub const fn input(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            retryable: false,
            exit: ExitClass::Input,
        }
    }

    pub const fn authorization() -> Self {
        Self {
            code: "telegram_disabled",
            message: "Telegram requires BOXOLOGY_TELEGRAM_ENABLED=1",
            retryable: false,
            exit: ExitClass::Authorization,
        }
    }

    pub const fn unsupported() -> Self {
        Self {
            code: "unsupported",
            message: "operation is not available",
            retryable: false,
            exit: ExitClass::Policy,
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
    let _ = subcommand;
    failure(command, AppError::unsupported())
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
        return failure("status", AppError::unsupported());
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

fn enabled() -> bool {
    env::var(ENABLED_VARIABLE).is_ok_and(|value| value == "1")
}

fn parse<T: for<'de> Deserialize<'de>>(input: &[u8]) -> Result<T, AppError> {
    if input.len() > MAX_INPUT {
        return Err(AppError::input(
            "input_too_large",
            "request exceeds input limit",
        ));
    }
    serde_json::from_slice(input)
        .map_err(|_| AppError::input("invalid_json", "request must be one valid JSON object"))
}

fn success(command: &str, data: Value) -> (String, ExitClass) {
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

fn failure(command: &str, error: AppError) -> (String, ExitClass) {
    let envelope = Envelope {
        schema: SCHEMA,
        ok: false,
        command,
        data: None,
        error: Some(ErrorBody {
            code: error.code,
            message: error.message,
            retryable: error.retryable,
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
