use super::*;
use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use boxology_schema::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SchemaCapability, SchemaDocument, SchemaType,
    SchemaVariant, Shape,
};
use serde_json::json;

const REVISION: &str = "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";

fn document(box_id: &str) -> SchemaDocument {
    SchemaDocument {
        box_id: BoxId::new(box_id).unwrap(),
        capabilities: vec![SchemaCapability {
            name: CapabilityName::new("greet").unwrap(),
            docs: vec!["Greets a caller.".to_owned()],
            deprecation: None,
            error: "GreetError".to_owned(),
            input: InputSlot {
                name: "name".to_owned(),
                leaf: BoundaryLeaf::String,
            },
            output: OutputSlot {
                leaf: BoundaryLeaf::String,
            },
            shape: Shape::Unary,
            max_exposure: ExposureLevel::External,
            idempotency: Idempotency::None,
        }],
        provenance: Provenance::new(json!({"generator": "test"})),
        revision: REVISION.to_owned(),
        types: vec![SchemaType {
            name: "GreetError".to_owned(),
            docs: vec!["Greet failures.".to_owned()],
            deprecation: None,
            variants: vec![SchemaVariant {
                name: "EmptyName".to_owned(),
                docs: vec!["The name was empty.".to_owned()],
                deprecation: None,
            }],
        }],
    }
}

fn assert_unclassified_pair(base: SchemaDocument, submitted: SchemaDocument) {
    assert_ne!(base, submitted);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_eq!(report.findings().len(), 1);
    let finding = &report.findings()[0];
    assert_eq!(finding.code(), "BXC0028");
    assert_eq!(finding.path(), "hello");
    assert_eq!(finding.class(), Class::Incompatible);
    assert_eq!(report.verdict(), Class::Incompatible);
}

fn add_capability(document: &mut SchemaDocument) {
    let mut capability = document.capabilities[0].clone();
    capability.name = CapabilityName::new("wave").unwrap();
    document.capabilities.push(capability);
}
fn add_type(document: &mut SchemaDocument) {
    let mut schema_type = document.types[0].clone();
    schema_type.name = "WaveError".to_owned();
    document.types.push(schema_type);
}

fn shaped_document(capabilities: usize, types: usize) -> SchemaDocument {
    let mut document = document("hello");
    if capabilities == 2 {
        add_capability(&mut document);
    }
    if types == 2 {
        add_type(&mut document);
    }
    document
}

#[test]
fn pairing_errors_render_exactly() {
    assert_eq!(
        classify(None, None).unwrap_err().to_string(),
        "BXC0024 at=\"\" rule=\"classification requires a base or a submitted document\" \
         source=\"specs/s4-contract-change-classification.md D2\""
    );
    let base = document("hello");
    let submitted = document("other");
    assert_eq!(
        classify(Some(&base), Some(&submitted))
            .unwrap_err()
            .to_string(),
        "BXC0025 at=\"/box_id\" rule=\"base and submitted must declare the same box id\" \
         source=\"specs/s4-contract-change-classification.md D2\""
    );
}

#[test]
fn introduced_and_removed_are_exact() {
    let introduced = classify(None, Some(&document("hello"))).unwrap();
    let finding = &introduced.findings()[0];
    assert_eq!(
        (
            finding.code(),
            finding.path(),
            finding.class(),
            introduced.verdict()
        ),
        ("BXC0026", "hello", Class::Additive, Class::Additive)
    );

    let removed = classify(Some(&document("hello")), None).unwrap();
    let finding = &removed.findings()[0];
    assert_eq!(
        (
            finding.code(),
            finding.path(),
            finding.class(),
            removed.verdict()
        ),
        ("BXC0027", "hello", Class::Incompatible, Class::Incompatible)
    );
}

#[test]
fn independently_built_identical_documents_are_unchanged() {
    let base = document("hello");
    let submitted = document("hello");
    assert_eq!(base, submitted);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert!(report.findings().is_empty());
    assert_eq!(report.verdict(), Class::Unchanged);
}

#[test]
fn provenance_only_difference_is_unchanged() {
    let base = document("hello");
    let mut submitted = document("hello");
    submitted.provenance = Provenance::new(json!({"generator": "different"}));
    assert_ne!(base, submitted);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert!(report.findings().is_empty());
    assert_eq!(report.verdict(), Class::Unchanged);
}

