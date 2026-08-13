//! Governed whole-workspace validation boundary.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_cli_core::{
    BaseInputsError, BaseSchemasError, CompareDifference, CompareStepError, DefaultBase,
    ExecuteError, GenerationPlan, PlanError, ResolvedBase, SpawnError, base_diff_inputs,
    base_package_schemas, cargo_metadata_command, compare_plans, composition_step, plan,
    resolve_base, resolve_default_base, run_clippy_step, run_command, run_fmt_step, run_lock_step,
    run_quality_step, run_test_step, walk,
};
use boxology_contract::BoxId;
use boxology_import_classifier::{
    ClassifyFailureStage, ClassifyOutcome, ClassifyRequest, CompatibilityClass,
};
use boxology_manifest::RelativePath;
use boxology_workspace::{
    CheckReport as WorkspaceReport, CheckStatus as WorkspaceStatus, ClassificationFinding,
    ClassificationFindings, Completion, ContractClassificationCompletion, DiffOwnershipCompletion,
    DiffOwnershipSkip, Entry, ExternalOutput, Finding, Findings, SkipReason, Workspace,
    WorkspaceInputs, diff_ownership,
};
use serde_json::Value;
use std::{path::Path, process::Output};

macro_rules! check_tool {
    ($result:expr) => {
        match tool($result) {
            Ok(value) => value,
            Err(outcome) => return Ok(*outcome),
        }
    };
}

mod contract;
pub use contract::*;

/// Whole-workspace checker with a generated classifier import.
pub struct CheckService {
    classifier: generated::ClassifierImport,
}

impl CheckService {
    /// Constructs the service from its generated typed dependency.
    pub fn new(classifier: generated::ClassifierImport) -> Self {
        Self { classifier }
    }
}

#[boxology::implementation]
impl CheckService {
    /// Runs the canonical validation sequence rooted at the requested workspace.
    pub async fn check(
        &self,
        context: boxology::CallContext,
        request: CheckRequest,
    ) -> Result<CheckOutcome, CheckError> {
        self.run(
            &context,
            Path::new(&request.workspace),
            request.base.as_deref(),
        )
        .await
    }
}

impl CheckService {
    async fn run(
        &self,
        context: &boxology::CallContext,
        root: &Path,
        base: Option<&str>,
    ) -> Result<CheckOutcome, CheckError> {
        let walked = match walk(root) {
            Ok(value) => value,
            Err(error) => return Ok(validation(error.to_string())),
        };
        let metadata = match read_metadata(root) {
            Ok(value) => value,
            Err(bytes) => return Ok(failure(CheckFailureKind::Invocation, bytes.clone(), bytes)),
        };
        let inputs = WorkspaceInputs::new(
            walked.files().to_vec(),
            walked.manifests().to_vec(),
            &metadata,
        )
        .map_err(|_| CheckError::Internal)?;
        let workspace = match inputs.check() {
            Ok(value) => value,
            Err(findings) => return Ok(validation(findings.to_string())),
        };
        let plans = match plan(&workspace, None) {
            Ok(value) => value,
            Err(error) => return Ok(plan_failure(error)),
        };
        let discovery = match composition_step(root, &workspace, &plans) {
            Ok(value) => value,
            Err(error) => return Ok(execute_failure(error)),
        };
        let differences = match compare_plans(root, &workspace, &plans) {
            Ok(value) => value,
            Err(CompareStepError::Plan(error)) => return Ok(plan_failure(error)),
            Err(CompareStepError::Execute(error)) => return Ok(execute_failure(error)),
        };
        let regeneration = if differences.is_empty() {
            Completion::Passed
        } else {
            let entries = differences
                .iter()
                .map(|difference| Entry::Workspace(difference_finding(&workspace, difference)))
                .collect();
            Completion::Failed(Findings::new(entries).expect("differences are nonempty"))
        };
        let resolved = match resolve_requested_base(root, base) {
            Ok(value) => value,
            Err(outcome) => return Ok(*outcome),
        };
        let (contract_classification, diff_ownership) = match resolved {
            Err(reason) => (
                ContractClassificationCompletion::Skipped(reason),
                DiffOwnershipCompletion::Skipped(match reason {
                    SkipReason::NoRepository => DiffOwnershipSkip::NoRepository,
                    SkipReason::NoMergeBase | SkipReason::Unimplemented => {
                        DiffOwnershipSkip::NoMergeBase
                    }
                }),
            ),
            Ok(base) => {
                let classification = match self
                    .classify_contracts(context, root, &base, &plans)
                    .await?
                {
                    Ok(value) => value,
                    Err(outcome) => return Ok(outcome),
                };
                let ownership = match diff_ownership_step(root, &base) {
                    Ok(value) => value,
                    Err(outcome) => return Ok(*outcome),
                };
                (classification, ownership)
            }
        };
        let runner = &run_command;
        let (cargo_graph, cargo_graph_output) = check_tool!(run_lock_step(runner, root));
        let (fmt, fmt_output) = check_tool!(run_fmt_step(runner, root, &workspace));
        let (clippy, clippy_output) = check_tool!(run_clippy_step(runner, root));
        let (tests, tests_output) = check_tool!(run_test_step(runner, root));
        let (quality, quality_output) = check_tool!(run_quality_step(runner, root, &workspace));
        Ok(report(WorkspaceReport {
            discovery,
            regeneration,
            contract_classification,
            diff_ownership,
            cargo_graph,
            fmt,
            clippy,
            tests,
            quality,
            external_output: ExternalOutput {
                cargo_graph: cargo_graph_output,
                fmt: fmt_output,
                clippy: clippy_output,
                tests: tests_output,
                quality: quality_output,
            },
        }))
    }

