use boxology_cli::{
    CheckClassificationError, ClassifyStepError, DuplicatePackages, PackageSchemas, classify_step,
};
use boxology_contract::BoxId;
use boxology_generator::{OUTPUTS, generate};
use boxology_generator_model::GenerationRequest;
use boxology_schema::{SchemaDocument, SchemaPayload, SchemaVariant};
use boxology_workspace::{
    CheckReport, CheckStatus, Completion, ContractClassificationCompletion,
    DiffOwnershipCompletion, ExternalOutput,
};

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
    generate(request)
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

fn pkg(name: &str, base: Option<Vec<u8>>, submitted: Vec<u8>) -> PackageSchemas {
    PackageSchemas::new(BoxId::new(name).unwrap(), base, submitted)
}

fn coded(
    result: Result<ContractClassificationCompletion, ClassifyStepError>,
) -> CheckClassificationError {
    match result {
        Err(ClassifyStepError::Classification(error)) => error,
        other => panic!("expected classification error, got {other:?}"),
    }
}

fn report(contract_classification: ContractClassificationCompletion) -> CheckReport {
    CheckReport {
        discovery: Completion::Passed,
        regeneration: Completion::Passed,
        contract_classification,
        diff_ownership: DiffOwnershipCompletion::Passed,
        cargo_graph: Completion::Passed,
        fmt: Completion::Passed,
        clippy: Completion::Passed,
        tests: Completion::Passed,
        quality: Completion::Passed,
        external_output: ExternalOutput::empty(),
    }
}

#[test]
fn equal_revision_tamper_is_bxw0082_with_bxc0037() {
    let base = parse(&schema_bytes("hello", V1));
    let mut equal = base.clone();
    equal.types[0].variants.push(variant("Other"));
    let error = coded(classify_step(&[pkg(
        "hello",
        Some(base.canonical_bytes()),
        equal.canonical_bytes(),
    )]));
    assert_eq!(error.code(), "BXW0082");
    assert_eq!(error.side(), "pairing");
    assert_eq!(
        error.detail(),
        "the base-revision and checked-in schema documents must pair and satisfy classifier integrity"
    );
    assert!(error.diagnostics().to_string().contains("BXC0037"));
}

#[test]
fn revision_only_difference_is_bxw0082_with_bxc0038() {
    let base = parse(&schema_bytes("hello", V1));
    let silence = with_other_revision(base.clone());
    let error = coded(classify_step(&[pkg(
        "hello",
        Some(base.canonical_bytes()),
        silence.canonical_bytes(),
    )]));
    assert_eq!(error.code(), "BXW0082");
    assert!(error.diagnostics().to_string().contains("BXC0038"));
}

#[test]
fn variant_addition_preserves_condition_in_payload_and_render() {
    let completion = classify_step(&[pkg(
        "hello",
        Some(schema_bytes("hello", V1)),
        schema_bytes("hello", V2),
    )])
    .unwrap();
    let ContractClassificationCompletion::Failed(findings) = &completion else {
        panic!("expected Failed");
    };
    assert_eq!(
        findings.as_slice()[0].condition(),
        Some("unknown-variant tolerance")
    );
    assert!(
        findings
            .to_string()
            .contains("condition=\"unknown-variant tolerance\"")
    );
    assert!(
        report(completion)
            .render_human()
            .contains("condition=\"unknown-variant tolerance\"")
    );
}

