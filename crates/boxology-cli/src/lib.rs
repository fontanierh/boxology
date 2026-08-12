//! Compatibility facade for the effectful Boxology command core.
//!
//! The installed `boxology` binary remains in this package. Reusable command behavior lives in
//! `boxology-cli-core` and is re-exported here so existing library consumers keep the same seam.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub use boxology_cli_core::*;
mod telegram;
pub use telegram::{TelegramComposition, run_telegram};

use std::{
    future::Future,
    pin::{Pin, pin},
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll, Waker},
};

use boxology_contract::{
    BoxId, CallContext, CallError, Caller, CancelToken, CapabilityDescriptor, CapabilityId,
    CapabilityShape, Detail, ErasedCallError, ErasedCallTarget, ExposureLevel, SlotValue,
    TraceContext,
};
use boxology_runtime::{
    Composition, CompositionBuilder, ImportTarget, TransportBinding, TransportExposure,
    TransportHandle, TransportJoinFuture, TransportRuntime,
};
use check_contract::{
    CheckFailureKind, CheckHandle, CheckOutcome, CheckRequest, CheckStatus, CheckStepStatus,
};
use classifier_contract::{
    ClassifierError, ClassifierHandle, ClassifyFailure, ClassifyFailureStage, ClassifyOutcome,
    ClassifyReport, ClassifyRequest, CompatibilityClass,
};

const INVALID_CLASSIFIER_OUTCOME: &str = "classifier call failed: invalid classifier outcome";
const INVALID_CHECK_OUTCOME: &str = "check call failed: invalid check outcome\n";
const CHECK_CALL_FAILED: &str = "check call failed\n";

/// Live local classifier and check boxes assembled for the installed CLI.
pub struct CheckComposition {
    _composition: Composition,
    handle: CheckHandle,
}

impl CheckComposition {
    /// Assembles check behind its generated typed handle and resolves its classifier import locally.
    pub fn start() -> Result<Self, String> {
        let classifier = classifier_implementation::generated::implementation_descriptor();
        let check = check_implementation::generated::implementation_descriptor();
        let [capability] = check.contract().capabilities() else {
            return Err("check contract must expose exactly one capability".into());
        };
        let binding = Arc::new(LocalBinding::default());
        let mut builder = CompositionBuilder::new();
        builder.add_box(classifier, |imports| {
            classifier_implementation::generated::factory(
                classifier_implementation::ClassifierService,
                imports,
            )
        });
        builder.add_box(check, |imports| {
            let dependencies = check_implementation::generated::typed_imports(&imports);
            check_implementation::generated::factory(
                check_implementation::CheckService::new(dependencies.classifier),
                imports,
            )
        });
        let check_id = BoxId::new("check").expect("check box id is valid");
        let classifier_id = BoxId::new("classifier").expect("classifier box id is valid");
        builder.resolve_import(
            check_id.clone(),
            classifier_id.clone(),
            ImportTarget::local(classifier_id),
        );
        builder.expose(
            check_id,
            capability.id().clone(),
            binding.clone(),
            ExposureLevel::CodeOnly,
        );
        let composition = builder.start().map_err(|error| error.to_string())?;
        let runtime = binding
            .runtime()
            .ok_or_else(|| "check in-process binding did not start".to_owned())?;
        let [exposure] = runtime.exposures() else {
            return Err("check composition must expose exactly one capability".into());
        };
        let handle = CheckHandle::from_erased(Arc::new(ExposureTarget(vec![exposure.clone()])));
        Ok(Self {
            _composition: composition,
            handle,
        })
    }

    /// Runs check through the generated handle with the installed CLI's exact workspace request.
    pub fn check(&self, base: Option<String>) -> Result<CheckOutcome, String> {
        invoke_check(&self.handle, base)
    }
}

/// Byte streams and exit status projected from the typed check boundary.
#[doc(hidden)]
#[derive(Debug, PartialEq, Eq)]
pub struct CheckProjection {
    /// Process exit status.
    pub code: u8,
    /// Bytes written to standard output.
    pub stdout: Vec<u8>,
    /// Bytes written to standard error.
    pub stderr: Vec<u8>,
}

/// Invokes a generated check handle with the installed CLI's exact request shape.
#[doc(hidden)]
pub fn invoke_check(handle: &CheckHandle, base: Option<String>) -> Result<CheckOutcome, String> {
    ready(handle.check(
        context(),
        CheckRequest {
            workspace: ".".into(),
            base,
        },
    ))
    .map_err(|_| "check call failed".to_owned())?
    .map_err(|_| "check call failed".to_owned())
}