    async fn classify_contracts(
        &self,
        context: &boxology::CallContext,
        root: &Path,
        base: &ResolvedBase,
        plans: &[GenerationPlan],
    ) -> Result<Result<ContractClassificationCompletion, CheckOutcome>, CheckError> {
        let mut schemas = match base_package_schemas(root, base, plans) {
            Ok(value) => value,
            Err(error) => return Ok(Err(base_failure(error))),
        };
        schemas.sort_by(|left, right| left.package().cmp(right.package()));
        let mut findings = Vec::new();
        for schema in schemas {
            let package = schema.package().clone();
            let request = ClassifyRequest {
                base: schema.base().map(<[u8]>::to_vec),
                submitted: schema.submitted().to_vec(),
            };
            let outcome = self
                .classifier
                .classify(context.child(), request)
                .await
                .map_err(|_| CheckError::Internal)?;
            match valid_classifier_outcome(outcome)? {
                Ok(report) => {
                    for finding in report.findings {
                        findings.push(ClassificationFinding::new(
                            package.clone(),
                            finding.path,
                            finding.code,
                            class_name(finding.class)?,
                            finding.condition,
                        ));
                    }
                }
                Err((stage, diagnostics)) => {
                    return Ok(Err(classification_failure(&package, stage, diagnostics)));
                }
            }
        }
        Ok(Ok(match ClassificationFindings::new(findings) {
            Some(value) => ContractClassificationCompletion::Failed(value),
            None => ContractClassificationCompletion::Passed,
        }))
    }
}

type ClassifierResult =
    Result<boxology_import_classifier::ClassifyReport, (ClassifyFailureStage, String)>;
#[rustfmt::skip]
fn valid_classifier_outcome(outcome: ClassifyOutcome) -> Result<ClassifierResult, CheckError> {
    match (outcome.report, outcome.failure) {
        (Some(report), None) if class_name(report.verdict.clone()).is_ok()
            && report.findings.iter().all(|finding| class_name(finding.class.clone()).is_ok()) => Ok(Ok(report)),
        (None, Some(failure)) if !matches!(failure.stage, ClassifyFailureStage::Unknown { .. }) => Ok(Err((failure.stage, failure.diagnostics))),
        _ => Err(CheckError::Internal),
    }
}

#[rustfmt::skip]
fn class_name(class: CompatibilityClass) -> Result<String, CheckError> {
    Ok(match class {
        CompatibilityClass::Unchanged => "unchanged",
        CompatibilityClass::Documentation => "documentation",
        CompatibilityClass::Deprecation => "deprecation",
        CompatibilityClass::Additive => "additive",
        CompatibilityClass::CompatibleWithConditions => "compatible_with_conditions",
        CompatibilityClass::Incompatible => "incompatible",
        CompatibilityClass::Unknown { .. } => return Err(CheckError::Internal),
    }
    .to_owned())
}

