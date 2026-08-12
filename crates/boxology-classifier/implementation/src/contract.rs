boxology::contract! {
    pub enum CompatibilityClass {
        Unchanged,
        Documentation,
        Deprecation,
        Additive,
        CompatibleWithConditions,
        Incompatible,
    }

    pub struct ClassifyRequest {
        pub base: Option<Vec<u8>>,
        pub submitted: Vec<u8>,
    }

    pub struct ClassifyFinding {
        pub code: String,
        pub path: String,
        pub kind: String,
        pub class: CompatibilityClass,
        pub base_excerpt: Option<String>,
        pub submitted_excerpt: Option<String>,
        pub condition: Option<String>,
    }

    pub struct ClassifyReport {
        pub verdict: CompatibilityClass,
        pub findings: Vec<ClassifyFinding>,
        pub rendered_text: String,
    }

    pub enum ClassifyFailureStage {
        Base,
        Submitted,
        Pairing,
    }

    pub struct ClassifyFailure {
        pub stage: ClassifyFailureStage,
        pub diagnostics: String,
    }

    pub struct ClassifyOutcome {
        pub report: Option<ClassifyReport>,
        pub failure: Option<ClassifyFailure>,
    }

    #[error]
    pub enum ClassifierError { Internal }

    #[capability]
    pub async fn classify(request: ClassifyRequest) -> Result<ClassifyOutcome, ClassifierError>;
}
