use std::env;
use std::sync::Mutex;

#[allow(dead_code)]
mod api;
mod ask;
#[doc(hidden)]
pub mod cli;
#[cfg(test)]
#[allow(clippy::collapsible_if)]
mod listen;
mod outbound;
mod pairing;
mod receive;
mod state;

mod contract;
pub use contract::*;

#[derive(Default)]
pub struct TelegramService {
    consumer: Mutex<Option<state::ConsumerLock>>,
}

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

    pub async fn reply(
        &self,
        _context: boxology::CallContext,
        request: ReplyRequest,
    ) -> Result<DeliveryOutcome, SendTextError> {
        let result = if enabled() {
            outbound::reply_typed(outbound::ReplyCommand {
                event_id: request.event_id,
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

    pub async fn resolve_send(
        &self,
        _context: boxology::CallContext,
        request: ResolveSendRequest,
    ) -> Result<ResolveSendOutcome, SendTextError> {
        let result = if enabled() {
            let kind = match request.resolution.kind {
                ResolutionKind::Delivered => "delivered",
                ResolutionKind::NotDelivered => "not_delivered",
                ResolutionKind::Unknown { .. } => "unknown",
            };
            outbound::resolve_typed(outbound::ResolveCommand {
                dedup_key: request.dedup_key,
                kind: kind.into(),
                message_id: request.resolution.message_id,
            })
        } else {
            Err(AppError::authorization())
        };
        Ok(match result {
            Ok(receipt) => ResolveSendOutcome {
                resolution: Some(ResolveSendReceipt {
                    dedup_key: receipt.dedup_key,
                    resolved: match receipt.resolved {
                        outbound::Resolved::Delivered => ResolutionKind::Delivered,
                        outbound::Resolved::NotDelivered => ResolutionKind::NotDelivered,
                    },
                    message_id: receipt.message_id,
                }),
                error: None,
            },
            Err(error) => ResolveSendOutcome {
                resolution: None,
                error: Some(operation_error(error)),
            },
        })
    }

    pub async fn pair_begin(
        &self,
        _context: boxology::CallContext,
        request: PairBeginRequest,
    ) -> Result<PairBeginOutcome, SendTextError> {
        let result = if enabled() {
            pairing::begin_typed(pairing::BeginCommand {
                nonce_ttl_seconds: request.nonce_ttl_seconds,
            })
        } else {
            Err(AppError::authorization())
        };
        Ok(match result {
            Ok(receipt) => PairBeginOutcome {
                pairing: Some(PairBeginReceipt {
                    deep_link: receipt.deep_link,
                    expires_at: receipt.expires_at,
                    bot: TelegramBotIdentity {
                        id: receipt.bot.id,
                        username: receipt.bot.username,
                    },
                }),
                error: None,
            },
            Err(error) => PairBeginOutcome {
                pairing: None,
                error: Some(operation_error(error)),
            },
        })
    }

    pub async fn pair_complete(
        &self,
        _context: boxology::CallContext,
        request: PairCompleteRequest,
    ) -> Result<PairCompleteOutcome, SendTextError> {
        let result = if enabled() {
            pairing::complete_typed(pairing::CompleteCommand {
                timeout_seconds: request.timeout_seconds,
            })
        } else {
            Err(AppError::authorization())
        };
        Ok(match result {
            Ok(receipt) => PairCompleteOutcome {
                pairing: Some(PairCompleteReceipt {
                    user_id: receipt.user_id,
                    chat_id: receipt.chat_id,
                    paired_at: receipt.paired_at,
                    confirmation: match receipt.confirmation {
                        pairing::Confirmation::Delivered => PairConfirmation::Delivered,
                        pairing::Confirmation::Ambiguous => PairConfirmation::Ambiguous,
                        pairing::Confirmation::NotAttempted => PairConfirmation::NotAttempted,
                    },
                }),
                error: None,
            },
            Err(error) => PairCompleteOutcome {
                pairing: None,
                error: Some(operation_error(error)),
            },
        })
    }

    pub async fn pair_revoke(
        &self,
        _context: boxology::CallContext,
        _request: PairRevokeRequest,
    ) -> Result<PairRevokeOutcome, SendTextError> {
        let result = if enabled() {
            pairing::revoke_typed()
        } else {
            Err(AppError::authorization())
        };
        Ok(match result {
            Ok(receipt) => PairRevokeOutcome {
                revocation: Some(PairRevokeReceipt {
                    pairing_revoked: receipt.pairing_revoked,
                }),
                error: None,
            },
            Err(error) => PairRevokeOutcome {
                revocation: None,
                error: Some(operation_error(error)),
            },
        })
    }

    pub async fn poll(
        &self,
        _context: boxology::CallContext,
        request: PollRequest,
    ) -> Result<PollOutcome, SendTextError> {
        let result = if enabled() {
            let command = receive::PollCommand {
                timeout_seconds: request.timeout_seconds,
            };
            let consumer = self
                .consumer
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if consumer.is_some() {
                receive::poll_typed_locked(command)
            } else {
                drop(consumer);
                receive::poll_typed(command)
            }
        } else {
            Err(AppError::authorization())
        };
        Ok(match result {
            Ok(result) => PollOutcome {
                result: Some(typed_poll_result(result)),
                error: None,
            },
            Err(error) => PollOutcome {
                result: None,
                error: Some(operation_error(error)),
            },
        })
    }

    pub async fn listen_start(
        &self,
        _context: boxology::CallContext,
        _request: ListenStartRequest,
    ) -> Result<ListenStartOutcome, SendTextError> {
        let result = listen_start_typed(&self.consumer);
        Ok(match result {
            Ok(startup) => ListenStartOutcome {
                startup: Some(startup),
                error: None,
            },
            Err(error) => ListenStartOutcome {
                startup: None,
                error: Some(operation_error(error)),
            },
        })
    }

    pub async fn ack(
        &self,
        _context: boxology::CallContext,
        request: AckRequest,
    ) -> Result<AckOutcome, SendTextError> {
        let result = if enabled() {
            receive::ack_typed(receive::AckCommand {
                event_id: request.event_id,
            })
        } else {
            Err(AppError::authorization())
        };
        Ok(match result {
            Ok(receipt) => AckOutcome {
                acknowledgement: Some(AckReceipt {
                    event_id: receipt.event_id,
                    handled: receipt.handled,
                    already_handled: receipt.already_handled,
                }),
                error: None,
            },
            Err(error) => AckOutcome {
                acknowledgement: None,
                error: Some(operation_error(error)),
            },
        })
    }

    pub async fn status(
        &self,
        _context: boxology::CallContext,
        request: StatusRequest,
    ) -> Result<StatusOutcome, SendTextError> {
        Ok(match status_typed(request) {
            Ok(status) => StatusOutcome {
                status: Some(status),
                error: None,
            },
            Err(error) => StatusOutcome {
                status: None,
                error: Some(operation_error(error)),
            },
        })
    }
}

#[cfg(test)]
macro_rules! direct_backend {
    ($($method:ident($request:ty) -> $outcome:ty;)*) => {
        impl cli::Backend for TelegramService {$(
            fn $method(&self, request: $request) -> Result<$outcome, AppError> {
                direct(boxology_generated_contract::TelegramDispatch::$method(self, call_context(), request))
            }
        )*}
    };
}

#[cfg(test)]
cli::backend_methods!(direct_backend);

#[cfg(test)]
fn call_context() -> boxology_contract::CallContext {
    boxology_contract::CallContext::new(
        boxology_contract::Caller::Anonymous,
        None,
        boxology_contract::CancelToken::new(),
        boxology_contract::TraceContext::empty(),
        None,
    )
}

#[cfg(test)]
fn direct<T>(
    future: impl std::future::Future<Output = Result<T, boxology_generated_contract::SendTextError>>,
) -> Result<T, AppError> {
    let mut future = std::pin::pin!(future);
    match std::future::Future::poll(
        future.as_mut(),
        &mut std::task::Context::from_waker(std::task::Waker::noop()),
    ) {
        std::task::Poll::Ready(Ok(value)) => Ok(value),
        std::task::Poll::Ready(Err(_)) => Err(AppError::invariant()),
        std::task::Poll::Pending => Err(AppError::invariant()),
    }
}

fn listen_start_typed(
    slot: &Mutex<Option<state::ConsumerLock>>,
) -> Result<ListenStartReceipt, AppError> {
    if !enabled() {
        return Err(AppError::authorization());
    }
    let mut slot = slot.lock().unwrap_or_else(|error| error.into_inner());
    if slot.is_some() {
        return Err(AppError::new(
            "consumer_locked",
            "another local consumer holds the lock",
            ExitClass::Conflict,
        ));
    }
    let paths = state::Paths::from_env()?;
    let consumer = state::ConsumerLock::acquire(&paths)?;
    let current = state::read(&paths)?;
    if current.pairing.is_none() {
        return Err(AppError::new(
            "not_paired",
            "Telegram pairing is required",
            ExitClass::Policy,
        ));
    }
    let unhandled = u64::try_from(current.events.iter().filter(|event| !event.handled).count())
        .map_err(|_| {
            AppError::new(
                "listen_count_overflow",
                "listener event count exceeds its supported range",
                ExitClass::Invariant,
            )
        })?;
    *slot = Some(consumer);
    Ok(ListenStartReceipt {
        next_offset: current.next_offset,
        unhandled,
    })
}

fn typed_poll_result(result: receive::PollResult) -> PollResult {
    let event = result.event.map(|event| InboundEvent {
        event_id: event.event_id,
        kind: match event.kind.as_str() {
            "text" => InboundEventKind::Text,
            "ask_reply" => InboundEventKind::AskReply,
            "ask_choice" => InboundEventKind::AskChoice,
            _ => unreachable!("validated durable event kind"),
        },
        text: (event.kind != "ask_choice").then_some(event.text),
        received_at: event.received_at,
        reply_to: event.reply_to.map(|target| InboundReplyTarget {
            ask_id: target.ask_id,
            outbound_message_id: target.outbound_message_id,
        }),
        ask_id: event.ask_id,
        lifecycle_key: event.lifecycle_key,
        choice: event.choice.map(|choice| InboundChoice {
            kind: choice.kind,
            key: choice.key,
        }),
    });
    PollResult {
        receipt: PollReceipt {
            fetched: result.fetched,
            locally_durable: event.as_ref().map(|_| true),
            telegram_confirmed: result.telegram_confirmed,
            next_offset: result.next_offset,
            telegram_confirmed_before: result.telegram_confirmed_before,
            callback_receipt_failed: result.callback_warning,
        },
        event,
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

    pub const fn invariant() -> Self {
        Self {
            code: "invalid_backend_outcome",
            message: "Telegram capability returned an invalid outcome",
            retryable: false,
            exit: ExitClass::Invariant,
            retry_after: None,
        }
    }
}

#[cfg(test)]
pub fn execute(args: &[String], input: &[u8]) -> (String, ExitClass) {
    cli::execute(&TelegramService::default(), enabled(), args, input)
}

fn status_typed(request: StatusRequest) -> Result<StatusResult, AppError> {
    if request.probe {
        if !enabled() {
            return Err(AppError::authorization());
        }
        let api = api::for_commands(api::load_token()?)?;
        let bot = api.get_me().map_err(api_error)?;
        let webhook = api.webhook_info().map_err(api_error)?;
        let local = state::Paths::from_env().and_then(|paths| state::read(&paths))?;
        let bot_matches = local.bot.is_some_and(|stored| stored.id == bot.id);
        return Ok(StatusResult {
            local: None,
            probe: Some(ProbeStatus {
                api_reachable: true,
                bot_matches,
                webhook_configured: !webhook.url.is_empty(),
                get_updates_compatible: webhook.url.is_empty(),
            }),
        });
    }
    let paths = state::Paths::from_env()?;
    let local = state::read(&paths)?;
    let count = |value| {
        u64::try_from(value).map_err(|_| {
            AppError::new(
                "status_count_overflow",
                "local status count exceeds its supported range",
                ExitClass::Invariant,
            )
        })
    };
    Ok(StatusResult {
        probe: None,
        local: Some(LocalStatus {
            enabled: enabled(),
            paired: local.pairing.is_some(),
            next_offset: local.next_offset,
            telegram_confirmed_before: local.confirmed_before,
            consumer_locked: state::consumer_locked(&paths).unwrap_or(false),
            inbox: InboxStatus {
                unhandled: count(local.events.iter().filter(|event| !event.handled).count())?,
                bytes: count(serde_json::to_vec(&local.events).map_or(0, |bytes| bytes.len()))?,
                full: local.events.len() >= 1_000,
            },
            asks: AskStatus {
                active: count(local.asks.iter().filter(|ask| ask.state == "open").count())?,
                total: count(local.asks.len())?,
            },
            outbound: OutboundStatus {
                ambiguous: count(
                    local
                        .outbound
                        .iter()
                        .filter(|record| record.state == "ambiguous")
                        .count(),
                )?,
                total: count(local.outbound.len())?,
            },
            pending_pair: local.pending_pair.is_some(),
            last_receive_at: local.last_receive_at,
            last_error_code: local.last_error_code,
        }),
    })
}

pub(crate) fn enabled() -> bool {
    env::var(ENABLED_VARIABLE).is_ok_and(|value| value == "1")
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

#[cfg(test)]
pub(crate) fn parse<T: for<'de> serde::Deserialize<'de>>(input: &[u8]) -> Result<T, AppError> {
    cli::parse(input)
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
            (ExitClass::Input, SendTextError::Input, FailureClass::Input),
            (
                ExitClass::Authorization,
                SendTextError::Authorization,
                FailureClass::Authorization,
            ),
            (
                ExitClass::Conflict,
                SendTextError::Conflict,
                FailureClass::Conflict,
            ),
            (ExitClass::Local, SendTextError::Local, FailureClass::Local),
            (
                ExitClass::Policy,
                SendTextError::Policy,
                FailureClass::Policy,
            ),
            (
                ExitClass::Transient,
                SendTextError::Transient,
                FailureClass::Transient,
            ),
            (
                ExitClass::Permanent,
                SendTextError::Permanent,
                FailureClass::Permanent,
            ),
            (
                ExitClass::Ambiguous,
                SendTextError::Ambiguous,
                FailureClass::Ambiguous,
            ),
            (
                ExitClass::Invariant,
                SendTextError::Invariant,
                FailureClass::Invariant,
            ),
        ];
        for (index, (exit, send_error, failure_class)) in cases.into_iter().enumerate() {
            assert_eq!(
                map_send_text_error(AppError::new("test", "test", exit)),
                send_error
            );
            let retryable = exit == ExitClass::Transient;
            let retry_after = retryable.then_some(17);
            assert_eq!(
                operation_error(AppError {
                    code: "exact_code",
                    message: "exact message",
                    retryable,
                    exit,
                    retry_after,
                }),
                OperationError {
                    code: "exact_code".into(),
                    message: "exact message".into(),
                    retryable,
                    retry_after_seconds: retry_after,
                    class: failure_class,
                },
                "failure projection case {index}"
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