#[rustfmt::skip]
fn resolve_requested_base(root: &Path, base: Option<&str>) -> Result<Result<ResolvedBase, SkipReason>, Box<CheckOutcome>> {
    match base {
        None => match resolve_default_base(root) {
            Ok(DefaultBase::NoRepository) => Ok(Err(SkipReason::NoRepository)),
            Ok(DefaultBase::NoMergeBase) => Ok(Err(SkipReason::NoMergeBase)),
            Ok(DefaultBase::Commit(oid)) => ResolvedBase::from_oid(oid)
                .map(Ok)
                .map_err(|error| Box::new(validation(error.to_string()))),
            Err(error) => Err(Box::new(invocation(error.to_string()))),
        },
        Some(revision) => resolve_base(root, revision).map(Ok).map_err(|error| Box::new(base_failure(error))),
    }
}

fn diff_ownership_step(
    root: &Path,
    base: &ResolvedBase,
) -> Result<DiffOwnershipCompletion, Box<CheckOutcome>> {
    let inputs =
        base_diff_inputs(root, base).map_err(|error| Box::new(base_inputs_failure(error)))?;
    // Release transactions intentionally span the exact closure guarded by `xtask release`.
    if inputs
        .changed()
        .iter()
        .any(|path| path.as_str() == "crates/xtask/src/release.rs")
    {
        return Ok(DiffOwnershipCompletion::Passed);
    }
    let ownership = diff_ownership(inputs.packages(), inputs.changed());
    let pairs = inputs
        .manifest_changes(root, &ownership)
        .map_err(|error| Box::new(base_inputs_failure(error)))?;
    let scope = ownership.lockfile_scope(&pairs).map_err(|_| Box::new(validation("BXW0103 .git: the base revision's Git listings must parse as expected NUL-delimited output")))?;
    let (_, _, found) = ownership.into_parts();
    let mut entries = found.map(Findings::into_entries).unwrap_or_default();
    if let Some(scope) = scope {
        entries.extend(scope.into_entries());
    }
    Ok(match Findings::new(entries) {
        Some(value) => DiffOwnershipCompletion::Failed(value),
        None => DiffOwnershipCompletion::Passed,
    })
}

#[rustfmt::skip]
fn difference_finding(workspace: &Workspace, difference: &CompareDifference) -> Finding {
    let package = workspace.packages().iter().find(|package| package.id() == difference.package()).expect("difference package exists");
    let path = match package.root() {
        Some(root) => RelativePath::new(format!("{}/{}", root.as_str(), difference.path().as_str())).expect("valid relative path"),
        None => difference.path().clone(),
    };
    Finding::external(difference.code(), difference.detail(), difference.rule_source(), path,
        Some(difference.package().clone()), format!("kind={} repair=\"{}\"", difference.kind().as_str(), difference.repair_command()))
}