/// Projects a typed check outcome to the installed CLI's legacy streams and status.
#[doc(hidden)]
pub fn project_check(outcome: Result<CheckOutcome, String>, json: bool) -> CheckProjection {
    let invalid = || CheckProjection {
        code: 1,
        stdout: Vec::new(),
        stderr: INVALID_CHECK_OUTCOME.as_bytes().to_vec(),
    };
    let outcome = match outcome {
        Ok(value) => value,
        Err(_) => {
            return CheckProjection {
                code: 1,
                stdout: Vec::new(),
                stderr: CHECK_CALL_FAILED.as_bytes().to_vec(),
            };
        }
    };
    match (outcome.report, outcome.failure) {
        (Some(report), None) => {
            if report
                .steps
                .iter()
                .any(|step| matches!(step.status, CheckStepStatus::Unknown { .. }))
            {
                return invalid();
            }
            let code = match report.status {
                CheckStatus::Passed => 0,
                CheckStatus::Failed => 1,
                CheckStatus::Unknown { .. } => return invalid(),
            };
            CheckProjection {
                code,
                stdout: if json { report.json } else { report.human },
                stderr: Vec::new(),
            }
        }
        (None, Some(failure)) => {
            let code = match failure.kind {
                CheckFailureKind::Validation => 1,
                CheckFailureKind::Invocation => 2,
                CheckFailureKind::Unknown { .. } => return invalid(),
            };
            CheckProjection {
                code,
                stdout: Vec::new(),
                stderr: if json { failure.json } else { failure.human },
            }
        }
        _ => invalid(),
    }
}

/// Live local classifier box assembled for the CLI composition.
pub struct ClassifierComposition {
    _composition: Composition,
    handle: ClassifierHandle,
}

impl ClassifierComposition {
    /// Assembles the classifier implementation behind its generated typed handle.
    pub fn start() -> Result<Self, String> {
        let descriptor = classifier_implementation::generated::implementation_descriptor();
        let [capability] = descriptor.contract().capabilities() else {
            return Err("classifier contract must expose exactly one capability".into());
        };
        let binding = Arc::new(LocalBinding::default());
        let mut builder = CompositionBuilder::new();
        builder.add_box(descriptor, |imports| {
            classifier_implementation::generated::factory(
                classifier_implementation::ClassifierService,
                imports,
            )
        });
        builder.expose(
            BoxId::new("classifier").expect("classifier box id is valid"),
            capability.id().clone(),
            binding.clone(),
            ExposureLevel::CodeOnly,
        );
        let composition = builder.start().map_err(|error| error.to_string())?;
        let runtime = binding
            .runtime()
            .ok_or_else(|| "classifier in-process binding did not start".to_owned())?;
        let [exposure] = runtime.exposures() else {
            return Err("classifier composition must expose exactly one capability".into());
        };
        let handle =
            ClassifierHandle::from_erased(Arc::new(ExposureTarget(vec![exposure.clone()])));
        Ok(Self {
            _composition: composition,
            handle,
        })
    }

    /// Classifies canonical schema bytes through the generated handle.
    pub fn classify(
        &self,
        base: Option<&[u8]>,
        submitted: &[u8],
    ) -> Result<ClassifyReport, String> {
        let request = ClassifyRequest {
            base: base.map(<[u8]>::to_vec),
            submitted: submitted.to_vec(),
        };
        match ready(self.handle.classify(context(), request))? {
            Ok(outcome) => outcome_report(outcome),
            Err(CallError::Domain(ClassifierError::Internal | ClassifierError::Unknown { .. })) => {
                Err(INVALID_CLASSIFIER_OUTCOME.into())
            }
            Err(error) => Err(format!("classifier call failed: {error}")),
        }
    }
}

fn outcome_report(outcome: ClassifyOutcome) -> Result<ClassifyReport, String> {
    match (outcome.report, outcome.failure) {
        (Some(report), None) if report_classes_are_known(&report) => Ok(report),
        (None, Some(failure)) => failure_message(failure),
        _ => Err(INVALID_CLASSIFIER_OUTCOME.into()),
    }
}

fn failure_message(failure: ClassifyFailure) -> Result<ClassifyReport, String> {
    let (code, stage, detail) = match failure.stage {
        ClassifyFailureStage::Base => (
            "BXW0077",
            "base",
            "the checked-in schema document must satisfy the strict format-1 reader",
        ),
        ClassifyFailureStage::Submitted => (
            "BXW0078",
            "submitted",
            "the regenerated schema document must satisfy the strict format-1 reader",
        ),
        ClassifyFailureStage::Pairing => (
            "BXW0079",
            "pairing",
            "the checked-in and regenerated schema documents must pair and satisfy classifier integrity",
        ),
        ClassifyFailureStage::Unknown { .. } => {
            return Err(INVALID_CLASSIFIER_OUTCOME.into());
        }
    };
    Err(format!("{code} {stage}: {detail}: {}", failure.diagnostics))
}

