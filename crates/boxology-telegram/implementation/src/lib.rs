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

    pub struct ReplyRequest {
        pub event_id: String,
        pub text: String,
        pub dedup_key: String,
    }

    pub enum ResolutionKind {
        Delivered,
        NotDelivered,
    }

    pub struct DeliveryResolution {
        pub kind: ResolutionKind,
        pub message_id: Option<i64>,
    }

    pub struct ResolveSendRequest {
        pub dedup_key: String,
        pub resolution: DeliveryResolution,
    }

    pub struct ResolveSendReceipt {
        pub dedup_key: String,
        pub resolved: ResolutionKind,
        pub message_id: Option<i64>,
    }

    pub struct ResolveSendOutcome {
        pub resolution: Option<ResolveSendReceipt>,
        pub error: Option<OperationError>,
    }

    pub struct PairBeginRequest {
        pub nonce_ttl_seconds: Option<u64>,
    }

    pub struct TelegramBotIdentity {
        pub id: i64,
        pub username: String,
    }

    pub struct PairBeginReceipt {
        pub deep_link: String,
        pub expires_at: i64,
        pub bot: TelegramBotIdentity,
    }

    pub struct PairBeginOutcome {
        pub pairing: Option<PairBeginReceipt>,
        pub error: Option<OperationError>,
    }

    pub struct PairCompleteRequest {
        pub timeout_seconds: Option<u64>,
    }

    pub enum PairConfirmation {
        Delivered,
        Ambiguous,
        NotAttempted,
    }

    pub struct PairCompleteReceipt {
        pub user_id: i64,
        pub chat_id: i64,
        pub paired_at: i64,
        pub confirmation: PairConfirmation,
    }

    pub struct PairCompleteOutcome {
        pub pairing: Option<PairCompleteReceipt>,
        pub error: Option<OperationError>,
    }

    pub struct PairRevokeRequest {}

    pub struct PairRevokeReceipt {
        pub pairing_revoked: bool,
    }

    pub struct PairRevokeOutcome {
        pub revocation: Option<PairRevokeReceipt>,
        pub error: Option<OperationError>,
    }

    pub struct PollRequest {
        pub timeout_seconds: Option<u64>,
    }

    pub enum InboundEventKind {
        Text,
        AskReply,
        AskChoice,
    }

    pub struct InboundReplyTarget {
        pub ask_id: Option<String>,
        pub outbound_message_id: Option<i64>,
    }

    pub struct InboundChoice {
        pub kind: String,
        pub key: Option<String>,
    }

    pub struct InboundEvent {
        pub event_id: String,
        pub kind: InboundEventKind,
        pub text: Option<String>,
        pub received_at: i64,
        pub reply_to: Option<InboundReplyTarget>,
        pub ask_id: Option<String>,
        pub lifecycle_key: Option<String>,
        pub choice: Option<InboundChoice>,
    }

    pub struct PollReceipt {
        pub fetched: bool,
        pub locally_durable: Option<bool>,
        pub telegram_confirmed: Option<bool>,
        pub next_offset: i64,
        pub telegram_confirmed_before: i64,
        pub callback_receipt_failed: bool,
    }

    pub struct PollResult {
        pub event: Option<InboundEvent>,
        pub receipt: PollReceipt,
    }

    pub struct PollOutcome {
        pub result: Option<PollResult>,
        pub error: Option<OperationError>,
    }

    pub struct AckRequest {
        pub event_id: String,
    }

    pub struct AckReceipt {
        pub event_id: String,
        pub handled: bool,
        pub already_handled: bool,
    }

    pub struct AckOutcome {
        pub acknowledgement: Option<AckReceipt>,
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

    #[capability(idempotency = inherent)]
    pub async fn reply(request: ReplyRequest) -> Result<DeliveryOutcome, SendTextError>;

    #[capability(idempotency = inherent)]
    pub async fn resolve_send(request: ResolveSendRequest) -> Result<ResolveSendOutcome, SendTextError>;

    #[capability]
    pub async fn pair_begin(request: PairBeginRequest) -> Result<PairBeginOutcome, SendTextError>;

    #[capability]
    pub async fn pair_complete(request: PairCompleteRequest) -> Result<PairCompleteOutcome, SendTextError>;

    #[capability]
    pub async fn pair_revoke(request: PairRevokeRequest) -> Result<PairRevokeOutcome, SendTextError>;

    #[capability]
    pub async fn poll(request: PollRequest) -> Result<PollOutcome, SendTextError>;

    #[capability(idempotency = inherent)]
    pub async fn ack(request: AckRequest) -> Result<AckOutcome, SendTextError>;
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
            receive::poll_typed(receive::PollCommand {
                timeout_seconds: request.timeout_seconds,
            })
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
