//! Governed classifier use-case boundary over the ordinary classifier engine.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_classifier::{Class, ClassificationReport, classify};
use boxology_schema::SchemaDocument;

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

    #[error]
    pub enum ClassifierError { Failure(String) }

    #[capability]
    pub async fn classify(request: ClassifyRequest) -> Result<ClassifyReport, ClassifierError>;
}

/// Pure implementation of the generated classifier capability.
pub struct ClassifierService;

#[boxology::implementation]
impl ClassifierService {
    /// Classifies strict canonical schema bytes without consulting ambient state.
    pub async fn classify(
        &self,
        _context: boxology::CallContext,
        request: ClassifyRequest,
    ) -> Result<ClassifyReport, ClassifierError> {
        let base = match request.base.as_deref() {
            Some(bytes) => Some(SchemaDocument::parse(bytes).map_err(base_failure)?),
            None => None,
        };
        let submitted = SchemaDocument::parse(&request.submitted).map_err(submitted_failure)?;
        let report = classify(base.as_ref(), Some(&submitted)).map_err(pairing_failure)?;
        Ok(boundary_report(&report))
    }
}

fn base_failure(diagnostics: boxology_schema::Diagnostics) -> ClassifierError {
    ClassifierError::Failure(format!(
        "BXW0077 base: the checked-in schema document must satisfy the strict format-1 reader: {diagnostics}"
    ))
}

fn submitted_failure(diagnostics: boxology_schema::Diagnostics) -> ClassifierError {
    ClassifierError::Failure(format!(
        "BXW0078 submitted: the regenerated schema document must satisfy the strict format-1 reader: {diagnostics}"
    ))
}

fn pairing_failure(diagnostics: boxology_schema::Diagnostics) -> ClassifierError {
    ClassifierError::Failure(format!(
        "BXW0079 pairing: the checked-in and regenerated schema documents must pair and satisfy classifier integrity: {diagnostics}"
    ))
}

fn boundary_report(report: &ClassificationReport) -> ClassifyReport {
    ClassifyReport {
        verdict: boundary_class(report.verdict()),
        findings: report
            .findings()
            .iter()
            .map(|finding| ClassifyFinding {
                code: finding.code().to_owned(),
                path: finding.path().to_owned(),
                kind: finding.kind().to_owned(),
                class: boundary_class(finding.class()),
                base_excerpt: finding.base_excerpt().map(str::to_owned),
                submitted_excerpt: finding.submitted_excerpt().map(str::to_owned),
                condition: finding.condition().map(str::to_owned),
            })
            .collect(),
        rendered_text: boxology_classifier::render_text(report),
    }
}

fn boundary_class(class: Class) -> CompatibilityClass {
    match class {
        Class::Unchanged => CompatibilityClass::Unchanged,
        Class::Documentation => CompatibilityClass::Documentation,
        Class::Deprecation => CompatibilityClass::Deprecation,
        Class::Additive => CompatibilityClass::Additive,
        Class::CompatibleWithConditions => CompatibilityClass::CompatibleWithConditions,
        Class::Incompatible => CompatibilityClass::Incompatible,
    }
}

/// Generated implementation adapter for composition assembly.
#[doc(hidden)]
pub mod generated {
    include!("../../generated/adapter/adapter.rs");
}
