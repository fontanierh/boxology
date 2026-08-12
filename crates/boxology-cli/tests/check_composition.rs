use boxology_cli::{CheckComposition, invoke_check, project_check};
use boxology_contract::{
    CallContext, CapabilityId, Detail, ErasedCallError, ErasedCallTarget, OpaquePayload,
    OpaqueTree, SlotValue,
};
use check_contract::{
    CheckError, CheckFailure, CheckFailureKind, CheckHandle, CheckOutcome, CheckReport,
    CheckStatus, CheckStepReport, CheckStepStatus, test_support::CheckFake,
};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

fn report(status: CheckStatus) -> CheckOutcome {
    CheckOutcome {
        report: Some(CheckReport {
            steps: Vec::new(),
            status,
            human: b"human\n".to_vec(),
            json: b"{\"result\":\"passed\"}\n".to_vec(),
        }),
        failure: None,
    }
}

#[test]
fn generated_handle_receives_exact_installed_request_and_projects_report() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let fake = CheckFake::new().with_check({
        let requests = requests.clone();
        move |_, request| {
            requests.lock().unwrap().push(request);
            async { Ok(report(CheckStatus::Passed)) }
        }
    });

    let human = project_check(invoke_check(&fake.handle(), Some("main".into())), false);
    let json = project_check(invoke_check(&fake.handle(), None), true);

    let requests = requests.lock().unwrap();
    assert_eq!(requests[0].workspace, ".");
    assert_eq!(requests[0].base.as_deref(), Some("main"));
    assert_eq!(requests[1].workspace, ".");
    assert_eq!(requests[1].base, None);
    assert_eq!(
        (human.code, human.stdout, human.stderr),
        (0, b"human\n".to_vec(), Vec::new())
    );
    assert_eq!(
        (json.code, json.stdout, json.stderr),
        (0, b"{\"result\":\"passed\"}\n".to_vec(), Vec::new())
    );
}

#[test]
fn typed_failures_and_malformed_outcomes_preserve_legacy_exit_semantics() {
    for (kind, expected) in [
        (CheckFailureKind::Validation, 1),
        (CheckFailureKind::Invocation, 2),
    ] {
        let projected = project_check(
            Ok(CheckOutcome {
                report: None,
                failure: Some(CheckFailure {
                    kind,
                    human: b"human failure\n".to_vec(),
                    json: b"{\"failure\":true}\n".to_vec(),
                }),
            }),
            true,
        );
        assert_eq!(projected.code, expected);
        assert!(projected.stdout.is_empty());
        assert_eq!(projected.stderr, b"{\"failure\":true}\n");
    }

    for malformed in [
        CheckOutcome {
            report: None,
            failure: None,
        },
        CheckOutcome {
            report: report(CheckStatus::Failed).report,
            failure: Some(CheckFailure {
                kind: CheckFailureKind::Validation,
                human: Vec::new(),
                json: Vec::new(),
            }),
        },
    ] {
        let projected = project_check(Ok(malformed), false);
        assert_eq!(projected.code, 1);
        assert!(projected.stdout.is_empty());
        assert_eq!(
            projected.stderr,
            b"check call failed: invalid check outcome\n"
        );
    }
}

#[test]
fn unknown_step_status_rejects_even_a_passed_report_without_emitting_supplied_bytes() {
    let projected = project_check(
        Ok(CheckOutcome {
            report: Some(CheckReport {
                steps: vec![CheckStepReport {
                    id: "future".into(),
                    status: CheckStepStatus::Unknown {
                        tag: "SentinelStep".into(),
                        payload: OpaquePayload::new(OpaqueTree::Null),
                    },
                    reason: None,
                    findings: Vec::new(),
                    output: None,
                }],
                status: CheckStatus::Passed,
                human: b"SENTINEL HUMAN\n".to_vec(),
                json: b"SENTINEL JSON\n".to_vec(),
            }),
            failure: None,
        }),
        false,
    );
    assert_eq!(projected.code, 1);
    assert!(projected.stdout.is_empty());
    assert_eq!(
        projected.stderr,
        b"check call failed: invalid check outcome\n"
    );
}

struct SentinelTarget;

impl ErasedCallTarget for SentinelTarget {
    fn call<'a>(
        &'a self,
        _: &'a CapabilityId,
        _: CallContext,
        _: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        Box::pin(std::future::ready(Err(ErasedCallError::Internal(
            Detail::new("sentinel_code").with_message("SENTINEL PROVIDER DETAIL"),
        ))))
    }
}

struct InvalidResponseTarget;

impl ErasedCallTarget for InvalidResponseTarget {
    fn call<'a>(
        &'a self,
        _: &'a CapabilityId,
        _: CallContext,
        _: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        Box::pin(std::future::ready(Ok(SlotValue::Null)))
    }
}

#[test]
fn generated_handle_failures_are_fixed_and_never_expose_provider_detail() {
    let domain = CheckFake::new().with_check(|_, _| async { Err(CheckError::Internal) });
    let handles = [
        domain.handle(),
        CheckHandle::from_erased(Arc::new(SentinelTarget)),
        CheckHandle::from_erased(Arc::new(InvalidResponseTarget)),
    ];
    for handle in handles {
        let projected = project_check(invoke_check(&handle, None), false);
        assert_eq!(projected.code, 1);
        assert!(projected.stdout.is_empty());
        assert_eq!(projected.stderr, b"check call failed\n");
        assert!(!String::from_utf8_lossy(&projected.stderr).contains("SENTINEL"));
    }
}

#[test]
fn production_check_and_classifier_composition_assembles() {
    CheckComposition::start().expect("production check composition assembles");
}