fn report_classes_are_known(report: &ClassifyReport) -> bool {
    class_is_known(&report.verdict)
        && report
            .findings
            .iter()
            .all(|finding| class_is_known(&finding.class))
}

fn class_is_known(class: &CompatibilityClass) -> bool {
    !matches!(class, CompatibilityClass::Unknown { .. })
}

fn context() -> CallContext {
    CallContext::new(
        Caller::Anonymous,
        None,
        CancelToken::new(),
        TraceContext::empty(),
        None,
    )
}

fn ready<F: Future>(future: F) -> Result<F::Output, String> {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => Ok(output),
        Poll::Pending => Err("local generated call unexpectedly pending".into()),
    }
}

#[derive(Default)]
struct LocalBinding {
    runtime: Mutex<Option<Weak<TransportRuntime<()>>>>,
}

impl LocalBinding {
    fn runtime(&self) -> Option<Arc<TransportRuntime<()>>> {
        self.runtime
            .lock()
            .expect("local binding lock poisoned")
            .as_ref()
            .and_then(Weak::upgrade)
    }
}

struct LocalHandle {
    _runtime: Arc<TransportRuntime<()>>,
}

impl TransportHandle for LocalHandle {
    fn stop_intake(&self) {}
    fn cancel_tasks(&self) {}
    fn abort_tasks(&self) {}
    fn join_tasks(self: Box<Self>) -> TransportJoinFuture {
        Box::pin(std::future::ready(Ok(())))
    }
}

impl TransportBinding for LocalBinding {
    type Config = ();
    type Handle = LocalHandle;

    fn config(&self) -> Arc<()> {
        Arc::new(())
    }

    fn conform(
        &self,
        descriptor: &CapabilityDescriptor,
        _level: ExposureLevel,
    ) -> Result<(), Detail> {
        match descriptor.shape() {
            CapabilityShape::Unary => Ok(()),
            _ => Err(Detail::new("unsupported_interaction_shape")),
        }
    }

    fn prepare(&self, _descriptors: &[&'static CapabilityDescriptor]) -> Result<(), Detail> {
        Ok(())
    }

    fn start(&self, runtime: TransportRuntime<()>) -> Result<LocalHandle, Detail> {
        let runtime = Arc::new(runtime);
        let mut retained = self.runtime.lock().expect("local binding lock poisoned");
        if retained.replace(Arc::downgrade(&runtime)).is_some() {
            return Err(Detail::new("local_binding_already_started"));
        }
        Ok(LocalHandle { _runtime: runtime })
    }
}

struct ExposureTarget(Vec<TransportExposure>);

impl ErasedCallTarget for ExposureTarget {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        match self
            .0
            .iter()
            .find(|exposure| exposure.descriptor().id() == capability)
        {
            Some(exposure) => exposure.dispatch(context, input),
            None => Box::pin(std::future::ready(Err(ErasedCallError::Internal(
                Detail::new("local_capability_mismatch"),
            )))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::{OpaquePayload, OpaqueTree};

    fn empty_report(verdict: CompatibilityClass) -> ClassifyReport {
        ClassifyReport {
            verdict,
            findings: Vec::new(),
            rendered_text: "classification unchanged\n".into(),
        }
    }

    fn unknown_class() -> CompatibilityClass {
        CompatibilityClass::Unknown {
            tag: "Future".into(),
            payload: OpaquePayload::new(OpaqueTree::Null),
        }
    }

    #[test]
    fn invalid_or_unknown_classifier_outcomes_fail_internally() {
        for outcome in [
            ClassifyOutcome {
                report: None,
                failure: None,
            },
            ClassifyOutcome {
                report: Some(empty_report(CompatibilityClass::Unchanged)),
                failure: Some(ClassifyFailure {
                    stage: ClassifyFailureStage::Base,
                    diagnostics: "diagnostic".into(),
                }),
            },
            ClassifyOutcome {
                report: Some(empty_report(unknown_class())),
                failure: None,
            },
            ClassifyOutcome {
                report: None,
                failure: Some(ClassifyFailure {
                    stage: ClassifyFailureStage::Unknown {
                        tag: "Future".into(),
                        payload: OpaquePayload::new(OpaqueTree::Null),
                    },
                    diagnostics: "diagnostic".into(),
                }),
            },
        ] {
            assert_eq!(
                outcome_report(outcome).unwrap_err(),
                INVALID_CLASSIFIER_OUTCOME
            );
        }
    }
}
