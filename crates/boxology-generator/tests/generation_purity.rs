use boxology_contract::BoxId;
use boxology_generator::{GeneratedTree, generate};
use boxology_generator_model::GenerationRequest;
use std::{fs, path::Path};

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
const SORTED_OUTPUTS: [&str; 4] = [
    "generated/adapter/adapter.rs",
    "generated/contract/Cargo.toml",
    "generated/contract/src/lib.rs",
    "generated/schema.json",
];

const BODY_SENTINEL: &str = "BOXOLOGY_BODY_SENTINEL";
const INITIALIZER_SENTINEL: &str = "BOXOLOGY_INITIALIZER_SENTINEL";
const BUILD_SENTINEL: &str = "BOXOLOGY_BUILD_SENTINEL";
const PROC_MACRO_SENTINEL: &str = "BOXOLOGY_PROC_MACRO_SENTINEL";

fn request(inputs: Vec<(String, Vec<u8>)>) -> GenerationRequest {
    GenerationRequest::new(
        BoxId::new("hello").expect("fixed id is valid"),
        "src/lib.rs".into(),
        inputs,
        vec![],
        OUTPUTS.iter().map(|path| (*path).into()).collect(),
    )
    .expect("fixed purity request is valid")
}

fn baseline_request() -> GenerationRequest {
    request(vec![
        (
            "boxology.toml".into(),
            b"schema = 1\nid = \"hello\"\nkind = \"box\"\n".to_vec(),
        ),
        (
            "src/lib.rs".into(),
            format!(
                "{CONTRACT}\npub struct HelloService;\nimpl HelloService {{ fn helper(&self) {{}} }}\n"
            )
            .into_bytes(),
        ),
    ])
}

fn marker_literal(marker: &Path) -> String {
    format!("{:?}", marker.to_string_lossy())
}

fn hostile_request(marker: &Path) -> GenerationRequest {
    let marker = marker_literal(marker);
    let implementation = format!(
        r#"{CONTRACT}
pub struct HelloService;

static BOXOLOGY_INITIALIZER: &str = include_str!("{INITIALIZER_SENTINEL}");

impl HelloService {{
    #[sentinel_proc_macro::attribute]
    async fn greet(&self, name: String) -> Result<String, GreetError> {{
        let _ = std::fs::write({marker}, b"{BODY_SENTINEL}");
        panic!("{BODY_SENTINEL}");
    }}
}}
"#
    );
    request(vec![
        (
            "boxology.toml".into(),
            b"schema = 1\nid = \"hello\"\nkind = \"box\"\nbuild = \"build.rs\"\n".to_vec(),
        ),
        ("src/lib.rs".into(), implementation.into_bytes()),
        (
            "Cargo.toml".into(),
            b"[package]\nname = \"hostile\"\nversion = \"0.0.0\"\nbuild = \"build.rs\"\n".to_vec(),
        ),
        (
            "build.rs".into(),
            format!(
                r#"fn main() {{
    let _ = std::fs::write({marker:?}, b"{BUILD_SENTINEL}");
    panic!("{BUILD_SENTINEL}");
    compile_error!("{BUILD_SENTINEL}");
}}
"#,
                marker = marker.trim_matches('"'),
            )
            .into_bytes(),
        ),
        (
            "proc-macro.rs".into(),
            format!(
                r#"extern crate proc_macro;
#[proc_macro::proc_macro]
pub fn hostile(_: proc_macro::TokenStream) -> proc_macro::TokenStream {{
    let _ = std::fs::write({marker:?}, b"{PROC_MACRO_SENTINEL}");
    panic!("{PROC_MACRO_SENTINEL}");
    compile_error!("{PROC_MACRO_SENTINEL}");
}}
"#,
                marker = marker.trim_matches('"'),
            )
            .into_bytes(),
        ),
    ])
}

fn generated_bytes(tree: &GeneratedTree) -> Vec<u8> {
    assert_eq!(tree.files().len(), OUTPUTS.len());
    assert_eq!(
        tree.files()
            .iter()
            .map(|file| file.path())
            .collect::<Vec<_>>(),
        SORTED_OUTPUTS,
    );
    tree.files()
        .iter()
        .flat_map(|file| file.bytes().iter().copied())
        .collect()
}

fn assert_output_is_clean(tree: &GeneratedTree) {
    let bytes = generated_bytes(tree);
    let text = String::from_utf8(bytes).expect("generated outputs are UTF-8");
    for sentinel in [
        BODY_SENTINEL,
        INITIALIZER_SENTINEL,
        BUILD_SENTINEL,
        PROC_MACRO_SENTINEL,
    ] {
        assert!(
            !text.contains(sentinel),
            "generated output leaked hostile sentinel {sentinel}"
        );
    }
}

#[test]
fn generation_does_not_execute_hostile_implementation_inputs() {
    let marker = std::env::temp_dir().join(format!(
        "boxology-generation-purity-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let _ = fs::remove_file(&marker);
    assert!(
        !marker.exists(),
        "stale purity marker was present before test"
    );

    let baseline = generate(&baseline_request()).expect("inert baseline generation succeeds");
    let hostile = generate(&hostile_request(&marker)).expect(
        "generation must not compile or execute implementation, initializer, build, or proc-macro inputs",
    );

    assert_eq!(hostile, baseline);
    assert_output_is_clean(&hostile);
    assert!(!marker.exists(), "hostile generation executed a sentinel");
    let _ = fs::remove_file(&marker);
}
