#![cfg(unix)]

use boxology_contract::{
    BoxId, CallContext, CallError, Caller, CancelToken, CapabilityId, ErasedCallError,
    ErasedCallTarget, ExposureLevel, SlotValue, TraceContext,
};
use boxology_generated_contract::{
    CheckError, CheckFailureKind, CheckFinding, CheckHandle, CheckRequest, CheckStatus,
    CheckStepStatus,
};
use boxology_import_classifier::{
    ClassifyFailure, ClassifyFailureStage, ClassifyFinding, ClassifyOutcome, ClassifyReport,
    CompatibilityClass, test_support::ClassifierFake,
};
use boxology_runtime::{
    Composition, CompositionBuilder, ImportTarget, RemoteImportTarget, TransportExposure,
    test_support::StubTransport,
};
use boxology_workspace::WorkspaceInputs;
use check_implementation::{CheckService, generated};
use std::{
    env,
    ffi::OsString,
    fs,
    future::Future,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    pin::{Pin, pin},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
};

const ROOT_MANIFEST: &str = "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n";
const METADATA: &str = r#"{"workspace_root":"/w","workspace_members":["path+file:///w/ping/generated/contract#0.0.0","path+file:///w/ping/implementation#0.0.0"],"packages":[{"id":"path+file:///w/ping/generated/contract#0.0.0","name":"ping-contract","manifest_path":"/w/ping/generated/contract/Cargo.toml","dependencies":[]},{"id":"path+file:///w/ping/implementation#0.0.0","name":"ping-implementation","manifest_path":"/w/ping/implementation/Cargo.toml","dependencies":[]}]}"#;
const CLEAN_JSON: &[u8] = b"{\n  \"schema\": \"boxology.check-report@1\",\n  \"steps\": [\n    {\n      \"id\": \"discovery\",\n      \"status\": \"passed\",\n      \"findings\": []\n    },\n    {\n      \"id\": \"regeneration\",\n      \"status\": \"passed\",\n      \"findings\": []\n    },\n    {\n      \"id\": \"contract-classification\",\n      \"status\": \"skipped\",\n      \"reason\": \"contract classification skipped: no repository is available\",\n      \"findings\": []\n    },\n    {\n      \"id\": \"diff-ownership\",\n      \"status\": \"skipped\",\n      \"reason\": \"not run: no repository is available\",\n      \"findings\": []\n    },\n    {\n      \"id\": \"cargo-graph\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    },\n    {\n      \"id\": \"fmt\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    },\n    {\n      \"id\": \"clippy\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    },\n    {\n      \"id\": \"tests\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    },\n    {\n      \"id\": \"quality\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    }\n  ],\n  \"result\": \"passed\"\n}\n";
const CLEAN_HUMAN: &[u8] = b"check discovery passed\ncheck regeneration passed\ncheck contract-classification skipped\n  contract classification skipped: no repository is available\ncheck diff-ownership skipped\n  not run: no repository is available\ncheck cargo-graph passed\ncheck fmt passed\ncheck clippy passed\ncheck tests passed\ncheck quality passed\ncheck result passed\n";
const OID: &str = "0000000000000000000000000000000000000000";
static NEXT: AtomicU64 = AtomicU64::new(0);

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

struct Fixture {
    home: PathBuf,
    root: PathBuf,
    bin: PathBuf,
    metadata: PathBuf,
    base: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = env::temp_dir().join(format!(
            "boxology-check-handle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = home.join("workspace");
        let bin = home.join("bin");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&bin).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ping");
        fs::create_dir_all(root.join("ping")).unwrap();
        fs::copy(
            source.join("boxology.toml"),
            root.join("ping/boxology.toml"),
        )
        .unwrap();
        copy_tree(
            &source.join("implementation"),
            &root.join("ping/implementation"),
        );
        copy_tree(&source.join("generated"), &root.join("ping/generated"));
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"ping/implementation\", \"ping/generated/contract\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.lock"), b"").unwrap();
        fs::write(root.join("boxology.toml"), ROOT_MANIFEST).unwrap();
        let metadata = home.join("metadata.json");
        let base = home.join("base.json");
        fs::write(&metadata, METADATA).unwrap();
        fs::write(&base, b"base-schema-exact").unwrap();
        let cargo = bin.join("cargo");
        fs::write(
            &cargo,
            "#!/bin/sh\nif [ \"$1\" = metadata ]; then\n  if [ \"${BOXOLOGY_TEST_METADATA_FAIL:-}\" = 1 ]; then printf '%s\\n' 'synthetic metadata stderr' >&2; exit 17; fi\n  /bin/cat \"$BOXOLOGY_TEST_METADATA\"; exit 0\nfi\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        let fixture = Self {
            home,
            root,
            bin,
            metadata,
            base,
        };
        fixture.regenerate();
        fixture
    }

    fn regenerate(&self) {
        let walked = boxology_cli_core::walk(&self.root).unwrap();
        let inputs = WorkspaceInputs::new(
            walked.files().to_vec(),
            walked.manifests().to_vec(),
            METADATA,
        )
        .unwrap();
        let workspace = inputs.check().unwrap();
        for plan in boxology_cli_core::plan(&workspace, None).unwrap() {
            boxology_cli_core::execute(&self.root, &plan).unwrap();
        }
    }

    fn install_git(&self) {
        let git = self.bin.join("git");
        fs::write(
            &git,
            "#!/bin/sh\ncase \"$1 $2\" in\n 'rev-parse --verify') printf '%s\\n' \"$BOXOLOGY_TEST_OID\";;\n 'ls-tree --name-only') printf '%s\\0' \"$6\";;\n 'cat-file -e') exit 0;;\n 'cat-file blob') /bin/cat \"$BOXOLOGY_TEST_BASE\";;\n 'ls-tree -r'|'diff --name-only') exit 0;;\n *) exit 19;;\nesac\n",
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn environment(&self, metadata_failure: bool) -> Environment {
        Environment::set([
            (
                "PATH",
                env::join_paths([self.bin.as_path(), Path::new("/usr/bin"), Path::new("/bin")])
                    .unwrap(),
            ),
            (
                "GIT_CEILING_DIRECTORIES",
                self.root.clone().into_os_string(),
            ),
            (
                "BOXOLOGY_TEST_METADATA",
                self.metadata.clone().into_os_string(),
            ),
            ("BOXOLOGY_TEST_BASE", self.base.clone().into_os_string()),
            ("BOXOLOGY_TEST_OID", OsString::from(OID)),
            (
                "BOXOLOGY_TEST_METADATA_FAIL",
                OsString::from(if metadata_failure { "1" } else { "0" }),
            ),
        ])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.home).unwrap();
    }
}

