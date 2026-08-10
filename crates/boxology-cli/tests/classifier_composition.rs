use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

use boxology_cli::{ClassifierComposition, classify};
use boxology_contract::{CallContext, Caller, CancelToken, TraceContext};
use classifier_contract::{
    ClassifierHandle, ClassifyFailure, ClassifyFailureStage, ClassifyFinding, ClassifyOutcome,
    ClassifyReport, ClassifyRequest, CompatibilityClass, test_support::ClassifierFake,
};

const HELLO: &[u8] = include_bytes!("../../fixtures/hello/generated/schema.json");
const PING: &[u8] = include_bytes!("../../fixtures/ping/generated/schema.json");

fn context() -> CallContext {
    CallContext::new(
        Caller::Anonymous,
        None,
        CancelToken::new(),
        TraceContext::empty(),
        None,
    )
}

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("pure classifier future unexpectedly pending"),
    }
}

#[test]
fn real_composition_preserves_reports_and_failures() {
    let classifier = ClassifierComposition::start().expect("classifier composition starts");
    let report = classifier
        .classify(None, HELLO)
        .expect("introduction classifies");
    assert_eq!(report.verdict, CompatibilityClass::Additive);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].code, "BXC0026");
    assert_eq!(
        report.rendered_text,
        boxology_classifier::render_text(&classify(None, HELLO).unwrap())
    );

    for (base, submitted) in [
        (Some(b"{".as_slice()), HELLO),
        (None, b"{".as_slice()),
        (Some(HELLO), PING),
    ] {
        assert_eq!(
            classifier.classify(base, submitted).unwrap_err(),
            classify(base, submitted).unwrap_err().to_string()
        );
    }
}

#[test]
fn generated_fake_carries_success_and_every_failure_stage() {
    let expected = ClassifyOutcome {
        report: Some(ClassifyReport {
            verdict: CompatibilityClass::CompatibleWithConditions,
            findings: vec![ClassifyFinding {
                code: "BXC0068".into(),
                path: "box/type/Mode/variant/Future".into(),
                kind: "output enum variant added".into(),
                class: CompatibilityClass::CompatibleWithConditions,
                base_excerpt: None,
                submitted_excerpt: Some("Future".into()),
                condition: Some("unknown-variant tolerance".into()),
            }],
            rendered_text: "classification compatible_with_conditions\n".into(),
        }),
        failure: None,
    };
    let returned = expected.clone();
    let fake = ClassifierFake::new().with_classify(move |_context, request| {
        assert_eq!(request.base, Some(vec![1, 2]));
        assert_eq!(request.submitted, vec![3, 4]);
        let returned = returned.clone();
        async move { Ok(returned) }
    });
    let handle: ClassifierHandle = fake.handle();
    let actual = ready(handle.classify(
        context(),
        ClassifyRequest {
            base: Some(vec![1, 2]),
            submitted: vec![3, 4],
        },
    ))
    .expect("programmed fake returns its typed outcome");
    assert_eq!(actual, expected);

    for stage in [
        ClassifyFailureStage::Base,
        ClassifyFailureStage::Submitted,
        ClassifyFailureStage::Pairing,
    ] {
        let expected = ClassifyOutcome {
            report: None,
            failure: Some(ClassifyFailure {
                stage,
                diagnostics: "coded diagnostics".into(),
            }),
        };
        let returned = expected.clone();
        let fake = ClassifierFake::new().with_classify(move |_context, _request| {
            let returned = returned.clone();
            async move { Ok(returned) }
        });
        let actual = ready(fake.handle().classify(
            context(),
            ClassifyRequest {
                base: None,
                submitted: Vec::new(),
            },
        ))
        .expect("programmed fake returns its typed failure");
        assert_eq!(actual, expected);
    }
}

#[test]
fn generated_fake_carries_constructible_invalid_outcomes() {
    for expected in [
        ClassifyOutcome {
            report: None,
            failure: None,
        },
        ClassifyOutcome {
            report: Some(ClassifyReport {
                verdict: CompatibilityClass::Unchanged,
                findings: Vec::new(),
                rendered_text: "classification unchanged\n".into(),
            }),
            failure: Some(ClassifyFailure {
                stage: ClassifyFailureStage::Base,
                diagnostics: "diagnostic".into(),
            }),
        },
    ] {
        let returned = expected.clone();
        let fake = ClassifierFake::new().with_classify(move |_context, _request| {
            let returned = returned.clone();
            async move { Ok(returned) }
        });
        let actual = ready(fake.handle().classify(
            context(),
            ClassifyRequest {
                base: None,
                submitted: Vec::new(),
            },
        ))
        .expect("generated fake transports the invalid combination for consumer testing");
        assert_eq!(actual, expected);
    }
}