#[test]
fn findings_sort_by_package_path_then_code_from_reverse_input() {
    let mut alpha_base = parse(&schema_bytes("alpha", V1));
    alpha_base.types[0].variants.push(variant("Other"));
    let mut alpha_submitted = parse(&schema_bytes("alpha", V1));
    alpha_submitted.types[0]
        .docs
        .push("Extra type docs.".to_owned());
    let alpha_submitted = with_other_revision(alpha_submitted);
    let ContractClassificationCompletion::Failed(findings) = classify_step(&[
        pkg(
            "zeta",
            Some(schema_bytes("zeta", V2)),
            schema_bytes("zeta", V1),
        ),
        pkg(
            "alpha",
            Some(alpha_base.canonical_bytes()),
            alpha_submitted.canonical_bytes(),
        ),
    ])
    .unwrap() else {
        panic!("expected Failed");
    };
    let keys: Vec<_> = findings
        .as_slice()
        .iter()
        .map(|f| (f.package().as_str(), f.path(), f.code()))
        .collect();
    assert_eq!(
        keys,
        [
            ("alpha", "alpha/type/GreetError", "BXC0033"),
            ("alpha", "alpha/type/GreetError/variant/Other", "BXC0035"),
            ("zeta", "zeta/type/GreetError/variant/Other", "BXC0035"),
        ]
    );
}

#[test]
fn reverse_incompatible_is_failed_payload() {
    let ContractClassificationCompletion::Failed(findings) = classify_step(&[pkg(
        "hello",
        Some(schema_bytes("hello", V2)),
        schema_bytes("hello", V1),
    )])
    .unwrap() else {
        panic!("expected Failed");
    };
    assert_eq!(findings.as_slice()[0].code(), "BXC0035");
}

#[test]
fn equal_modulo_provenance_is_passed_with_no_finding_lines() {
    let bytes = schema_bytes("hello", V1);
    let completion = classify_step(&[pkg("hello", Some(bytes.clone()), bytes)]).unwrap();
    assert_eq!(completion, ContractClassificationCompletion::Passed);
    assert_eq!(
        report(completion)
            .render_human()
            .lines()
            .filter(|line| line.starts_with("  BXC"))
            .count(),
        0
    );
}

#[test]
fn classification_findings_do_not_affect_exit_code() {
    let completion = classify_step(&[pkg(
        "hello",
        Some(schema_bytes("hello", V1)),
        schema_bytes("hello", V2),
    )])
    .unwrap();
    let report = report(completion);
    assert_eq!(report.status(), CheckStatus::Passed);
    assert_eq!(report.exit_code(), 0);
}

#[test]
fn two_findings_on_one_package_are_both_kept() {
    let mut base = parse(&schema_bytes("hello", V1));
    base.types[0].variants.push(variant("Other"));
    let mut submitted = parse(&schema_bytes("hello", V1));
    submitted.types[0].docs.push("Extra type docs.".to_owned());
    let submitted = with_other_revision(submitted);
    let ContractClassificationCompletion::Failed(findings) = classify_step(&[pkg(
        "hello",
        Some(base.canonical_bytes()),
        submitted.canonical_bytes(),
    )])
    .unwrap() else {
        panic!("expected Failed");
    };
    assert_eq!(
        findings
            .as_slice()
            .iter()
            .map(|f| f.code())
            .collect::<Vec<_>>(),
        ["BXC0033", "BXC0035"]
    );
    assert!(findings.to_string().contains("BXC0033"));
    assert!(findings.to_string().contains("BXC0035"));
}

#[test]
fn parse_and_duplicate_errors_are_immediate() {
    assert_eq!(
        coded(classify_step(&[pkg(
            "hello",
            Some(b"{".to_vec()),
            schema_bytes("hello", V1),
        )]))
        .code(),
        "BXW0080"
    );
    assert_eq!(
        coded(classify_step(&[pkg(
            "hello",
            Some(schema_bytes("hello", V1)),
            b"{".to_vec(),
        )]))
        .code(),
        "BXW0081"
    );
    let bytes = schema_bytes("hello", V1);
    assert!(matches!(
        classify_step(&[
            pkg("hello", Some(bytes.clone()), bytes.clone()),
            pkg("hello", Some(bytes.clone()), bytes),
        ])
        .unwrap_err(),
        ClassifyStepError::Duplicate(DuplicatePackages)
    ));
}