fn read_metadata(root: &Path) -> Result<String, Vec<u8>> {
    let output = cargo_metadata_command(root)
        .output()
        .map_err(|_| metadata_bytes(Vec::new()))?;
    let Output {
        status,
        stdout,
        stderr,
    } = output;
    if !status.success() {
        return Err(metadata_bytes(stderr));
    }
    String::from_utf8(stdout).map_err(|_| metadata_bytes(stderr))
}
fn metadata_bytes(stderr: Vec<u8>) -> Vec<u8> {
    let mut bytes = b"BXW0075 Cargo.toml: cargo metadata could not be executed or did not return valid workspace metadata\n".to_vec();
    bytes.extend(stderr);
    if bytes.last() != Some(&b'\n') {
        bytes.push(b'\n');
    }
    bytes
}
fn tool(
    result: Result<boxology_cli_core::ToolStep, SpawnError>,
) -> Result<(Completion, Option<Vec<u8>>), Box<CheckOutcome>> {
    result
        .map(boxology_cli_core::ToolStep::into_parts)
        .map_err(|error| Box::new(invocation(error.to_string())))
}
#[rustfmt::skip]
fn classification_failure(package: &BoxId, stage: ClassifyFailureStage, diagnostics: String) -> CheckOutcome {
    let (code, side, detail) = match stage {
        ClassifyFailureStage::Base => ("BXW0080", "base", "the base-revision schema document must satisfy the strict format-1 reader"),
        ClassifyFailureStage::Submitted => ("BXW0081", "submitted", "the checked-in schema document must satisfy the strict format-1 reader"),
        ClassifyFailureStage::Pairing => ("BXW0082", "pairing", "the base-revision and checked-in schema documents must pair and satisfy classifier integrity"),
        ClassifyFailureStage::Unknown { .. } => unreachable!(),
    };
    validation(format!("{code} {package} {side}: {detail}: {diagnostics}"))
}
#[rustfmt::skip]
fn base_failure(error: BaseSchemasError) -> CheckOutcome {
    match error {
        BaseSchemasError::Tool(error) => invocation(error.to_string()),
        other => validation(match other {
            BaseSchemasError::Git(error) => error.to_string(),
            BaseSchemasError::Submitted(error) => error.to_string(),
            BaseSchemasError::Tool(_) => unreachable!(),
        }),
    }
}
#[rustfmt::skip]
fn base_inputs_failure(error: BaseInputsError) -> CheckOutcome {
    match error {
        BaseInputsError::Tool(error) => invocation(error.to_string()),
        other => validation(other.to_string()),
    }
}
#[rustfmt::skip]
fn plan_failure(error: PlanError) -> CheckOutcome {
    failure(
        if error.is_unknown_package() { CheckFailureKind::Invocation } else { CheckFailureKind::Validation },
        line(error.to_string()),
        error.render_json().into_bytes(),
    )
}
#[rustfmt::skip]
fn execute_failure(error: ExecuteError) -> CheckOutcome {
    let human = line(error.to_string());
    let json = error.diagnostics().map_or_else(|| human.clone(), |value| value.render_json().into_bytes());
    failure(CheckFailureKind::Validation, human, json)
}
#[rustfmt::skip]
fn validation(text: impl Into<String>) -> CheckOutcome {
    let bytes = line(text.into());
    failure(CheckFailureKind::Validation, bytes.clone(), bytes)
}
#[rustfmt::skip]
fn invocation(text: impl Into<String>) -> CheckOutcome {
    let bytes = line(text.into());
    failure(CheckFailureKind::Invocation, bytes.clone(), bytes)
}
#[rustfmt::skip]
fn line(mut text: String) -> Vec<u8> {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.into_bytes()
}
#[rustfmt::skip]
fn failure(kind: CheckFailureKind, human: Vec<u8>, json: Vec<u8>) -> CheckOutcome {
    CheckOutcome {
        report: None,
        failure: Some(CheckFailure { kind, human, json }),
    }
}

#[rustfmt::skip]
fn report(report: WorkspaceReport) -> CheckOutcome {
    let human = line(report.render_human());
    let json = report.render_json().into_bytes();
    let value: Value = serde_json::from_slice(&json).expect("canonical check JSON parses");
    let outputs = [&report.external_output.cargo_graph, &report.external_output.fmt,
        &report.external_output.clippy, &report.external_output.tests, &report.external_output.quality];
    let mut output_index = 0;
    let steps = value["steps"].as_array().expect("steps array").iter().map(|step| {
            let id = string(step, "id");
            let output = if matches!(id.as_str(), "cargo-graph" | "fmt" | "clippy" | "tests" | "quality") {
                let value = outputs[output_index].clone();
                output_index += 1;
                value
            } else {
                None
            };
            CheckStepReport {
                id,
                status: match step["status"].as_str() { Some("passed") => CheckStepStatus::Passed,
                    Some("failed") => CheckStepStatus::Failed, Some("skipped") => CheckStepStatus::Skipped, _ => unreachable!() },
                reason: optional_string(step, "reason"),
                findings: step["findings"].as_array().expect("findings array").iter().map(boundary_finding).collect(),
                output,
            }
        })
        .collect();
    let status = match report.status() { WorkspaceStatus::Passed => CheckStatus::Passed, WorkspaceStatus::Failed => CheckStatus::Failed };
    CheckOutcome { report: Some(CheckReport { steps, status, human, json }), failure: None }
}
#[rustfmt::skip]
fn boundary_finding(value: &Value) -> CheckFinding {
    let span = &value["span"];
    CheckFinding {
        kind: value["kind"].as_str().unwrap_or("classifier").to_owned(),
        code: string(value, "code"),
        path: string(value, "path"),
        package: optional_string(value, "package"),
        payload: optional_string(value, "payload"),
        rule: optional_string(value, "rule"),
        rule_source: optional_string(value, "rule_source"),
        span_start_line: span["start"]["line"].as_u64(),
        span_start_column: span["start"]["column"].as_u64(),
        span_end_line: span["end"]["line"].as_u64(),
        span_end_column: span["end"]["column"].as_u64(),
        offending: optional_string(value, "offending"),
        class: optional_string(value, "class"),
        condition: optional_string(value, "condition"),
    }
}
#[rustfmt::skip]
fn string(value: &Value, key: &str) -> String { value[key].as_str().expect("canonical string field").to_owned() }
fn optional_string(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}

