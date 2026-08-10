use boxology_schema::{BoundaryLeaf, SchemaDataShape, SchemaDocument, TypeExpression};
use std::fs;
use std::path::{Path, PathBuf};

const EXPECTED_OWNERS: &[&str] = &["greeter", "hello", "ping"];

struct FixtureCase {
    owner: &'static str,
    bytes: &'static [u8],
    box_id: &'static str,
    capability: &'static str,
    input_name: &'static str,
    input_leaf: BoundaryLeaf,
    output_leaf: BoundaryLeaf,
    error: &'static str,
    variant: &'static str,
    revision: &'static str,
}

const CASES: &[FixtureCase] = &[
    FixtureCase {
        owner: "greeter",
        bytes: include_bytes!("../../fixtures/greeter/generated/schema.json"),
        box_id: "greeter",
        capability: "greet_loudly",
        input_name: "name",
        input_leaf: BoundaryLeaf::String,
        output_leaf: BoundaryLeaf::String,
        error: "GreetLoudlyError",
        variant: "Refused",
        revision: "sha256:a45a70dacfc5e3ea7911944d3f4fd385da1de2cdabfac86d554d4a321e3244cc",
    },
    FixtureCase {
        owner: "hello",
        bytes: include_bytes!("../../fixtures/hello/generated/schema.json"),
        box_id: "hello",
        capability: "greet",
        input_name: "name",
        input_leaf: BoundaryLeaf::String,
        output_leaf: BoundaryLeaf::String,
        error: "GreetError",
        variant: "EmptyName",
        revision: "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176",
    },
    FixtureCase {
        owner: "ping",
        bytes: include_bytes!("../../fixtures/ping/generated/schema.json"),
        box_id: "ping",
        capability: "ping",
        input_name: "nonce",
        input_leaf: BoundaryLeaf::U64,
        output_leaf: BoundaryLeaf::U64,
        error: "HelloError",
        variant: "EmptyName",
        revision: "sha256:c89886aac818a0bb5e9f9b928e5590291c142c14430b658a4020480575d84970",
    },
];

struct DiscoveredFixture {
    owner: String,
    schema_path: PathBuf,
    bytes: Vec<u8>,
}

fn discover_fixtures() -> Vec<DiscoveredFixture> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures");
    let entries = fs::read_dir(&fixtures)
        .unwrap_or_else(|error| panic!("cannot enumerate {fixtures:?}: {error}"));
    let mut discovered = Vec::new();

    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("cannot read {fixtures:?}: {error}"));
        let owner_path = entry.path();
        if !owner_path.is_dir() {
            continue;
        }

        let generated = owner_path.join("generated");
        if !generated.exists() {
            continue;
        }
        assert!(
            generated.is_dir(),
            "fixture generated path is not a directory: {generated:?}"
        );

        let schema_path = generated.join("schema.json");
        if !schema_path.exists() {
            continue;
        }
        let metadata = fs::metadata(&schema_path)
            .unwrap_or_else(|error| panic!("fixture schema is missing {schema_path:?}: {error}"));
        assert!(
            metadata.is_file(),
            "fixture schema is not a file: {schema_path:?}"
        );
        let bytes = fs::read(&schema_path)
            .unwrap_or_else(|error| panic!("cannot read fixture schema {schema_path:?}: {error}"));
        assert!(
            !bytes.is_empty(),
            "fixture schema is empty: {schema_path:?}"
        );
        let owner = entry
            .file_name()
            .into_string()
            .unwrap_or_else(|name| panic!("fixture owner is not UTF-8: {name:?}"));
        discovered.push(DiscoveredFixture {
            owner,
            schema_path,
            bytes,
        });
    }

    discovered.sort_by(|left, right| left.owner.cmp(&right.owner));
    discovered
}

fn static_case(owner: &str) -> &'static FixtureCase {
    let matches: Vec<_> = CASES.iter().filter(|case| case.owner == owner).collect();
    assert_eq!(
        matches.len(),
        1,
        "fixture {owner} must match exactly one static case"
    );
    matches[0]
}

fn assert_revision_shape(revision: &str, owner: &str) {
    assert_eq!(revision.len(), 71, "{owner} revision must be 71 bytes");
    let hex = revision
        .strip_prefix("sha256:")
        .unwrap_or_else(|| panic!("{owner} revision must start with sha256:"));
    assert_eq!(hex.len(), 64, "{owner} revision digest must be 64 bytes");
    assert!(
        hex.bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
        "{owner} revision digest must be lowercase hexadecimal"
    );
}

