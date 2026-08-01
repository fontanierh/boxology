use boxology_cli::{classify, render};
use boxology_contract::BoxId;
use boxology_generator::{OUTPUTS, generate};
use boxology_generator_model::GenerationRequest;
use boxology_schema::{SchemaDocument, SchemaPayload, SchemaVariant};

const V1: &str = "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
const V2: &str = "boxology::contract! { #[error] pub enum GreetError { EmptyName, Other } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
const OTHER: &str = "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn wave(name:String)->Result<String,GreetError>; }";

fn schema_bytes(box_id: &str, source: &str) -> Vec<u8> {
    let request = GenerationRequest::new(
        BoxId::new(box_id).unwrap(),
        "src/lib.rs".to_owned(),
        vec![
            (
                "boxology.toml".to_owned(),
                format!("schema = 1\nid = \"{box_id}\"\nkind = \"box\"\n").into_bytes(),
            ),
            ("src/lib.rs".to_owned(), source.as_bytes().to_vec()),
        ],
        Vec::new(),
        OUTPUTS.iter().map(|path| (*path).to_owned()).collect(),
    )
    .unwrap();
    generate(&request)
        .unwrap()
        .files()
        .iter()
        .find(|file| file.path() == "generated/schema.json")
        .expect("schema.json is a fixed generator output")
        .bytes()
        .to_vec()
}

fn parse(bytes: &[u8]) -> SchemaDocument {
    SchemaDocument::parse(bytes).unwrap()
}

fn with_other_revision(mut document: SchemaDocument) -> SchemaDocument {
    let other = parse(&schema_bytes("hello", OTHER));
    assert_ne!(document.revision, other.revision);
    document.revision = other.revision;
    document
}

fn variant(name: &str) -> SchemaVariant {
    SchemaVariant {
        name: name.to_owned(),
        docs: Vec::new(),
        deprecation: None,
        payload: SchemaPayload::Unit,
    }
}

#[test]
fn unparseable_base_is_bxw0077() {
    let submitted = schema_bytes("hello", V1);
    let expected = SchemaDocument::parse(b"{").unwrap_err();
    let error = classify(Some(b"{"), &submitted).unwrap_err();
    assert_eq!(error.code(), "BXW0077");
    assert_eq!(error.side(), "base");
    assert_eq!(
        error.detail(),
        "the checked-in schema document must satisfy the strict format-1 reader"
    );
    assert_eq!(error.diagnostics().to_string(), expected.to_string());
    assert_eq!(
        error.to_string(),
        format!("BXW0077 base: {}: {}", error.detail(), expected)
    );
}

#[test]
fn unparseable_submitted_is_bxw0078() {
    let base = schema_bytes("hello", V1);
    let expected = SchemaDocument::parse(b"{").unwrap_err();
    let error = classify(Some(&base), b"{").unwrap_err();
    assert_eq!(error.code(), "BXW0078");
    assert_eq!(error.side(), "submitted");
    assert_eq!(
        error.detail(),
        "the regenerated schema document must satisfy the strict format-1 reader"
    );
    assert_eq!(error.diagnostics().to_string(), expected.to_string());
    assert_eq!(
        error.to_string(),
        format!("BXW0078 submitted: {}: {}", error.detail(), expected)
    );
}

#[test]
fn mismatched_box_ids_are_bxw0079() {
    let base = schema_bytes("ping", V1);
    let submitted = schema_bytes("pong", V1);
    let base_doc = parse(&base);
    let submitted_doc = parse(&submitted);
    let expected =
        boxology_classifier::classify(Some(&base_doc), Some(&submitted_doc)).unwrap_err();
    let error = classify(Some(&base), &submitted).unwrap_err();
    assert_eq!(error.code(), "BXW0079");
    assert_eq!(error.side(), "pairing");
    assert_eq!(
        error.detail(),
        "the checked-in and regenerated schema documents must pair and satisfy classifier integrity"
    );
    assert_eq!(error.diagnostics().to_string(), expected.to_string());
    assert!(error.diagnostics().to_string().contains("BXC0025"));
}

#[test]
fn revision_integrity_errors_are_bxw0079() {
    let base = parse(&schema_bytes("hello", V1));
    let silence = with_other_revision(base.clone());
    let error = classify(Some(&base.canonical_bytes()), &silence.canonical_bytes()).unwrap_err();
    assert_eq!(error.code(), "BXW0079");
    assert!(error.diagnostics().to_string().contains("BXC0038"));

    let mut equal = base.clone();
    equal.types[0].variants.push(variant("Other"));
    let error = classify(Some(&base.canonical_bytes()), &equal.canonical_bytes()).unwrap_err();
    assert_eq!(error.code(), "BXW0079");
    assert!(error.diagnostics().to_string().contains("BXC0037"));
}

#[test]
fn variant_addition_is_conditional_and_asymmetric() {
    let base = schema_bytes("hello", V1);
    let submitted = schema_bytes("hello", V2);
    assert_ne!(parse(&base).revision, parse(&submitted).revision);

    let forward = classify(Some(&base), &submitted).unwrap();
    assert_eq!(
        forward.verdict().canonical_name(),
        "compatible_with_conditions"
    );
    assert_eq!(forward.findings().len(), 1);
    let finding = &forward.findings()[0];
    assert_eq!(finding.code(), "BXC0036");
    assert_eq!(finding.path(), "hello/type/GreetError/variant/Other");
    assert_eq!(
        finding.class().canonical_name(),
        "compatible_with_conditions"
    );
    assert_eq!(finding.condition(), Some("unknown-variant tolerance"));

    let reverse = classify(Some(&submitted), &base).unwrap();
    assert_eq!(reverse.verdict().canonical_name(), "incompatible");
    assert_eq!(reverse.findings().len(), 1);
    let finding = &reverse.findings()[0];
    assert_eq!(finding.code(), "BXC0035");
    assert_eq!(finding.path(), "hello/type/GreetError/variant/Other");
    assert_eq!(finding.class().canonical_name(), "incompatible");
    assert_eq!(finding.condition(), None);
}

#[test]
fn render_contains_every_finding_unmodified() {
    let mut base = parse(&schema_bytes("hello", V1));
    base.types[0].variants.push(variant("Other"));
    let mut submitted = parse(&schema_bytes("hello", V1));
    submitted.types[0].docs.push("Extra type docs.".to_owned());
    let submitted = with_other_revision(submitted);
    let report = classify(Some(&base.canonical_bytes()), &submitted.canonical_bytes()).unwrap();
    assert_eq!(
        render(&report),
        "classification incompatible\n\
         finding BXC0033 hello/type/GreetError documentation\n\
         finding BXC0035 hello/type/GreetError/variant/Other incompatible\n"
    );
}

#[test]
fn render_contains_conditional_finding_unmodified() {
    let base = schema_bytes("hello", V1);
    let submitted = schema_bytes("hello", V2);
    let report = classify(Some(&base), &submitted).unwrap();
    assert_eq!(
        render(&report),
        "classification compatible_with_conditions\n\
         finding BXC0036 hello/type/GreetError/variant/Other compatible_with_conditions condition=\"unknown-variant tolerance\"\n"
    );
}

#[test]
fn absent_base_is_contract_introduced() {
    let submitted = schema_bytes("hello", V1);
    let report = classify(None, &submitted).unwrap();
    assert_eq!(report.verdict().canonical_name(), "additive");
    assert_eq!(report.findings().len(), 1);
    assert_eq!(report.findings()[0].code(), "BXC0026");
    assert_eq!(report.findings()[0].path(), "hello");
}
