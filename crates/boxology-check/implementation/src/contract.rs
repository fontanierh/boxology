boxology::contract! {
    pub struct CheckRequest { pub workspace: String, pub base: Option<String> }
    pub enum CheckStatus { Passed, Failed }
    pub enum CheckStepStatus { Passed, Failed, Skipped }
    pub enum CheckFailureKind { Validation, Invocation }
    pub struct CheckFinding {
        pub kind: String, pub code: String, pub path: String, pub package: Option<String>,
        pub payload: Option<String>, pub rule: Option<String>, pub rule_source: Option<String>,
        pub span_start_line: Option<u64>, pub span_start_column: Option<u64>,
        pub span_end_line: Option<u64>, pub span_end_column: Option<u64>,
        pub offending: Option<String>, pub class: Option<String>, pub condition: Option<String>,
    }
    pub struct CheckStepReport {
        pub id: String, pub status: CheckStepStatus, pub reason: Option<String>,
        pub findings: Vec<CheckFinding>, pub output: Option<Vec<u8>>,
    }
    pub struct CheckReport {
        pub steps: Vec<CheckStepReport>, pub status: CheckStatus,
        pub human: Vec<u8>, pub json: Vec<u8>,
    }
    pub struct CheckFailure {
        pub kind: CheckFailureKind, pub human: Vec<u8>, pub json: Vec<u8>,
    }
    pub struct CheckOutcome { pub report: Option<CheckReport>, pub failure: Option<CheckFailure> }
    #[error]
    pub enum CheckError { Internal }
    #[capability]
    pub async fn check(request: CheckRequest) -> Result<CheckOutcome, CheckError>;
}
