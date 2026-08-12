#[rustfmt::skip]
boxology::contract! {
    pub enum MessageRole { System, User, Assistant, Tool }
    pub enum FinishReason { Stop, ToolCalls, Length }
    pub enum CompletionFailureClass { Configuration, Input, Local, Transient, Permanent, Malformed }
    pub struct ToolCall { pub id: String, pub name: String, pub arguments_json: String }
    pub struct CompletionMessage { pub role: MessageRole, pub content: Option<String>, pub tool_call_id: Option<String>, pub name: Option<String>, pub tool_calls: Vec<ToolCall> }
    pub struct ToolDefinition { pub name: String, pub description: String, pub input_schema_json: String }
    pub struct TokenUsage { pub input_tokens: u64, pub output_tokens: u64, pub total_tokens: u64 }
    pub struct CompletionRequest { pub messages: Vec<CompletionMessage>, pub tools: Vec<ToolDefinition>, pub max_output_tokens: Option<u64> }
    pub struct CompletionResult { pub content: Option<String>, pub tool_calls: Vec<ToolCall>, pub finish_reason: FinishReason, pub usage: TokenUsage }
    pub struct CompletionFailure { pub class: CompletionFailureClass, pub code: String, pub message: String, pub retryable: bool, pub retry_after_seconds: Option<u64> }
    pub struct CompletionOutcome { pub completion: Option<CompletionResult>, pub failure: Option<CompletionFailure> }
    #[error]
    pub enum CompleteError { Internal }
    #[capability]
    pub async fn complete(request: CompletionRequest) -> Result<CompletionOutcome, CompleteError>;
}
