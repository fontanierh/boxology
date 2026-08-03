use boxology_contract::BoxId;
use boxology_generator::generate;
use boxology_generator_model::GenerationRequest;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

const CONTRACT: &str = r#"boxology::contract! {
    #[error] pub enum GreetError { EmptyName }
    #[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, GreetError>;
}
"#;
const VALID_IMPLEMENTATION: &str = "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _ = context; Ok(name) } }";
const OUTPUTS: [&str; 4] = [
    "generated/contract/Cargo.toml",
    "generated/contract/src/lib.rs",
    "generated/adapter/adapter.rs",
    "generated/schema.json",
];
static NEXT: AtomicUsize = AtomicUsize::new(0);
const CHECKER_MACRO: &str = "__boxology_check_implementation!";
const IMPLEMENTATION_ATTRIBUTE: &[u8] = b"#[boxology::implementation]";
const CALL_ASSERTION: &str = "receiver.greet(context, input)";
const SERVICE_ASSERTION: &str = "::core::marker::Send +\n        ::core::marker::Sync + 'static";
const SERVICE_CALL: &str = "require_service::<$receiver > ();";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpanLocation {
    file_name: String,
    byte_start: u64,
    byte_end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpansionFrame {
    macro_decl_name: Option<String>,
    call_site: Option<SpanLocation>,
    definition_site: Option<SpanLocation>,
}

#[derive(Debug)]
struct PrimarySpan {
    label: String,
    assertion: Option<&'static str>,
    through_checker: bool,
}

#[derive(Debug)]
struct CompilerError {
    code: Option<String>,
    primary_spans: Vec<PrimarySpan>,
    child_messages: Vec<String>,
    service_assertion: bool,
    rendered: String,
    has_checker_ancestry: bool,
}

impl CompilerError {
    fn has_code(&self, code: &str) -> bool {
        self.code.as_deref() == Some(code)
    }

    fn primary_label_has_types(&self, expected: &str, found: &str) -> bool {
        self.primary_spans
            .iter()
            .any(|span| label_has_types(&span.label, expected, found))
    }

    fn generated_types(&self, expected: &str, found: &str) -> bool {
        self.primary_spans.iter().any(|span| {
            span.through_checker
                && span.assertion == Some(CALL_ASSERTION)
                && label_has_types(&span.label, expected, found)
        })
    }

    fn child_names(&self, function: &str) -> bool {
        self.child_messages
            .iter()
            .any(|message| message.contains(function))
    }
}

fn label_has_types(label: &str, expected: &str, found: &str) -> bool {
    let label = compact(label);
    label.contains("expected")
        && label.contains(&compact_type(expected))
        && label.contains("found")
        && label.contains(&compact_type(found))
}

fn compact(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn compact_type(value: &str) -> String {
    compact(value).replace('`', "")
}

fn span_location(value: Option<&Value>) -> Option<SpanLocation> {
    let span = value?.as_object()?;
    Some(SpanLocation {
        file_name: span.get("file_name")?.as_str()?.to_owned(),
        byte_start: span.get("byte_start")?.as_u64()?,
        byte_end: span.get("byte_end")?.as_u64()?,
    })
}

fn expansion_ancestry(span: &Value) -> Vec<ExpansionFrame> {
    let mut frames = Vec::new();
    let mut expansion = span.get("expansion");
    while let Some(expansion_object) = expansion.and_then(Value::as_object) {
        frames.push(ExpansionFrame {
            macro_decl_name: expansion_object
                .get("macro_decl_name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            call_site: span_location(expansion_object.get("span")),
            definition_site: span_location(expansion_object.get("def_site_span")),
        });
        expansion = expansion_object
            .get("span")
            .and_then(|call_site| call_site.get("expansion"));
    }
    frames
}

fn same_path(actual: &str, expected: &Path, root: &Path) -> bool {
    let actual = Path::new(actual);
    if actual.is_absolute() {
        return actual.canonicalize().ok() == expected.canonicalize().ok();
    }
    expected
        .strip_prefix(root)
        .is_ok_and(|expected| actual == expected)
}

fn same_file(location: &SpanLocation, expected: &Path, root: &Path) -> bool {
    same_path(&location.file_name, expected, root)
}

fn is_implementation_site(location: &SpanLocation, source: &Path, case: &Path) -> bool {
    same_file(location, source, case)
        && fs::read(source).ok().is_some_and(|bytes| {
            bytes.get(location.byte_start as usize..location.byte_end as usize)
                == Some(IMPLEMENTATION_ATTRIBUTE)
        })
}

fn assertion_at(
    location: &SpanLocation,
    generated_source: &Path,
    case: &Path,
) -> Option<&'static str> {
    if !same_file(location, generated_source, case) {
        return None;
    }
    let source = fs::read_to_string(generated_source).ok()?;
    [CALL_ASSERTION, SERVICE_ASSERTION]
        .into_iter()
        .find(|text| {
            let mut matches = source.match_indices(text);
            let Some((start, _)) = matches.next() else {
                return false;
            };
            matches.next().is_none()
                && start <= location.byte_start as usize
                && location.byte_end as usize <= start + text.len()
        })
}

fn generated_checker(
    ancestry: &[ExpansionFrame],
    generated_source: &Path,
    consumer_source: &Path,
    implementation_source: &Path,
    case: &Path,
    repository: &Path,
) -> bool {
    let mut checker_frames = ancestry
        .iter()
        .filter(|frame| is_checker_macro(frame.macro_decl_name.as_deref()));
    let Some(checker) = checker_frames.next() else {
        return false;
    };
    let Some(definition) = checker.definition_site.as_ref() else {
        return false;
    };
    let has_call = |frame: &ExpansionFrame| {
        frame
            .call_site
            .as_ref()
            .is_some_and(|site| !site.file_name.is_empty())
    };
    let checkers_match = has_call(checker)
        && same_file(definition, generated_source, case)
        && checker_frames
            .all(|frame| has_call(frame) && frame.definition_site.as_ref() == Some(definition));
    let mut implementations = ancestry.iter().filter(|frame| {
        matches!(
            frame.macro_decl_name.as_deref(),
            Some("#[boxology::implementation]") | Some("#[implementation]")
        )
    });
    let Some(implementation) = implementations.next() else {
        return false;
    };
    checkers_match
        && implementations.next().is_none()
        && implementation
            .definition_site
            .as_ref()
            .is_some_and(|site| same_file(site, implementation_source, repository))
        && implementation
            .call_site
            .as_ref()
            .is_some_and(|site| is_implementation_site(site, consumer_source, case))
}

fn is_checker_macro(name: Option<&str>) -> bool {
    name == Some(CHECKER_MACRO)
        || name.is_some_and(|name| name.ends_with("::__boxology_check_implementation!"))
}

fn consumer_library_target(target: &Value, package: &str, source: &Path, case: &Path) -> bool {
    let expected_name = package.replace('-', "_");
    let is_library = target
        .get("kind")
        .and_then(Value::as_array)
        .is_some_and(|kinds| kinds.len() == 1 && kinds[0].as_str() == Some("lib"));
    let has_expected_source = target
        .get("src_path")
        .and_then(Value::as_str)
        .is_some_and(|path| same_path(path, source, case));
    target.get("name").and_then(Value::as_str) == Some(&expected_name)
        && is_library
        && has_expected_source
}

fn compiler_errors(output: &Output, package: &str, case: &Path) -> Vec<CompilerError> {
    let consumer_source = case.join("consumer/src/lib.rs");
    let generated_source = case.join("generated/contract/src/lib.rs");
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let repository = crates.parent().unwrap();
    let implementation_source = crates.join("boxology-macros/src/lib.rs");
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .enumerate()
        .filter_map(|(line_number, line)| {
            let document: Value = serde_json::from_slice(line).unwrap_or_else(|error| {
                panic!("cargo emitted malformed JSON on line {line_number}: {error}")
            });
            if document.get("reason").and_then(Value::as_str) != Some("compiler-message") {
                return None;
            }
            let target = document
                .get("target")
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("compiler-message has no target name: {document}"));
            let target = Value::Object(target.clone());
            if !consumer_library_target(&target, package, &consumer_source, case) {
                return None;
            }
            let message = document
                .get("message")
                .and_then(Value::as_object)
                .unwrap_or_else(|| panic!("compiler-message has no diagnostic: {document}"));
            if message.get("level").and_then(Value::as_str) != Some("error") {
                return None;
            }
            let code = message.get("code").and_then(|code| match code {
                Value::Null => None,
                Value::Object(code) => code.get("code").and_then(Value::as_str).map(str::to_owned),
                value => panic!("compiler diagnostic has malformed code: {value}"),
            });
            let spans = message
                .get("spans")
                .and_then(Value::as_array)
                .unwrap_or_else(|| panic!("compiler diagnostic has no spans: {message:?}"));
            let primary_spans: Vec<&Value> = spans
                .iter()
                .filter(|span| span.get("is_primary").and_then(Value::as_bool) == Some(true))
                .collect();
            let children = message
                .get("children")
                .and_then(Value::as_array)
                .unwrap_or_else(|| panic!("compiler diagnostic has no children: {message:?}"));
            let child_messages = children
                .iter()
                .map(|child| {
                    child
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or_else(|| panic!("compiler child has no message: {child}"))
                        .to_owned()
                })
                .collect();
            let service_assertion = children
                .iter()
                .filter(|child| {
                    child["message"]
                        .as_str()
                        .is_some_and(|message| message.contains("require_service"))
                })
                .filter_map(|child| child.get("spans").and_then(Value::as_array))
                .flatten()
                .filter_map(|span| span_location(Some(span)))
                .any(|location| {
                    assertion_at(&location, &generated_source, case) == Some(SERVICE_ASSERTION)
                })
                && fs::read_to_string(&generated_source)
                    .is_ok_and(|source| source.match_indices(SERVICE_CALL).count() == 1);
            let has_checker_ancestry = primary_spans.iter().any(|span| {
                expansion_ancestry(span)
                    .iter()
                    .any(|frame| is_checker_macro(frame.macro_decl_name.as_deref()))
            });
            let primary_spans = primary_spans
                .into_iter()
                .filter_map(|span| {
                    let label = span.get("label").and_then(Value::as_str)?.to_owned();
                    let location = span_location(Some(span))?;
                    let ancestry = expansion_ancestry(span);
                    Some(PrimarySpan {
                        label,
                        assertion: assertion_at(&location, &generated_source, case),
                        through_checker: generated_checker(
                            &ancestry,
                            &generated_source,
                            &consumer_source,
                            &implementation_source,
                            case,
                            repository,
                        ),
                    })
                })
                .collect();
            let rendered = message
                .get("rendered")
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    panic!("compiler diagnostic has no rendered message: {message:?}")
                })
                .to_owned();
            Some(CompilerError {
                code,
                primary_spans,
                child_messages,
                service_assertion,
                rendered,
                has_checker_ancestry,
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Class {
    Input,
    Output,
    Error,
    Service,
}

fn class_matches(errors: &[CompilerError], class: Class, hardened: bool) -> bool {
    let typed = |code, expected, found, child: Option<&str>| {
        errors.iter().any(|error| {
            error.has_code(code)
                && if hardened {
                    error.generated_types(expected, found)
                } else {
                    error.primary_label_has_types(expected, found)
                }
                && child.is_none_or(|name| error.child_names(name))
        })
    };
    match class {
        Class::Input => typed("E0308", "`u32`", "`String`", None),
        Class::Output => typed(
            "E0271",
            "Result<String, GreetError>",
            "Result<u32, GreetError>",
            Some("require_future"),
        ),
        Class::Error => typed(
            "E0271",
            "Result<String, GreetError>",
            "Result<String, LocalError>",
            Some("require_future"),
        ),
        Class::Service => {
            let traits = ["Send", "Sync"].iter().all(|name| {
                errors.iter().any(|error| {
                    let labels = error.primary_spans.iter().any(|span| {
                        (!hardened || error.service_assertion)
                            && span.label.contains("Rc<()>")
                            && (span.label.contains(name)
                                || (*name == "Send" && span.label.contains("sent between"))
                                || (*name == "Sync" && span.label.contains("shared between")))
                    });
                    error.has_code("E0277") && labels && error.child_names("require_service")
                })
            });
            traits
                && (!hardened
                    || errors.iter().any(|error| {
                        error.primary_spans.iter().any(|span| {
                            span.through_checker && span.assertion == Some(CALL_ASSERTION)
                        })
                    }))
        }
    }
}

fn source(implementation: &str) -> String {
    format!("{CONTRACT}\n{implementation}\n")
}

fn valid_with(extra: &str) -> String {
    format!("{VALID_IMPLEMENTATION}\n{extra}")
}

fn request(source: &str) -> GenerationRequest {
    GenerationRequest::new(
        BoxId::new("hello").unwrap(),
        "src/lib.rs".into(),
        vec![
            (
                "boxology.toml".into(),
                b"schema = 1\nid = \"hello\"\nkind = \"box\"\n".to_vec(),
            ),
            ("src/lib.rs".into(), source.as_bytes().to_vec()),
        ],
        vec![],
        OUTPUTS.iter().map(|path| (*path).into()).collect(),
    )
    .unwrap()
}

fn check(root: &Path, name: &str, implementation: &str) -> std::process::Output {
    check_mutated(root, name, implementation, None)
}

fn check_mutated(
    root: &Path,
    name: &str,
    implementation: &str,
    mutation: Option<(&str, &str)>,
) -> Output {
    let case = root.join(name);
    let program = source(implementation);
    let generated = generate(&request(&program)).unwrap();
    for file in generated.files() {
        let path = case.join(file.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = mutation
            .filter(|_| file.path() == Path::new("generated/contract/src/lib.rs"))
            .map_or_else(
                || file.bytes().to_vec(),
                |(old, new)| {
                    let original = std::str::from_utf8(file.bytes()).unwrap();
                    let changed = original.replacen(old, new, 1);
                    assert_ne!(changed, original, "mutant did not alter the checker");
                    changed.into_bytes()
                },
            );
        fs::write(path, bytes).unwrap();
    }
    fs::create_dir_all(case.join("consumer/src")).unwrap();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::write(
        case.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers=[\"generated/contract\",\"consumer\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology-contract={{version=\"=0.0.0\",path={:?}}}\n",
            workspace.join("boxology-contract")
        ),
    ).unwrap();
    fs::write(
        case.join("consumer/Cargo.toml"),
        format!(
            "[package]\nname=\"case-{name}\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology={{path={:?}}}\nboxology_generated_contract={{package=\"hello-contract\",path=\"../generated/contract\"}}\n",
            workspace.join("boxology")
        ),
    ).unwrap();
    fs::write(case.join("consumer/src/lib.rs"), program).unwrap();
    Command::new("cargo")
        .args([
            "check",
            "--offline",
            "--message-format=json",
            "--manifest-path",
        ])
        .arg(case.join("consumer/Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .unwrap()
}

fn assert_unrelated_control(
    root: &Path,
    name: &str,
    implementation: &str,
    class: Class,
    has_checker_ancestry: bool,
    mutation: Option<(&str, &str)>,
) {
    let output = check_mutated(root, name, implementation, mutation);
    assert!(!output.status.success(), "{name} unexpectedly compiled");
    let errors = compiler_errors(&output, &format!("case-{name}"), &root.join(name));
    assert!(
        class_matches(&errors, class, false),
        "{name} did not satisfy the pre-repair predicate: {errors:?}"
    );
    assert!(
        errors.iter().any(|error| error.has_checker_ancestry) == has_checker_ancestry,
        "{name} checker ancestry evidence differed: {errors:?}"
    );
    assert!(
        !class_matches(&errors, class, true),
        "{name} was accepted despite lacking checker provenance: {errors:?}"
    );
}

#[test]
fn relative_diagnostic_paths_are_exact() {
    let case = Path::new("/case");
    let source = case.join("consumer/src/lib.rs");
    let accepted = |src_path| {
        let target =
            serde_json::json!({"name": "case_probe", "kind": ["lib"], "src_path": src_path});
        consumer_library_target(&target, "case-probe", &source, case)
    };
    assert!(accepted("consumer/src/lib.rs"));
    assert!(!accepted("src/lib.rs"));
    assert!(!accepted("shadow/consumer/src/lib.rs"));
}

#[test]
#[ignore = "deep nested-Cargo matrix runs in main-push --no-budget CI"]
fn aliases_qualified_paths_and_unrelated_helpers_compile() {
    let root = std::env::temp_dir().join(format!(
        "boxology-impl-positive-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let implementation = r#"
type Context = boxology::CallContext;
type Input = std::string::String;
type Output = std::string::String;
type Error = GreetError;
struct String;
struct Result<T, E>(core::marker::PhantomData<(T, E)>);
trait Send {} impl<T> Send for T {}
trait Sync {} impl<T> Sync for T {}
pub struct HelloService;
#[boxology::implementation]
impl crate::HelloService {
    async fn greet(&self, context: Context, name: Input) -> core::result::Result<Output, Error> {
        let _ = context;
        Ok(name)
    }
    fn helper<T>(&self, value: T) -> T { value }
}"#;
    let output = check(&root, "positive", implementation);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let alternate = implementation.replace(
        "let _ = context;\n        Ok(name)",
        "drop(context);\n        async {}.await;\n        Ok(name)",
    );
    assert_eq!(
        generate(&request(&source(implementation))).unwrap(),
        generate(&request(&source(&alternate))).unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "deep nested-Cargo matrix runs in main-push --no-budget CI"]
fn structural_and_rust_type_failures_are_independent() {
    let root = std::env::temp_dir().join(format!(
        "boxology-impl-negative-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let structural_cases = [
        (
            "wrong-context",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: String, name: String) -> Result<String, GreetError> { let _=context; Ok(name) } }",
            "CallContext",
        ),
        (
            "altered-receiver",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&mut self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
            "invalid structural signature",
        ),
        (
            "extra-parameter",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String, extra: u8) -> Result<String, GreetError> { let _=(context,name,extra); todo!() } }",
            "invalid structural signature",
        ),
        (
            "missing",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn renamed(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
            "implementation is missing",
        ),
        (
            "generic-impl",
            "pub struct HelloService<T>(T); #[boxology::implementation] impl<T> HelloService<T> { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
            "impl cannot be generic",
        ),
        (
            "generic-method",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet<T>(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
            "invalid structural signature",
        ),
        (
            "impl-where",
            "pub struct HelloService; #[boxology::implementation] impl HelloService where HelloService: Sized { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
            "impl cannot have a where clause",
        ),
        (
            "method-where",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> where Self: Sized { let _=(context,name); todo!() } }",
            "invalid structural signature",
        ),
        (
            "input-impl-trait",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: impl Into<String>) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
            "invalid structural signature",
        ),
        (
            "output-impl-trait",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> impl Sized { let _=(context,name); 1u8 } }",
            "invalid structural signature",
        ),
        (
            "non-send-future",
            "trait Send {} impl<T> Send for T {} trait Sync {} impl<T> Sync for T {} pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let held=std::rc::Rc::new(()); async {}.await; drop(held); let _=context; Ok(name) } }",
            "future cannot be sent",
        ),
    ];
    for (name, implementation, class) in structural_cases {
        let output = check(&root, name, implementation);
        assert!(!output.status.success(), "{name} unexpectedly compiled");
        let errors = compiler_errors(&output, &format!("case-{name}"), &root.join(name));
        assert!(
            errors.iter().any(|error| error.rendered.contains(class)),
            "{name} lacked rendered {class:?}: {errors:?}"
        );
    }

    let typed_cases = [
        (
            "wrong-input",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: u32) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
        ),
        (
            "wrong-output",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<u32, GreetError> { let _=(context,name); todo!() } }",
        ),
        (
            "wrong-error",
            "pub enum LocalError { Bad } pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, LocalError> { let _=(context,name); todo!() } }",
        ),
        (
            "non-send-service",
            "pub struct HelloService(std::rc::Rc<()>); #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
        ),
    ];
    for (name, implementation) in typed_cases {
        let output = check(&root, name, implementation);
        assert!(!output.status.success(), "{name} unexpectedly compiled");
        let errors = compiler_errors(&output, &format!("case-{name}"), &root.join(name));
        let class = match name {
            "wrong-input" => Class::Input,
            "wrong-output" => Class::Output,
            "wrong-error" => Class::Error,
            "non-send-service" => Class::Service,
            _ => unreachable!("unlisted typed case {name}"),
        };
        assert!(
            class_matches(&errors, class, true),
            "{name} lacked its hardened class: {errors:?}"
        );
    }

    assert_unrelated_control(
        &root,
        "unrelated-output",
        &valid_with(
            "fn require_future<F: core::future::Future<Output = Result<String, GreetError>>>(_: F) {} async fn unrelated_output() -> Result<u32, GreetError> { Err(GreetError::EmptyName) } fn unrelated_helper() { require_future(unrelated_output()); }",
        ),
        Class::Output,
        false,
        None,
    );

    assert_unrelated_control(
        &root,
        "unrelated-error",
        &valid_with(
            "pub enum LocalError { Bad } fn require_future<F: core::future::Future<Output = Result<String, GreetError>>>(_: F) {} async fn unrelated_error() -> Result<String, LocalError> { Err(LocalError::Bad) } fn unrelated_helper() { require_future(unrelated_error()); }",
        ),
        Class::Error,
        false,
        None,
    );

    assert_unrelated_control(
        &root,
        "unrelated-service",
        &valid_with(
            "struct UnrelatedService(std::rc::Rc<()>); fn require_service<T: core::marker::Send + core::marker::Sync + 'static>() {} fn unrelated_helper() { require_service::<UnrelatedService>(); }",
        ),
        Class::Service,
        false,
        None,
    );

    assert_unrelated_control(
        &root,
        "unrelated-input",
        &valid_with("fn unrelated_helper() { let _: u32 = String::new(); }"),
        Class::Input,
        false,
        None,
    );

    assert_unrelated_control(
        &root,
        "replaced-input-assertion",
        "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: u32) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
        Class::Input,
        true,
        Some((
            "require_future(receiver.greet(context, input));",
            "let _: u32 = ::std::string::String::new();",
        )),
    );

    assert_unrelated_control(
        &root,
        "direct-checker",
        &valid_with(
            "pub struct DirectService; impl DirectService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<u32, GreetError> { let _ = (context, name); todo!() } } boxology_generated_contract::__boxology_check_implementation!(DirectService; greet valid;);",
        ),
        Class::Output,
        true,
        None,
    );

    assert_unrelated_control(
        &root,
        "replaced-service-call",
        "pub struct HelloService(std::rc::Rc<()>); #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
        Class::Service,
        true,
        Some((SERVICE_CALL, "require_service::<::std::rc::Rc<()> > ();")),
    );

    fs::remove_dir_all(root).unwrap();
}