#[test]
fn every_effectively_mutable_comparable_field_fails_closed() {
    let mutations: &[fn(&mut SchemaDocument)] = &[
        |document| document.revision.push('x'),
        |document| document.capabilities[0].name = CapabilityName::new("wave").unwrap(),
        |document| document.capabilities[0].docs.push("New docs.".to_owned()),
        |document| document.capabilities[0].deprecation = Some("use wave2".to_owned()),
        |document| document.capabilities[0].error = "WaveError".to_owned(),
        |document| document.capabilities[0].input.name.push('x'),
        |document| document.capabilities[0].input.leaf = BoundaryLeaf::Bool,
        |document| document.capabilities[0].output.leaf = BoundaryLeaf::Bool,
        |document| document.capabilities[0].max_exposure = ExposureLevel::Internal,
        |document| document.capabilities[0].idempotency = Idempotency::Inherent,
        |document| document.types[0].name = "WaveError".to_owned(),
        |document| document.types[0].docs.push("New docs.".to_owned()),
        |document| document.types[0].deprecation = Some("use another error".to_owned()),
        |document| document.types[0].variants[0].name = "MissingName".to_owned(),
        |document| {
            document.types[0].variants[0]
                .docs
                .push("New docs.".to_owned())
        },
        |document| document.types[0].variants[0].deprecation = Some("retired".to_owned()),
        |document| {
            document.types[0].variants.push(SchemaVariant {
                name: "Other".to_owned(),
                docs: Vec::new(),
                deprecation: None,
            })
        },
    ];
    for mutate in mutations {
        let mut submitted = document("hello");
        mutate(&mut submitted);
        assert_unclassified_pair(document("hello"), submitted);
    }
    // Shape has only `Unary` in the current format-1 vocabulary, so it has no effective mutation.
}

#[test]
fn collection_shape_changes_fail_closed() {
    type Case = (usize, usize, fn(&mut SchemaDocument));
    let cases: &[Case] = &[
        (1, 1, add_capability),
        (1, 1, |document| {
            document.capabilities.pop();
        }),
        (2, 1, |document| document.capabilities.swap(0, 1)),
        (1, 1, add_type),
        (1, 1, |document| {
            document.types.pop();
        }),
        (1, 2, |document| document.types.swap(0, 1)),
    ];
    for &(capabilities, types, mutate) in cases {
        let base = shaped_document(capabilities, types);
        let mut submitted = shaped_document(capabilities, types);
        mutate(&mut submitted);
        assert_unclassified_pair(base, submitted);
    }
}

#[test]
fn box_id_is_a_pairing_error_before_the_fail_closed_default() {
    let base = document("hello");
    let submitted = document("other");
    let diagnostics = classify(Some(&base), Some(&submitted)).unwrap_err();
    let diagnostic = diagnostics.into_vec().pop().unwrap();
    assert_eq!(diagnostic.code(), "BXC0025");
    assert_eq!(diagnostic.location(), "/box_id");
}

#[test]
fn maximum_severity_wins_in_both_finding_orders() {
    let low = Finding {
        code: "BXC0026",
        path: "hello".to_owned(),
        class: Class::Additive,
    };
    let high = Finding {
        code: "BXC0028",
        path: "hello".to_owned(),
        class: Class::Incompatible,
    };
    assert_eq!(report(vec![low, high]).verdict, Class::Incompatible);

    let low = Finding {
        code: "BXC0026",
        path: "hello".to_owned(),
        class: Class::Additive,
    };
    let high = Finding {
        code: "BXC0028",
        path: "hello".to_owned(),
        class: Class::Incompatible,
    };
    assert_eq!(report(vec![high, low]).verdict, Class::Incompatible);
}

#[test]
fn classes_have_exact_order_and_names() {
    let classes = [
        Class::Unchanged,
        Class::Documentation,
        Class::Deprecation,
        Class::Additive,
        Class::CompatibleWithConditions,
        Class::Incompatible,
    ];
    let names = [
        "unchanged",
        "documentation",
        "deprecation",
        "additive",
        "compatible_with_conditions",
        "incompatible",
    ];
    assert_eq!(classes.map(Class::canonical_name), names);
    for pair in classes.windows(2) {
        assert!(pair[0] < pair[1]);
    }
}

#[test]
fn public_seam_is_send_sync_static() {
    fn bounds<T: Send + Sync + 'static>() {}
    bounds::<Class>();
    bounds::<Finding>();
    bounds::<ClassificationReport>();
}
