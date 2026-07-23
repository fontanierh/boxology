use boxology_contract::BoxId;
use boxology_generator::generate;
use boxology_generator_model::GenerationRequest;
use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

const CONTRACT: &str = r#"boxology::contract! {
    #[error] pub enum GreetError { EmptyName }
    #[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, GreetError>;
}
"#;
const OUTPUTS: [&str; 4] = [
    "generated/contract/Cargo.toml",
    "generated/contract/src/lib.rs",
    "generated/adapter/adapter.rs",
    "generated/schema.json",
];
static NEXT: AtomicUsize = AtomicUsize::new(0);

fn source(implementation: &str) -> String {
    format!("{CONTRACT}\n{implementation}\n")
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
    let case = root.join(name);
    let program = source(implementation);
    let generated = generate(&request(&program)).unwrap();
    for file in generated.files() {
        let path = case.join(file.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, file.bytes()).unwrap();
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
        .args(["check", "--offline", "--manifest-path"])
        .arg(case.join("consumer/Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap()
}

#[test]
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
fn structural_and_rust_type_failures_are_independent() {
    let root = std::env::temp_dir().join(format!(
        "boxology-impl-negative-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let cases = [
        (
            "wrong-context",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: String, name: String) -> Result<String, GreetError> { let _=context; Ok(name) } }",
            "CallContext",
        ),
        (
            "wrong-input",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: u32) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
            "u32",
        ),
        (
            "wrong-output",
            "pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<u32, GreetError> { let _=(context,name); todo!() } }",
            "u32",
        ),
        (
            "wrong-error",
            "pub enum LocalError { Bad } pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, LocalError> { let _=(context,name); todo!() } }",
            "LocalError",
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
            "non-send-service",
            "pub struct HelloService(std::rc::Rc<()>); #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let _=(context,name); todo!() } }",
            "Send",
        ),
        (
            "non-send-future",
            "trait Send {} impl<T> Send for T {} trait Sync {} impl<T> Sync for T {} pub struct HelloService; #[boxology::implementation] impl HelloService { async fn greet(&self, context: boxology::CallContext, name: String) -> Result<String, GreetError> { let held=std::rc::Rc::new(()); async {}.await; drop(held); let _=context; Ok(name) } }",
            "future cannot be sent",
        ),
    ];
    for (name, implementation, class) in cases {
        let output = check(&root, name, implementation);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{name} unexpectedly compiled");
        assert!(stderr.contains(class), "{name} lacked {class:?}: {stderr}");
    }
    fs::remove_dir_all(root).unwrap();
}