fn assert_semantics(document: &SchemaDocument, case: &FixtureCase) {
    assert_eq!(
        document.box_id.as_str(),
        case.box_id,
        "{} box id",
        case.owner
    );
    assert_eq!(
        document.capabilities.len(),
        1,
        "{} capabilities",
        case.owner
    );
    let capability = &document.capabilities[0];
    assert_eq!(
        capability.name.as_str(),
        case.capability,
        "{} capability name",
        case.owner
    );
    assert_eq!(
        capability.input.name, case.input_name,
        "{} input name",
        case.owner
    );
    assert_eq!(
        capability.input.leaf, case.input_leaf,
        "{} input leaf",
        case.owner
    );
    assert_eq!(
        capability.output.leaf, case.output_leaf,
        "{} output leaf",
        case.owner
    );
    assert_eq!(
        capability.error, case.error,
        "{} capability error",
        case.owner
    );

    assert_eq!(document.types.len(), 1, "{} declared types", case.owner);
    let error_type = &document.types[0];
    assert_eq!(error_type.name, case.error, "{} error type", case.owner);
    assert_eq!(
        capability.error, error_type.name,
        "{} capability error reference",
        case.owner
    );
    assert_eq!(
        error_type.variants.len(),
        1,
        "{} error variants",
        case.owner
    );
    assert_eq!(
        error_type.variants[0].name, case.variant,
        "{} error variant",
        case.owner
    );

    assert_revision_shape(&document.revision, case.owner);
    assert_eq!(document.revision, case.revision, "{} revision", case.owner);
    assert_eq!(
        document.provenance.value(),
        &serde_json::Value::String("@PROVENANCE@".to_owned()),
        "{} provenance",
        case.owner
    );
}

#[test]
fn checked_in_s2_schemas_parse_and_round_trip() {
    let discovered = discover_fixtures();
    let discovered_owners: Vec<_> = discovered
        .iter()
        .map(|fixture| fixture.owner.as_str())
        .collect();
    assert_eq!(
        discovered_owners, EXPECTED_OWNERS,
        "discovered fixture owners must be exactly the checked-in S2 schemas"
    );

    let mut case_owners: Vec<_> = CASES.iter().map(|case| case.owner).collect();
    case_owners.sort_unstable();
    assert_eq!(
        case_owners, EXPECTED_OWNERS,
        "static fixture cases must cover each owner exactly once"
    );

    for fixture in discovered {
        let case = static_case(&fixture.owner);
        assert_eq!(
            fixture.bytes.as_slice(),
            case.bytes,
            "{} path must match its include_bytes! case ({:?})",
            fixture.owner,
            fixture.schema_path
        );
        let document = SchemaDocument::parse(&fixture.bytes).unwrap_or_else(|diagnostics| {
            panic!(
                "failed to parse fixture {} at {:?}: {diagnostics}",
                fixture.owner, fixture.schema_path
            )
        });
        assert_eq!(
            document.canonical_bytes(),
            case.bytes,
            "{} canonical bytes must equal the full checked-in bytes",
            fixture.owner
        );
        assert_semantics(&document, case);
    }
}

#[test]
fn structured_document_is_readable_through_the_public_seam() {
    let mut value: serde_json::Value = serde_json::from_slice(CASES[1].bytes).unwrap();
    let error = value["types"].as_array_mut().unwrap().pop().unwrap();
    value["types"] = serde_json::json!([
        {"kind": "enum", "name": "Mode", "docs": [], "deprecation": null,
         "variants": [{"name": "Fast", "docs": [], "deprecation": null}]},
        {"kind": "struct", "name": "Request", "docs": [], "deprecation": null, "fields": []},
        error
    ]);
    value["capabilities"][0]["input"]["type"] = serde_json::json!("Request");
    value["capabilities"][0]["output"]["type"] = serde_json::json!("Option<Vec<Mode>>");

    let document = SchemaDocument::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(
        document.data_types[0].shape,
        SchemaDataShape::Enum(_)
    ));
    assert!(
        matches!(document.data_types[1].shape, SchemaDataShape::Struct(ref fields) if fields.is_empty())
    );
    assert_eq!(
        document.capabilities[0].input.leaf,
        TypeExpression::Local("Request".into())
    );
    assert_eq!(
        document.capabilities[0].output.leaf.canonical_name(),
        "Option<Vec<Mode>>"
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&document.canonical_bytes()).unwrap(),
        value
    );
}
