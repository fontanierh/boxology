#[rustfmt::skip]
boxology::contract! {
    pub enum TurnFailureClass { Input, Session, Model, Tool, Protocol, Cancelled, Deadline }
    pub struct ModelTool { pub name: String, pub description: String, pub input_schema_json: String }
    pub struct RunTurnRequest { pub session_id: String, pub turn_id: String, pub user_message: String, pub system_prompt: String, pub tools: Vec<ModelTool>, pub max_output_tokens: Option<u64> }
    pub struct TurnUsage { pub input_tokens: u64, pub output_tokens: u64, pub total_tokens: u64 }
    pub struct RunTurnResult { pub answer: String, pub tool_name: Option<String>, pub tool_call_id: Option<String>, pub tool_output_json: Option<String>, pub usage: TurnUsage }
    pub struct TurnFailure { pub class: TurnFailureClass, pub code: String, pub message: String, pub retryable: bool, pub side_effect_possible: bool }
    pub struct RunTurnOutcome { pub result: Option<RunTurnResult>, pub failure: Option<TurnFailure> }
    pub struct CompactRequest { pub session_id: String, pub checkpoint_id: String, pub summary: String }
    pub struct CompactResult { pub event_id: String, pub sequence: u64, pub appended: bool }
    pub struct CompactOutcome { pub result: Option<CompactResult>, pub failure: Option<TurnFailure> }
    #[error] pub enum RunTurnError { Internal }
    #[capability] pub async fn run_turn(request: RunTurnRequest) -> Result<RunTurnOutcome, RunTurnError>;
    #[capability] pub async fn compact(request: CompactRequest) -> Result<CompactOutcome, RunTurnError>;
}
