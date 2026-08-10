//! Compatibility facade for the effectful Boxology command core.
//!
//! The installed `boxology` binary remains in this package. Reusable command behavior lives in
//! `boxology-cli-core` and is re-exported here so existing library consumers keep the same seam.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub use boxology_cli_core::*;

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
    Composition, CompositionBuilder, TransportBinding, TransportExposure, TransportHandle,
    TransportJoinFuture, TransportRuntime,
};
use classifier_contract::{
    ClassifierError, ClassifierHandle, ClassifyFailure, ClassifyFailureStage, ClassifyOutcome,
    ClassifyReport, ClassifyRequest, CompatibilityClass,
};

const INVALID_CLASSIFIER_OUTCOME: &str = "classifier call failed: invalid classifier outcome";

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
        let handle = ClassifierHandle::from_erased(Arc::new(ExposureTarget(exposure.clone())));
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
        Poll::Pending => Err("pure classifier call unexpectedly pending".into()),
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
            .expect("classifier binding lock poisoned")
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
        let mut retained = self
            .runtime
            .lock()
            .expect("classifier binding lock poisoned");
        if retained.replace(Arc::downgrade(&runtime)).is_some() {
            return Err(Detail::new("classifier_binding_already_started"));
        }
        Ok(LocalHandle { _runtime: runtime })
    }
}

struct ExposureTarget(TransportExposure);

impl ErasedCallTarget for ExposureTarget {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        if capability != self.0.descriptor().id() {
            return Box::pin(std::future::ready(Err(ErasedCallError::Internal(
                Detail::new("classifier_capability_mismatch"),
            ))));
        }
        self.0.dispatch(context, input)
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
