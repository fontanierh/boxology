#[rustfmt::skip]
boxology::contract! {
    pub struct ReadRequest { pub path: String }
    pub struct WriteRequest { pub path: String, pub content: String }
    pub struct EditRequest { pub path: String, pub old_text: String, pub new_text: String }
    pub struct BashRequest { pub command: String, pub cwd: Option<String>, pub timeout_ms: Option<u64> }
    pub struct BashResult { pub stdout: String, pub stderr: String, pub stdout_bytes: u64, pub stderr_bytes: u64, pub stdout_truncated: bool, pub stderr_truncated: bool, pub exit_code: Option<i32>, pub signal: Option<i32> }
    pub struct ExecuteRequest { pub read: Option<ReadRequest>, pub write: Option<WriteRequest>, pub edit: Option<EditRequest>, pub bash: Option<BashRequest> }
    pub enum FileOperation { Read, Write, Edit }
    pub struct FileResult { pub operation: FileOperation, pub path: String, pub content: Option<String>, pub bytes: u64, pub changed: bool }
    pub struct ExecuteResult { pub file: Option<FileResult>, pub bash: Option<BashResult> }
    pub enum ToolFailureClass { Input, Boundary, Missing, Conflict, Resource, Local, Cancelled, Deadline }
    pub struct ToolFailure { pub class: ToolFailureClass, pub code: String, pub message: String, pub retryable: bool, pub side_effect_possible: bool }
    pub struct ExecuteOutcome { pub result: Option<ExecuteResult>, pub failure: Option<ToolFailure> }
    #[error]
    pub enum ExecuteError { Internal }
    #[capability]
    pub async fn execute(request: ExecuteRequest) -> Result<ExecuteOutcome, ExecuteError>;
}
