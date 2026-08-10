use crate::receive;
use crate::state::{self, Paths};
use crate::{AppError, ExitClass, SCHEMA, enabled};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static STOP: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListenRequest {
    schema: u8,
    long_poll_seconds: Option<u64>,
    heartbeat_seconds: Option<u64>,
}

pub(crate) fn run(input: &[u8], output: &mut dyn Write) -> ExitClass {
    if !enabled() {
        return emit_failure(output, AppError::authorization());
    }
    let request: ListenRequest = match crate::parse(input) {
        Ok(request) => request,
        Err(error) => return emit_failure(output, error),
    };
    if request.schema != SCHEMA {
        return emit_failure(
            output,
            AppError::input("unsupported_schema", "unsupported schema"),
        );
    }
    let long_poll = request.long_poll_seconds.unwrap_or(30);
    let heartbeat = request.heartbeat_seconds.unwrap_or(60);
    if !(1..=50).contains(&long_poll) || !(10..=300).contains(&heartbeat) {
        return emit_failure(
            output,
            AppError::input("invalid_listen_limits", "listen limits are out of bounds"),
        );
    }
    let paths = match Paths::from_env() {
        Ok(paths) => paths,
        Err(error) => return emit_failure(output, error),
    };
    let consumer = match state::ConsumerLock::acquire(&paths) {
        Ok(lock) => lock,
        Err(error) => return emit_failure(output, error),
    };
    let initial = match state::read(&paths) {
        Ok(state) => state,
        Err(error) => return fatal(output, error),
    };
    if initial.pairing.is_none() {
        return fatal(
            output,
            AppError::new(
                "not_paired",
                "Telegram pairing is required",
                ExitClass::Policy,
            ),
        );
    }
    if emit(output, json!({"kind": "startup", "paired": true, "next_offset": initial.next_offset, "unhandled": initial.events.iter().filter(|event| !event.handled).count()})).is_err() {
        drop(consumer);
        return ExitClass::Local;
    }
    install_signal_handlers();
    STOP.store(false, Ordering::Relaxed);
    let poll_input = serde_json::to_vec(&json!({"schema": SCHEMA, "timeout_seconds": long_poll}))
        .expect("poll request");
    let mut emitted = BTreeSet::new();
    let mut last_heartbeat = Instant::now();
    let mut backoff = 1_u64;
    let result = loop {
        if STOP.load(Ordering::Relaxed) {
            break stop(output, "signal");
        }
        let state = match state::read(&paths) {
            Ok(state) => state,
            Err(error) => break fatal(output, error),
        };
        if let Some(event) = receive::oldest_unhandled(&state) {
            if emitted.insert(event.event_id.clone())
                && emit(
                    output,
                    json!({"kind": "event", "event": receive::event_value(&event)}),
                )
                .is_err()
            {
                break ExitClass::Local;
            }
            if last_heartbeat.elapsed() < Duration::from_secs(heartbeat) {
                thread::sleep(Duration::from_millis(250));
            }
        } else {
            match receive::poll_locked(&poll_input) {
                Ok(data) => {
                    backoff = 1;
                    if let Some(warnings) = data.get("warnings")
                        && emit(output, json!({"kind": "warning", "code": "callback_receipt_failed", "message": warnings})).is_err()
                    {
                        break ExitClass::Local;
                    }
                    if let Some(event) = data.get("event").filter(|event| !event.is_null()) {
                        let event_id = event
                            .get("event_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if !event_id.is_empty()
                            && emitted.insert(event_id.into())
                            && emit(output, json!({"kind": "event", "event": event})).is_err()
                        {
                            break ExitClass::Local;
                        }
                    }
                }
                Err(error) if error.exit == ExitClass::Transient => {
                    if emit(output, json!({"kind": "warning", "code": error.code, "message": "Telegram receive is temporarily unavailable", "retryable": true})).is_err() {
                        break ExitClass::Local;
                    }
                    let wait = error.retry_after.unwrap_or(backoff).min(30);
                    thread::sleep(Duration::from_secs(wait.max(1)));
                    backoff = backoff.saturating_mul(2).min(30);
                }
                Err(error) if error.code == "inbox_full" => {
                    if emit(output, json!({"kind": "warning", "code": "inbox_full", "message": "inbound storage is full", "retryable": true})).is_err() {
                        break ExitClass::Local;
                    }
                    thread::sleep(Duration::from_secs(1));
                }
                Err(error) => break fatal(output, error),
            }
        }
        if last_heartbeat.elapsed() >= Duration::from_secs(heartbeat) {
            let current = match state::read(&paths) {
                Ok(state) => state,
                Err(error) => break fatal(output, error),
            };
            if emit(output, json!({"kind": "heartbeat", "at": state::now(), "unhandled": current.events.iter().filter(|event| !event.handled).count(), "inbox_full": current.events.len() >= 1_000})).is_err() {
                break ExitClass::Local;
            }
            last_heartbeat = Instant::now();
        }
    };
    drop(consumer);
    result
}

fn emit(output: &mut dyn Write, data: Value) -> std::io::Result<()> {
    serde_json::to_writer(
        &mut *output,
        &json!({"schema": SCHEMA, "ok": true, "command": "listen", "data": data}),
    )?;
    output.write_all(b"\n")?;
    output.flush()
}

fn emit_failure(output: &mut dyn Write, error: AppError) -> ExitClass {
    let mut body =
        json!({"code": error.code, "message": error.message, "retryable": error.retryable});
    if let Some(retry_after) = error.retry_after {
        body["retry_after_seconds"] = json!(retry_after);
    }
    if serde_json::to_writer(
        &mut *output,
        &json!({"schema": SCHEMA, "ok": false, "command": "listen", "error": body}),
    )
    .is_ok()
    {
        let _ = output.write_all(b"\n");
    }
    error.exit
}

fn fatal(output: &mut dyn Write, error: AppError) -> ExitClass {
    let exit = emit_failure(output, error);
    let _ = emit(output, json!({"kind": "stopped", "reason": "fatal_error"}));
    exit
}

fn stop(output: &mut dyn Write, reason: &str) -> ExitClass {
    let _ = emit(output, json!({"kind": "stopped", "reason": reason}));
    ExitClass::Success
}

#[cfg(unix)]
fn install_signal_handlers() {
    unsafe {
        signal(2, stop_signal);
        signal(15, stop_signal);
    }
}

#[cfg(not(unix))]
fn install_signal_handlers() {}

#[cfg(unix)]
extern "C" fn stop_signal(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
unsafe extern "C" {
    fn signal(signal: i32, handler: extern "C" fn(i32)) -> usize;
}
