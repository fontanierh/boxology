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

    pub struct ListenStartRequest {}

    pub struct ListenStartReceipt {
        pub next_offset: i64,
        pub unhandled: u64,
    }

    pub struct ListenStartOutcome {
        pub startup: Option<ListenStartReceipt>,
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

    pub struct StatusRequest {
        pub probe: bool,
    }

    pub struct InboxStatus {
        pub unhandled: u64,
        pub bytes: u64,
        pub full: bool,
    }

    pub struct AskStatus {
        pub active: u64,
        pub total: u64,
    }

    pub struct OutboundStatus {
        pub ambiguous: u64,
        pub total: u64,
    }

    pub struct LocalStatus {
        pub enabled: bool,
        pub paired: bool,
        pub next_offset: i64,
        pub telegram_confirmed_before: i64,
        pub consumer_locked: bool,
        pub inbox: InboxStatus,
        pub asks: AskStatus,
        pub outbound: OutboundStatus,
        pub pending_pair: bool,
        pub last_receive_at: Option<i64>,
        pub last_error_code: Option<String>,
    }

    pub struct ProbeStatus {
        pub api_reachable: bool,
        pub bot_matches: bool,
        pub webhook_configured: bool,
        pub get_updates_compatible: bool,
    }

    pub struct StatusResult {
        pub local: Option<LocalStatus>,
        pub probe: Option<ProbeStatus>,
    }

    pub struct StatusOutcome {
        pub status: Option<StatusResult>,
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

    #[capability]
    pub async fn listen_start(request: ListenStartRequest) -> Result<ListenStartOutcome, SendTextError>;

    #[capability(idempotency = inherent)]
    pub async fn ack(request: AckRequest) -> Result<AckOutcome, SendTextError>;

    #[capability]
    pub async fn status(request: StatusRequest) -> Result<StatusOutcome, SendTextError>;
}