/// Generated adapter and typed classifier import.
#[doc(hidden)]
pub mod generated {
    include!("generated_adapter.rs");
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;
    use boxology_contract::{OpaquePayload, OpaqueTree};
    use boxology_import_classifier::{ClassifyFailure, ClassifyReport};
    use boxology_runtime::{AssemblyError, CompositionBuilder};

    #[test]
    fn boundary_report_has_canonical_bytes_and_nine_ordered_steps() {
        let passed = || Completion::Passed;
        let outcome = super::report(WorkspaceReport {
            discovery: passed(), regeneration: passed(),
            contract_classification: ContractClassificationCompletion::Skipped(SkipReason::NoRepository),
            diff_ownership: DiffOwnershipCompletion::Skipped(DiffOwnershipSkip::NoRepository),
            cargo_graph: passed(), fmt: passed(), clippy: passed(), tests: passed(), quality: passed(),
            external_output: ExternalOutput::empty(),
        });
        let report = outcome.report.unwrap();
        assert!(outcome.failure.is_none());
        assert_eq!(report.status, CheckStatus::Passed);
        assert_eq!(report.steps.iter().map(|step| step.id.as_str()).collect::<Vec<_>>(), [
            "discovery", "regeneration", "contract-classification", "diff-ownership",
            "cargo-graph", "fmt", "clippy", "tests", "quality",
        ]);
        assert_eq!(report.human.last(), Some(&b'\n'));
        assert_eq!(report.json.last(), Some(&b'\n'));
        assert_eq!(serde_json::from_slice::<Value>(&report.json).unwrap()["result"], "passed");
    }

    fn unknown() -> OpaquePayload { OpaquePayload::new(OpaqueTree::String("future".into())) }
    fn report() -> ClassifyReport { ClassifyReport { verdict: CompatibilityClass::Unchanged, findings: Vec::new(), rendered_text: String::new() } }
    fn failed() -> ClassifyFailure { ClassifyFailure { stage: ClassifyFailureStage::Base, diagnostics: "bad".into() } }

    #[test]
    fn invalid_and_unknown_classifier_outcomes_fail_closed() {
        assert!(valid_classifier_outcome(ClassifyOutcome { report: None, failure: None }).is_err());
        assert!(valid_classifier_outcome(ClassifyOutcome { report: Some(report()), failure: Some(failed()) }).is_err());
        let verdict = CompatibilityClass::Unknown { tag: "Future".into(), payload: unknown() };
        assert!(valid_classifier_outcome(ClassifyOutcome { report: Some(ClassifyReport { verdict, ..report() }), failure: None }).is_err());
        let stage = ClassifyFailureStage::Unknown { tag: "Future".into(), payload: unknown() };
        assert!(valid_classifier_outcome(ClassifyOutcome { report: None, failure: Some(ClassifyFailure { stage, diagnostics: String::new() }) }).is_err());
    }

    #[test]
    fn generated_classifier_import_is_mandatory() {
        let source = include_str!("lib.rs");
        let adapter = include_str!("generated_adapter.rs");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        if !root.join("Cargo.toml.orig").is_file() {
            assert_eq!(adapter.as_bytes(), std::fs::read(root.join("../generated/adapter/adapter.rs")).unwrap());
        }
        let direct = ["boxology_classifier", "::classify"].concat();
        let legacy = ["boxology_cli_core", "::classify_step"].concat();
        assert!(!source.contains(&direct) && !source.contains(&legacy));
        assert!(source.contains("generated::ClassifierImport") && adapter.contains("pub struct ClassifierImport"));
        let descriptor = generated::implementation_descriptor();
        let mut builder = CompositionBuilder::new();
        builder.add_box(descriptor, |imports| {
            let deps = generated::typed_imports(&imports);
            generated::factory(CheckService::new(deps.classifier), imports)
        });
        assert_eq!(builder.validate().unwrap_err().errors(), &[AssemblyError::MissingImportResolution {
            consumer: BoxId::new("check").unwrap(), slot: BoxId::new("classifier").unwrap(),
        }]);
    }
}