struct Environment {
    previous: Vec<(&'static str, Option<OsString>)>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Environment {
    fn set<const N: usize>(values: [(&'static str, OsString); N]) -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = values
            .iter()
            .map(|(key, _)| (*key, env::var_os(key)))
            .collect();
        for (key, value) in values {
            unsafe { env::set_var(key, value) };
        }
        Self {
            previous,
            _guard: guard,
        }
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..) {
            unsafe {
                match value {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }
}

struct Remote<T> {
    target: T,
    capabilities: Vec<CapabilityId>,
}

impl<T: ErasedCallTarget + Send + Sync> ErasedCallTarget for Remote<T> {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        self.target.call(capability, context, input)
    }
}

impl<T: ErasedCallTarget + Send + Sync> RemoteImportTarget for Remote<T> {
    fn supports_capability(&self, capability: &CapabilityId) -> bool {
        self.capabilities.contains(capability)
    }
}

struct Exposed(Vec<TransportExposure>);

impl ErasedCallTarget for Exposed {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityId,
        context: CallContext,
        input: SlotValue,
    ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        self.0
            .iter()
            .find(|exposure| exposure.descriptor().id() == capability)
            .expect("check capability is exposed")
            .dispatch(context, input)
    }
}

fn assembled(fake: ClassifierFake) -> (Composition, CheckHandle) {
    let transport = Arc::new(StubTransport::new());
    let descriptor = generated::implementation_descriptor();
    let capability = descriptor.contract().capabilities()[0].id().clone();
    let classifier_capabilities = boxology_import_classifier::contract_descriptor()
        .capabilities()
        .iter()
        .map(|item| item.id().clone())
        .collect();
    let mut builder = CompositionBuilder::new();
    builder.add_box(descriptor, |imports| {
        let dependencies = generated::typed_imports(&imports);
        generated::factory(CheckService::new(dependencies.classifier), imports)
    });
    builder.resolve_import(
        BoxId::new("check").unwrap(),
        BoxId::new("classifier").unwrap(),
        ImportTarget::remote(Arc::new(Remote {
            target: fake,
            capabilities: classifier_capabilities,
        })),
    );
    builder.expose(
        BoxId::new("check").unwrap(),
        capability,
        transport.clone(),
        ExposureLevel::CodeOnly,
    );
    let composition = builder.start().unwrap();
    let handle = CheckHandle::from_erased(Arc::new(Exposed(
        transport.runtime().unwrap().exposures().to_vec(),
    )));
    (composition, handle)
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

fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("check unexpectedly pending"),
    }
}

fn call(
    handle: &CheckHandle,
    fixture: &Fixture,
    base: Option<&str>,
) -> Result<boxology_generated_contract::CheckOutcome, CallError<CheckError>> {
    ready(handle.check(
        context(),
        CheckRequest {
            workspace: fixture.root.to_string_lossy().into_owned(),
            base: base.map(str::to_owned),
        },
    ))
}

#[test]
fn clean_no_repository_crosses_real_handle_with_canonical_bytes() {
    let fixture = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let fake = ClassifierFake::new().with_classify({
        let calls = calls.clone();
        move |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { unreachable!() }
        }
    });
    let (_composition, handle) = assembled(fake);
    let _environment = fixture.environment(false);
    let outcome = call(&handle, &fixture, None).unwrap();
    let report = outcome.report.unwrap();
    assert!(outcome.failure.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        report.status,
        CheckStatus::Passed,
        "{}",
        String::from_utf8_lossy(&report.human)
    );
    assert_eq!(report.steps.len(), 9);
    assert_eq!(report.human, CLEAN_HUMAN);
    assert_eq!(report.json, CLEAN_JSON);
}

#[test]
fn typed_finding_preserves_exact_request_and_is_report_only() {
    let fixture = Fixture::new();
    fixture.install_git();
    let base = fs::read(&fixture.base).unwrap();
    let submitted = fs::read(fixture.root.join("ping/generated/schema.json")).unwrap();
    let fake = ClassifierFake::new().with_classify(move |_, request| {
        assert_eq!(request.base.as_deref(), Some(base.as_slice()));
        assert_eq!(request.submitted, submitted);
        async {
            Ok(ClassifyOutcome {
                report: Some(ClassifyReport {
                    verdict: CompatibilityClass::Incompatible,
                    findings: vec![ClassifyFinding {
                        code: "BXC9000".into(),
                        path: "ping.value".into(),
                        kind: "changed".into(),
                        class: CompatibilityClass::Incompatible,
                        base_excerpt: Some("old".into()),
                        submitted_excerpt: Some("new".into()),
                        condition: Some("migrate".into()),
                    }],
                    rendered_text: "ignored rendering".into(),
                }),
                failure: None,
            })
        }
    });
    let (_composition, handle) = assembled(fake);
    let _environment = fixture.environment(false);
    let outcome = call(&handle, &fixture, Some("main")).unwrap();
    let report = outcome.report.unwrap();
    assert!(outcome.failure.is_none());
    assert_eq!(
        report.status,
        CheckStatus::Passed,
        "{}",
        String::from_utf8_lossy(&report.human)
    );
    let step = report
        .steps
        .iter()
        .find(|step| step.id == "contract-classification")
        .unwrap();
    assert_eq!(step.status, CheckStepStatus::Failed);
    assert_eq!(
        step.findings,
        vec![CheckFinding {
            kind: "classifier".into(),
            code: "BXC9000".into(),
            path: "ping.value".into(),
            package: Some("ping".into()),
            payload: None,
            rule: None,
            rule_source: None,
            span_start_line: None,
            span_start_column: None,
            span_end_line: None,
            span_end_column: None,
            offending: None,
            class: Some("incompatible".into()),
            condition: Some("migrate".into()),
        }]
    );
}

#[test]
fn typed_classifier_failure_stages_preserve_exact_validation_bytes() {
    for (stage, expected) in [
        (
            ClassifyFailureStage::Base,
            "BXW0080 ping base: the base-revision schema document must satisfy the strict format-1 reader: coded diagnostics\n",
        ),
        (
            ClassifyFailureStage::Submitted,
            "BXW0081 ping submitted: the checked-in schema document must satisfy the strict format-1 reader: coded diagnostics\n",
        ),
        (
            ClassifyFailureStage::Pairing,
            "BXW0082 ping pairing: the base-revision and checked-in schema documents must pair and satisfy classifier integrity: coded diagnostics\n",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.install_git();
        let fake = ClassifierFake::new().with_classify(move |_, _| {
            let stage = stage.clone();
            async move {
                Ok(ClassifyOutcome {
                    report: None,
                    failure: Some(ClassifyFailure {
                        stage,
                        diagnostics: "coded diagnostics".into(),
                    }),
                })
            }
        });
        let (_composition, handle) = assembled(fake);
        let _environment = fixture.environment(false);
        let outcome = call(&handle, &fixture, Some("main")).unwrap();
        let failure = outcome.failure.unwrap();
        assert!(outcome.report.is_none());
        assert_eq!(failure.kind, CheckFailureKind::Validation);
        assert_eq!(failure.human, expected.as_bytes());
        assert_eq!(failure.json, expected.as_bytes());
    }
}

#[test]
fn malformed_classifier_outcome_becomes_domain_internal() {
    let fixture = Fixture::new();
    fixture.install_git();
    let fake = ClassifierFake::new().with_classify(|_, _| async {
        Ok(ClassifyOutcome {
            report: None,
            failure: None,
        })
    });
    let (_composition, handle) = assembled(fake);
    let _environment = fixture.environment(false);
    assert!(matches!(
        call(&handle, &fixture, Some("main")),
        Err(CallError::Domain(CheckError::Internal))
    ));
}

#[test]
fn metadata_invocation_failure_is_typed_without_classifier_call() {
    let fixture = Fixture::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let fake = ClassifierFake::new().with_classify({
        let calls = calls.clone();
        move |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { unreachable!() }
        }
    });
    let (_composition, handle) = assembled(fake);
    let _environment = fixture.environment(true);
    let outcome = call(&handle, &fixture, None).unwrap();
    let failure = outcome.failure.unwrap();
    assert!(outcome.report.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(failure.kind, CheckFailureKind::Invocation);
    assert_eq!(failure.human, b"BXW0075 Cargo.toml: cargo metadata could not be executed or did not return valid workspace metadata\nsynthetic metadata stderr\n");
    assert_eq!(failure.json, failure.human);
}
