#[rustfmt::skip]
boxology::contract! {
    pub struct LoadRequest { pub session_id: String }
    pub enum SessionEventKind { User, Assistant, ToolCall, ToolResult }
    pub struct NewSessionEvent { pub event_id: String, pub kind: SessionEventKind, pub payload_json: String }
    pub struct AppendRequest { pub session_id: String, pub expected_sequence: u64, pub event: NewSessionEvent }
    pub struct SessionEvent { pub sequence: u64, pub event_id: String, pub kind: SessionEventKind, pub payload_json: String }
    pub struct LoadResult { pub events: Vec<SessionEvent>, pub next_sequence: u64 }
    pub struct AppendResult { pub sequence: u64, pub appended: bool }
    pub enum SessionFailureClass { Input, Boundary, Conflict, Resource, Corrupt, Local, Cancelled, Deadline }
    pub struct SessionFailure { pub class: SessionFailureClass, pub code: String, pub message: String, pub retryable: bool, pub side_effect_possible: bool }
    pub struct LoadOutcome { pub result: Option<LoadResult>, pub failure: Option<SessionFailure> }
    pub struct AppendOutcome { pub result: Option<AppendResult>, pub failure: Option<SessionFailure> }
    #[error] pub enum SessionError { Internal }
    #[capability] pub async fn load(request: LoadRequest) -> Result<LoadOutcome, SessionError>;
    #[capability] pub async fn append(request: AppendRequest) -> Result<AppendOutcome, SessionError>;
}
