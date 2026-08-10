use super::*;
use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use boxology_schema::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SchemaCapability, SchemaDataField,
    SchemaDataShape, SchemaDataType, SchemaDataVariant, SchemaDocument, SchemaField, SchemaPayload,
    SchemaType, SchemaVariant, Shape, TypeExpression,
};
use serde_json::json;

const REVISION: &str = "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";
const OTHER_REVISION: &str =
    "sha256:a45a70dacfc5e3ea7911944d3f4fd385da1de2cdabfac86d554d4a321e3244cc";

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
        data_types: Vec::new(),
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
                payload: SchemaPayload::Unit,
            }],
        }],
    }
}

fn named_document() -> SchemaDocument {
    let mut document = document("hello");
    document.types[0].variants[0].payload = SchemaPayload::Named(vec![
        SchemaField {
            docs: Vec::new(),
            deprecation: None,
            name: "first".to_owned(),
            ty: BoundaryLeaf::String,
        },
        SchemaField {
            docs: Vec::new(),
            deprecation: None,
            name: "second".to_owned(),
            ty: BoundaryLeaf::I64,
        },
    ]);
    document
}

fn named_fields(document: &mut SchemaDocument) -> &mut Vec<SchemaField> {
    let SchemaPayload::Named(fields) = &mut document.types[0].variants[0].payload else {
        unreachable!("named payload")
    };
    fields
}

fn variant(name: &str) -> SchemaVariant {
    SchemaVariant {
        name: name.to_owned(),
        docs: Vec::new(),
        deprecation: None,
        payload: SchemaPayload::Unit,
    }
}

type Expected<'a> = (
    &'a str,
    &'a str,
    &'a str,
    Class,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

macro_rules! e {
    ($code:literal, $path:literal, $kind:literal, $class:expr, $base:expr, $submitted:expr, $condition:expr) => {
        ($code, $path, $kind, $class, $base, $submitted, $condition)
    };
}

fn assert_exact_report(report: &ClassificationReport, expected: &[Expected<'_>], verdict: Class) {
    assert_eq!(report.findings().len(), expected.len());
    for (finding, expected) in report.findings().iter().zip(expected) {
        let (code, path, kind, class, base, submitted, condition) = *expected;
        assert_eq!(finding.code(), code);
        assert_eq!(finding.path(), path);
        assert_eq!(finding.kind(), kind);
        assert_eq!(finding.class(), class);
        assert_eq!(finding.base_excerpt(), base);
        assert_eq!(finding.submitted_excerpt(), submitted);
        assert_eq!(finding.condition(), condition);
    }
    assert_eq!(report.verdict(), verdict);
}

fn assert_unclassified_pair(base: SchemaDocument, submitted: SchemaDocument) {
    assert_ne!(base, submitted);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[(
            "BXC0028",
            "hello",
            "unclassified change",
            Class::Incompatible,
            None,
            None,
            None,
        )],
        Class::Incompatible,
    );
}

const INTEGRITY_EQUAL_REVISIONS: &str = "BXC0037 at=\"\" rule=\"findings under equal revisions mean the projection and the \
classifier disagree\" source=\"specs/s4-contract-change-classification.md D6\"";

const INTEGRITY_SILENCE: &str = "BXC0038 at=\"\" rule=\"differing revisions with no finding mean the projection and the \
classifier disagree\" source=\"specs/s4-contract-change-classification.md D6\"";

fn assert_integrity_equal_revisions(base: SchemaDocument, submitted: SchemaDocument) {
    assert_eq!(base.revision, submitted.revision);
    assert_ne!(base, submitted);
    let diagnostics = classify(Some(&base), Some(&submitted)).unwrap_err();
    let diagnostic = diagnostics.into_vec().pop().unwrap();
    assert_eq!(diagnostic.code(), "BXC0037");
    assert_eq!(diagnostic.location(), "");
    assert_eq!(diagnostic.to_string(), INTEGRITY_EQUAL_REVISIONS);
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

fn two_error_document() -> SchemaDocument {
    let mut document = document("hello");
    let mut capability = document.capabilities[0].clone();
    capability.name = CapabilityName::new("wave").unwrap();
    capability.error = "WaveError".to_owned();
    document.capabilities.push(capability);

    let greet_type = document.types[0].clone();
    let mut wave_type = greet_type.clone();
    wave_type.name = "WaveError".to_owned();
    document.types = vec![wave_type, greet_type];
    document
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

fn structured_document() -> SchemaDocument {
    let mut document = document("hello");
    document.capabilities[0].input.leaf = TypeExpression::Local("Profile".into());
    document.capabilities[0].output.leaf = TypeExpression::Option(Box::new(TypeExpression::Vec(
        Box::new(TypeExpression::Local("Profile".into())),
    )));
    document.data_types = vec![
        SchemaDataType {
            name: "Mood".into(),
            docs: Vec::new(),
            deprecation: None,
            shape: SchemaDataShape::Enum(vec![data_variant("Calm"), data_variant("Busy")]),
        },
        SchemaDataType {
            name: "Profile".into(),
            docs: Vec::new(),
            deprecation: None,
            shape: SchemaDataShape::Struct(vec![
                data_field("name", TypeExpression::String),
                data_field(
                    "mood",
                    TypeExpression::Option(Box::new(TypeExpression::Local("Mood".into()))),
                ),
                data_field(
                    "tags",
                    TypeExpression::Vec(Box::new(TypeExpression::String)),
                ),
            ]),
        },
        SchemaDataType {
            name: "Archive".into(),
            docs: Vec::new(),
            deprecation: None,
            shape: SchemaDataShape::Struct(Vec::new()),
        },
    ];
    document
}

fn data_field(name: &str, ty: TypeExpression) -> SchemaDataField {
    SchemaDataField {
        name: name.into(),
        docs: Vec::new(),
        deprecation: None,
        ty,
    }
}

fn data_variant(name: &str) -> SchemaDataVariant {
    SchemaDataVariant {
        name: name.into(),
        docs: Vec::new(),
        deprecation: None,
    }
}

fn data_type_mut<'a>(document: &'a mut SchemaDocument, name: &str) -> &'a mut SchemaDataType {
    document
        .data_types
        .iter_mut()
        .find(|item| item.name == name)
        .unwrap()
}

fn profile_fields(document: &mut SchemaDocument) -> &mut Vec<SchemaDataField> {
    let SchemaDataShape::Struct(fields) = &mut data_type_mut(document, "Profile").shape else {
        unreachable!("Profile is a struct")
    };
    fields
}

fn mood_variants(document: &mut SchemaDocument) -> &mut Vec<SchemaDataVariant> {
    let SchemaDataShape::Enum(variants) = &mut data_type_mut(document, "Mood").shape else {
        unreachable!("Mood is an enum")
    };
    variants
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
    assert_exact_report(
        &introduced,
        &[(
            "BXC0026",
            "hello",
            "contract introduced",
            Class::Additive,
            None,
            Some("hello"),
            None,
        )],
        Class::Additive,
    );

    let removed = classify(Some(&document("hello")), None).unwrap();
    assert_exact_report(
        &removed,
        &[(
            "BXC0027",
            "hello",
            "contract removed",
            Class::Incompatible,
            Some("hello"),
            None,
            None,
        )],
        Class::Incompatible,
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
fn types_only_difference_under_equal_revisions_is_integrity_error() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants.push(variant("Other"));
    assert_eq!(base.capabilities, submitted.capabilities);
    assert_eq!(base.provenance, submitted.provenance);
    assert_ne!(base.types, submitted.types);
    assert_integrity_equal_revisions(base, submitted);
}

#[test]
fn reachability_uses_union_of_both_capability_graphs() {
    let base = document("hello");
    let mut submitted = document("hello");
    submitted.capabilities[0].error = "WaveError".to_owned();
    let roles = reachability(&base, &submitted);
    assert_eq!(
        roles.get("GreetError"),
        Some(&Roles {
            input: false,
            output: true
        })
    );
    assert_eq!(
        roles.get("WaveError"),
        Some(&Roles {
            input: false,
            output: true
        })
    );
    assert!(roles.values().all(|roles| !roles.input));
}

#[test]
fn type_changes_detects_variant_removal() {
    let mut base = document("hello");
    base.types[0].variants.push(variant("Other"));
    let submitted = document("hello");
    let roles = reachability(&base, &submitted);
    let changes = type_changes(&base, &submitted, &roles);
    assert_eq!(
        changes,
        Vec::from([TypeChange::VariantRemoved {
            type_name: "GreetError".to_owned(),
            variant_name: "Other".to_owned(),
            roles: Roles {
                input: false,
                output: true
            },
        }])
    );
}

#[test]
fn type_changes_detects_variant_reorder() {
    let mut base = document("hello");
    base.types[0].variants.push(variant("Other"));
    let mut submitted = base.clone();
    submitted.types[0].variants.swap(0, 1);
    let roles = reachability(&base, &submitted);
    let changes = type_changes(&base, &submitted, &roles);
    assert_eq!(
        changes,
        Vec::from([TypeChange::VariantsReordered {
            type_name: "GreetError".to_owned(),
        }])
    );
}

#[test]
fn type_changes_detects_types_reorder() {
    let base = two_error_document();
    let mut submitted = base.clone();
    submitted.types.swap(0, 1);
    let roles = reachability(&base, &submitted);
    let changes = type_changes(&base, &submitted, &roles);
    assert_eq!(changes, Vec::from([TypeChange::TypesReordered]));
}

#[test]
fn type_changes_detects_coarse_payload_change() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants[0].payload = SchemaPayload::Value {
        docs: Vec::new(),
        deprecation: None,
        ty: BoundaryLeaf::String,
    };
    let roles = reachability(&base, &submitted);
    let changes = type_changes(&base, &submitted, &roles);
    assert_eq!(
        changes,
        Vec::from([TypeChange::VariantPayloadChanged {
            type_name: "GreetError".to_owned(),
            variant_name: "EmptyName".to_owned(),
            roles: Roles {
                input: false,
                output: true
            },
        }])
    );
}

#[test]
fn value_payload_metadata_and_type_changes_are_independent() {
    let mut base = document("hello");
    base.types[0].variants[0].payload = SchemaPayload::Value {
        docs: Vec::new(),
        deprecation: None,
        ty: BoundaryLeaf::String,
    };
    let mut submitted = base.clone();
    submitted.types[0].variants[0].payload = SchemaPayload::Value {
        docs: vec!["Changed.".to_owned()],
        deprecation: Some("retired".to_owned()),
        ty: BoundaryLeaf::Bool,
    };
    let roles = reachability(&base, &submitted);
    assert_eq!(
        type_changes(&base, &submitted, &roles),
        Vec::from([
            TypeChange::PayloadDocsChanged {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
            },
            TypeChange::PayloadDeprecationChanged {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
            },
            TypeChange::PayloadTypeChanged {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
            },
        ])
    );
}

#[test]
fn capability_changes_emit_independent_metadata_kinds_in_contract_order() {
    let base = document("hello");
    let mut submitted = base.clone();
    let capability = &mut submitted.capabilities[0];
    capability.docs.push("More detail.".to_owned());
    capability.deprecation = Some("use wave".to_owned());
    capability.error = "OtherError".to_owned();
    capability.max_exposure = ExposureLevel::Internal;
    capability.idempotency = Idempotency::Inherent;

    assert_eq!(
        capability_changes(&base, &submitted),
        Vec::from([
            CapabilityChange::CapabilityDocsChanged {
                name: "greet".to_owned(),
            },
            CapabilityChange::CapabilityDeprecationChanged {
                name: "greet".to_owned(),
            },
            CapabilityChange::CapabilityErrorChanged {
                name: "greet".to_owned(),
            },
            CapabilityChange::CapabilityExposureChanged {
                name: "greet".to_owned(),
                base: ExposureLevel::External,
                submitted: ExposureLevel::Internal,
            },
            CapabilityChange::CapabilityIdempotencyChanged {
                name: "greet".to_owned(),
                base: Idempotency::None,
                submitted: Idempotency::Inherent,
            },
        ])
    );
}

#[test]
fn named_field_changes_emit_aligned_raw_kinds_in_deterministic_order() {
    let mut base = named_document();
    let SchemaPayload::Named(base_fields) = &mut base.types[0].variants[0].payload else {
        unreachable!("named payload")
    };
    base_fields.push(SchemaField {
        docs: Vec::new(),
        deprecation: None,
        name: "third".to_owned(),
        ty: BoundaryLeaf::Bool,
    });

    let mut submitted = base.clone();
    let SchemaPayload::Named(submitted_fields) = &mut submitted.types[0].variants[0].payload else {
        unreachable!("named payload")
    };
    submitted_fields[0].docs.push("New docs.".to_owned());
    submitted_fields[0].deprecation = Some("retired".to_owned());
    submitted_fields[0].ty = BoundaryLeaf::U64;
    submitted_fields.remove(1);
    submitted_fields.swap(0, 1);
    submitted_fields.push(SchemaField {
        docs: Vec::new(),
        deprecation: None,
        name: "fourth".to_owned(),
        ty: BoundaryLeaf::String,
    });

    let roles = reachability(&base, &submitted);
    assert_eq!(
        type_changes(&base, &submitted, &roles),
        Vec::from([
            TypeChange::FieldDocsChanged {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
                field_name: "first".to_owned(),
            },
            TypeChange::FieldDeprecationChanged {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
                field_name: "first".to_owned(),
            },
            TypeChange::FieldTypeChanged {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
                field_name: "first".to_owned(),
            },
            TypeChange::FieldRemoved {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
                field_name: "second".to_owned(),
                roles: Roles {
                    input: false,
                    output: true,
                },
            },
            TypeChange::FieldAdded {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
                field_name: "fourth".to_owned(),
                roles: Roles {
                    input: false,
                    output: true,
                },
            },
            TypeChange::FieldsReordered {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
            },
        ])
    );
}

#[test]
fn named_field_rename_is_raw_remove_then_add() {
    let base = named_document();
    let mut submitted = base.clone();
    named_fields(&mut submitted)[0].name = "renamed".to_owned();

    let roles = reachability(&base, &submitted);
    assert_eq!(
        type_changes(&base, &submitted, &roles),
        Vec::from([
            TypeChange::FieldRemoved {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
                field_name: "first".to_owned(),
                roles: Roles {
                    input: false,
                    output: true,
                },
            },
            TypeChange::FieldAdded {
                type_name: "GreetError".to_owned(),
                variant_name: "EmptyName".to_owned(),
                field_name: "renamed".to_owned(),
                roles: Roles {
                    input: false,
                    output: true,
                },
            },
        ])
    );
}

#[test]
fn type_changes_treats_type_rename_as_remove_plus_add() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].name = "WaveError".to_owned();
    let roles = reachability(&base, &submitted);
    let changes = type_changes(&base, &submitted, &roles);
    assert_eq!(
        changes,
        Vec::from([
            TypeChange::TypeRemoved {
                name: "GreetError".to_owned(),
                roles: Roles {
                    input: false,
                    output: true
                },
            },
            TypeChange::TypeAdded {
                name: "WaveError".to_owned(),
                roles: Roles {
                    input: false,
                    output: false
                },
            },
        ])
    );
}

#[rustfmt::skip]
#[test]
fn single_referenced_error_variant_addition_is_conditional() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants.push(variant("Other"));
    submitted.revision = OTHER_REVISION.to_owned();

    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(&report, &[e!("BXC0036", "hello/type/GreetError/variant/Other", "error variant added", Class::CompatibleWithConditions, None, Some("Other"), Some("unknown-variant tolerance"))], Class::CompatibleWithConditions);
}

#[rustfmt::skip]
#[test]
fn multiple_referenced_error_variant_additions_are_sorted() {
    let base = two_error_document();
    let mut submitted = base.clone();
    submitted.types[1].variants.push(variant("GreetOther"));
    submitted.types[0].variants.push(variant("WaveOther"));
    submitted.revision = OTHER_REVISION.to_owned();

    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0036", "hello/type/GreetError/variant/GreetOther", "error variant added", Class::CompatibleWithConditions, None, Some("GreetOther"), Some("unknown-variant tolerance")),
            e!("BXC0036", "hello/type/WaveError/variant/WaveOther", "error variant added", Class::CompatibleWithConditions, None, Some("WaveOther"), Some("unknown-variant tolerance")),
        ],
        Class::CompatibleWithConditions,
    );
}

fn with_flipped_revision(mut document: SchemaDocument) -> SchemaDocument {
    document.revision = OTHER_REVISION.to_owned();
    document
}

fn assert_variant_incompatible(base: SchemaDocument, mutate: impl FnOnce(&mut SchemaDocument)) {
    let mut submitted = base.clone();
    mutate(&mut submitted);
    submitted.revision = OTHER_REVISION.to_owned();
    assert_unclassified_pair(base, submitted);
}

#[test]
fn variant_changes_outside_named_addition_fail_closed() {
    // Reorder of common variants has no named row.
    let mut base = document("hello");
    base.types[0].variants.push(variant("Other"));
    assert_variant_incompatible(base, |document| {
        document.types[0].variants.swap(0, 1);
    });

    // Variant added on an unreferenced type falls to the fail-closed default (D5 preamble).
    let mut base = document("hello");
    let mut unreferenced = base.types[0].clone();
    unreferenced.name = "UnusedError".to_owned();
    base.types.push(unreferenced);
    assert_variant_incompatible(base, |document| {
        document.types[1].variants.push(variant("Other"));
    });
}

#[rustfmt::skip]
#[test]
fn variant_removed_is_incompatible() {
    let mut base = document("hello");
    base.types[0].variants.push(variant("Other"));
    let submitted = with_flipped_revision(document("hello"));
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0035", "hello/type/GreetError/variant/Other", "variant removed", Class::Incompatible, Some("Other"), None, None)],
        Class::Incompatible,
    );
}

#[test]
fn unreferenced_type_addition_fails_closed() {
    let base = document("hello");
    let mut submitted = base.clone();
    let mut unused = base.types[0].clone();
    unused.name = "UnusedError".to_owned();
    submitted.types.push(unused);
    submitted.revision = OTHER_REVISION.to_owned();
    assert_unclassified_pair(base, submitted);
}

#[rustfmt::skip]
#[test]
fn unclassified_beside_named_finding_fails_closed() {
    // Named docs finding beside an unreferenced-type addition: only this shape distinguishes the
    // unclassified disjunct from check B's empty-finding integrity path (every solo-unclassified
    // fixture is dual-defended).
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].docs.push("Extra type docs.".to_owned());
    let mut unused = base.types[0].clone();
    unused.name = "UnusedError".to_owned();
    submitted.types.push(unused);
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0028", "hello", "unclassified change", Class::Incompatible, None, None, None),
            e!("BXC0033", "hello/type/GreetError", "documentation changed", Class::Documentation, Some("Greet failures."), Some("Greet failures.\nExtra type docs."), None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn variant_payload_kind_change_is_incompatible_at_capability_error() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants[0].payload = SchemaPayload::Value {
        docs: Vec::new(),
        deprecation: None,
        ty: BoundaryLeaf::String,
    };
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0052", "hello.greet/error", "error payload changed", Class::Incompatible, Some("unit"), Some("value"), None)],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn type_docs_changed_is_documentation() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].docs.push("Extra type docs.".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0033", "hello/type/GreetError", "documentation changed", Class::Documentation, Some("Greet failures."), Some("Greet failures.\nExtra type docs."), None)],
        Class::Documentation,
    );
}

#[rustfmt::skip]
#[test]
fn variant_docs_changed_is_documentation() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants[0]
        .docs
        .push("Extra variant docs.".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0033", "hello/type/GreetError/variant/EmptyName", "documentation changed", Class::Documentation, Some("The name was empty."), Some("The name was empty.\nExtra variant docs."), None)],
        Class::Documentation,
    );
}

#[rustfmt::skip]
#[test]
fn type_deprecation_changed_is_deprecation() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].deprecation = Some("use another error".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0034", "hello/type/GreetError", "deprecation changed", Class::Deprecation, None, Some("use another error"), None)],
        Class::Deprecation,
    );
}

#[rustfmt::skip]
#[test]
fn variant_deprecation_changed_is_deprecation() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants[0].deprecation = Some("use another variant".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0034", "hello/type/GreetError/variant/EmptyName", "deprecation changed", Class::Deprecation, None, Some("use another variant"), None)],
        Class::Deprecation,
    );
}

#[rustfmt::skip]
#[test]
fn type_added_with_referencing_capability_is_additive() {
    let base = document("hello");
    let mut submitted = base.clone();
    add_capability(&mut submitted);
    submitted.capabilities[1].error = "WaveError".to_owned();
    let mut wave = base.types[0].clone();
    wave.name = "WaveError".to_owned();
    submitted.types.push(wave);
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0039", "hello.wave", "capability added", Class::Additive, None, Some("wave"), None),
            e!("BXC0031", "hello/type/WaveError", "type added", Class::Additive, None, Some("WaveError"), None),
        ],
        Class::Additive,
    );
}

#[rustfmt::skip]
#[test]
fn type_removed_with_referencing_capability_is_incompatible() {
    let base = two_error_document();
    let mut submitted = document("hello");
    submitted.revision = OTHER_REVISION.to_owned();
    // Keep GreetError; remove WaveError and its capability only.
    assert_eq!(base.types[1].name, "GreetError");
    assert_eq!(base.types[0].name, "WaveError");
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0040", "hello.wave", "capability removed", Class::Incompatible, Some("wave"), None, None),
            e!("BXC0032", "hello/type/WaveError", "type removed", Class::Incompatible, Some("WaveError"), None, None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn capability_added_is_additive() {
    let base = document("hello");
    let mut submitted = base.clone();
    add_capability(&mut submitted);
    submitted.revision = OTHER_REVISION.to_owned();
    assert_ne!(base.capabilities, submitted.capabilities);
    assert_eq!(base.types, submitted.types);
    assert_ne!(base.revision, submitted.revision);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0039", "hello.wave", "capability added", Class::Additive, None, Some("wave"), None)],
        Class::Additive,
    );
}

#[rustfmt::skip]
#[test]
fn capability_removed_is_incompatible() {
    let mut base = document("hello");
    add_capability(&mut base);
    let submitted = with_flipped_revision(document("hello"));
    assert_ne!(base.capabilities, submitted.capabilities);
    assert_eq!(base.types, submitted.types);
    assert_ne!(base.revision, submitted.revision);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0040", "hello.wave", "capability removed", Class::Incompatible, Some("wave"), None, None)],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn capability_rename_is_remove_plus_add() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.capabilities[0].name = CapabilityName::new("wave").unwrap();
    submitted.revision = OTHER_REVISION.to_owned();
    assert_ne!(base.capabilities, submitted.capabilities);
    assert_eq!(base.types, submitted.types);
    assert_ne!(base.revision, submitted.revision);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0040", "hello.greet", "capability removed", Class::Incompatible, Some("greet"), None, None),
            e!("BXC0039", "hello.wave", "capability added", Class::Additive, None, Some("wave"), None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn input_name_changed_is_incompatible() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.capabilities[0].input.name = "label".to_owned();
    submitted.revision = OTHER_REVISION.to_owned();
    assert_ne!(base.capabilities, submitted.capabilities);
    assert_eq!(base.types, submitted.types);
    assert_ne!(base.revision, submitted.revision);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0041", "hello.greet/input", "capability input parameter name changed", Class::Incompatible, Some("name"), Some("label"), None)],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn input_leaf_changed_is_incompatible() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.capabilities[0].input.leaf = BoundaryLeaf::Bool;
    submitted.revision = OTHER_REVISION.to_owned();
    assert_ne!(base.capabilities, submitted.capabilities);
    assert_eq!(base.types, submitted.types);
    assert_ne!(base.revision, submitted.revision);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0042", "hello.greet/input", "capability input type changed", Class::Incompatible, Some("String"), Some("bool"), None)],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn output_leaf_changed_is_incompatible() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.capabilities[0].output.leaf = BoundaryLeaf::Bool;
    submitted.revision = OTHER_REVISION.to_owned();
    assert_ne!(base.capabilities, submitted.capabilities);
    assert_eq!(base.types, submitted.types);
    assert_ne!(base.revision, submitted.revision);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[e!("BXC0043", "hello.greet/output", "capability output type changed", Class::Incompatible, Some("String"), Some("bool"), None)],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn input_name_and_leaf_changes_share_one_path_and_sort_by_code() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.capabilities[0].input.name = "label".to_owned();
    submitted.capabilities[0].input.leaf = BoundaryLeaf::Bool;
    submitted.revision = OTHER_REVISION.to_owned();
    assert_ne!(base.capabilities, submitted.capabilities);
    assert_eq!(base.types, submitted.types);
    assert_ne!(base.revision, submitted.revision);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0041", "hello.greet/input", "capability input parameter name changed", Class::Incompatible, Some("name"), Some("label"), None),
            e!("BXC0042", "hello.greet/input", "capability input type changed", Class::Incompatible, Some("String"), Some("bool"), None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn capability_metadata_rows_are_exact_and_maximum_severity_wins() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.capabilities[0].docs.push("More docs.".to_owned());
    submitted.capabilities[0].deprecation = Some("retired".to_owned());
    submitted.capabilities[0].error = "WaveError".to_owned();
    submitted.capabilities[0].max_exposure = ExposureLevel::Internal;
    submitted.capabilities[0].idempotency = Idempotency::Inherent;
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0033", "hello.greet", "documentation changed", Class::Documentation, Some("Greets a caller."), Some("Greets a caller.\nMore docs."), None),
            e!("BXC0034", "hello.greet", "deprecation changed", Class::Deprecation, None, Some("retired"), None),
            e!("BXC0044", "hello.greet/error", "capability declared error changed", Class::Incompatible, Some("GreetError"), Some("WaveError"), None),
            e!("BXC0046", "hello.greet/exposure", "max exposure lowered", Class::Incompatible, Some("external"), Some("internal"), None),
            e!("BXC0047", "hello.greet/idempotency", "idempotency strengthened", Class::Additive, Some("none"), Some("inherent"), None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn exposure_and_idempotency_directions_have_distinct_codes() {
    let mut low = document("hello");
    low.capabilities[0].max_exposure = ExposureLevel::CodeOnly;
    let mut high = low.clone();
    high.capabilities[0].max_exposure = ExposureLevel::External;
    high.capabilities[0].idempotency = Idempotency::Inherent;
    high.revision = OTHER_REVISION.to_owned();
    assert_exact_report(
        &classify(Some(&low), Some(&high)).unwrap(),
        &[
            e!("BXC0045", "hello.greet/exposure", "max exposure raised", Class::Additive, Some("code_only"), Some("external"), None),
            e!("BXC0047", "hello.greet/idempotency", "idempotency strengthened", Class::Additive, Some("none"), Some("inherent"), None),
        ],
        Class::Additive,
    );
    low.revision = OTHER_REVISION.to_owned();
    high.revision = REVISION.to_owned();
    assert_exact_report(
        &classify(Some(&high), Some(&low)).unwrap(),
        &[
            e!("BXC0046", "hello.greet/exposure", "max exposure lowered", Class::Incompatible, Some("external"), Some("code_only"), None),
            e!("BXC0048", "hello.greet/idempotency", "idempotency weakened", Class::Incompatible, Some("inherent"), Some("none"), None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn mixed_variant_addition_and_type_docs_keeps_conditional_verdict() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants.push(variant("Other"));
    submitted.types[0].docs.push("Extra type docs.".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0033", "hello/type/GreetError", "documentation changed", Class::Documentation, Some("Greet failures."), Some("Greet failures.\nExtra type docs."), None),
            e!("BXC0036", "hello/type/GreetError/variant/Other", "error variant added", Class::CompatibleWithConditions, None, Some("Other"), Some("unknown-variant tolerance")),
        ],
        Class::CompatibleWithConditions,
    );
}

#[rustfmt::skip]
#[test]
fn mixed_variant_addition_and_capability_docs_keeps_conditional_verdict() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants.push(variant("Other"));
    submitted.capabilities[0].docs.push("More docs.".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0033", "hello.greet", "documentation changed", Class::Documentation, Some("Greets a caller."), Some("Greets a caller.\nMore docs."), None),
            e!("BXC0036", "hello/type/GreetError/variant/Other", "error variant added", Class::CompatibleWithConditions, None, Some("Other"), Some("unknown-variant tolerance")),
        ],
        Class::CompatibleWithConditions,
    );
}

#[test]
fn equal_revision_variant_addition_is_integrity_error() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants.push(variant("Other"));
    assert_integrity_equal_revisions(base, submitted);
}

#[test]
fn revision_only_difference_is_integrity_silence() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.revision = OTHER_REVISION.to_owned();
    assert_ne!(base.revision, submitted.revision);
    let diagnostics = classify(Some(&base), Some(&submitted)).unwrap_err();
    let diagnostic = diagnostics.into_vec().pop().unwrap();
    assert_eq!(diagnostic.code(), "BXC0038");
    assert_eq!(diagnostic.location(), "");
    assert_eq!(diagnostic.to_string(), INTEGRITY_SILENCE);
}

#[test]
fn structured_reachability_uses_both_graphs_and_reaches_a_fixed_point() {
    let mut base = structured_document();
    base.data_types.push(SchemaDataType {
        name: "Envelope".into(),
        docs: Vec::new(),
        deprecation: None,
        shape: SchemaDataShape::Struct(vec![data_field(
            "profile",
            TypeExpression::Local("Profile".into()),
        )]),
    });
    base.capabilities[0].input.leaf = TypeExpression::Option(Box::new(TypeExpression::Vec(
        Box::new(TypeExpression::Local("Envelope".into())),
    )));
    base.capabilities[0].output.leaf = TypeExpression::Local("Envelope".into());
    let mut submitted = base.clone();
    profile_fields(&mut base)[1].ty = TypeExpression::String;
    let envelope = submitted.data_types.pop().unwrap();
    submitted.data_types.insert(1, envelope);
    profile_fields(&mut submitted).push(data_field(
        "envelope",
        TypeExpression::Local("Envelope".into()),
    ));
    let SchemaDataShape::Struct(fields) = &mut data_type_mut(&mut submitted, "Envelope").shape
    else {
        unreachable!("Envelope is a struct")
    };
    fields[0].ty = TypeExpression::String;

    let roles = reachability(&base, &submitted);
    let both = Roles {
        input: true,
        output: true,
    };
    assert_eq!(roles_for(&roles, "Envelope"), both);
    assert_eq!(
        roles_for(&roles, "Profile"),
        both,
        "base-only edge propagates"
    );
    assert_eq!(
        roles_for(&roles, "Mood"),
        both,
        "submitted-only edge propagates transitively"
    );
    assert_eq!(
        roles_for(&roles, "Archive"),
        Roles {
            input: false,
            output: false
        }
    );
    assert_eq!(
        roles_for(&roles, "GreetError"),
        Roles {
            input: false,
            output: true
        }
    );
}

fn assert_data_mutation(mutate: fn(&mut SchemaDocument), expected: Vec<DataChange>) {
    let base = structured_document();
    let mut submitted = base.clone();
    mutate(&mut submitted);
    submitted.revision = OTHER_REVISION.into();
    let roles = reachability(&base, &submitted);
    assert_eq!(data_changes(&base, &submitted, &roles), expected);
    assert_unclassified_pair(base, submitted);
}

#[rustfmt::skip]
#[test]
fn structured_raw_change_corpus_is_exact_and_fail_closed() {
    let none = Roles { input: false, output: false };
    let both = Roles { input: true, output: true };
    type Case = (fn(&mut SchemaDocument), Vec<DataChange>);
    let cases: Vec<Case> = vec![
        (|d| d.data_types.push(SchemaDataType { name: "Cache".into(), docs: Vec::new(), deprecation: None, shape: SchemaDataShape::Struct(Vec::new()) }), vec![DataChange::TypeAdded { name: "Cache".into(), roles: none }]),
        (|d| { d.data_types.pop(); }, vec![DataChange::TypeRemoved { name: "Archive".into(), roles: none }]),
        (|d| data_type_mut(d, "Archive").name = "Vault".into(), vec![DataChange::TypeRemoved { name: "Archive".into(), roles: none }, DataChange::TypeAdded { name: "Vault".into(), roles: none }]),
        (|d| d.data_types.swap(1, 2), vec![DataChange::TypesReordered]),
        (|d| data_type_mut(d, "Archive").shape = SchemaDataShape::Enum(vec![data_variant("Stored")]), vec![DataChange::TypeKindChanged { name: "Archive".into() }]),
        (|d| { let item = data_type_mut(d, "Archive"); item.docs.push("docs".into()); item.deprecation = Some("old".into()); }, vec![DataChange::TypeDocsChanged { name: "Archive".into() }, DataChange::TypeDeprecationChanged { name: "Archive".into() }]),
        (|d| profile_fields(d).push(data_field("active", TypeExpression::Bool)), vec![DataChange::FieldAdded { type_name: "Profile".into(), field_name: "active".into(), roles: both }]),
        (|d| { profile_fields(d).pop(); }, vec![DataChange::FieldRemoved { type_name: "Profile".into(), field_name: "tags".into(), roles: both }]),
        (|d| profile_fields(d)[0].name = "label".into(), vec![DataChange::FieldRemoved { type_name: "Profile".into(), field_name: "name".into(), roles: both }, DataChange::FieldAdded { type_name: "Profile".into(), field_name: "label".into(), roles: both }]),
        (|d| profile_fields(d).swap(0, 2), vec![DataChange::FieldsReordered { type_name: "Profile".into() }]),
        (|d| { let item = &mut profile_fields(d)[0]; item.docs.push("docs".into()); item.deprecation = Some("old".into()); item.ty = TypeExpression::Vec(Box::new(TypeExpression::String)); }, vec![DataChange::FieldDocsChanged { type_name: "Profile".into(), field_name: "name".into() }, DataChange::FieldDeprecationChanged { type_name: "Profile".into(), field_name: "name".into() }, DataChange::FieldTypeChanged { type_name: "Profile".into(), field_name: "name".into() }]),
        (|d| mood_variants(d).push(data_variant("Away")), vec![DataChange::VariantAdded { type_name: "Mood".into(), variant_name: "Away".into(), roles: both }]),
        (|d| { mood_variants(d).pop(); }, vec![DataChange::VariantRemoved { type_name: "Mood".into(), variant_name: "Busy".into(), roles: both }]),
        (|d| mood_variants(d)[0].name = "Quiet".into(), vec![DataChange::VariantRemoved { type_name: "Mood".into(), variant_name: "Calm".into(), roles: both }, DataChange::VariantAdded { type_name: "Mood".into(), variant_name: "Quiet".into(), roles: both }]),
        (|d| mood_variants(d).swap(0, 1), vec![DataChange::VariantsReordered { type_name: "Mood".into() }]),
        (|d| { let item = &mut mood_variants(d)[0]; item.docs.push("docs".into()); item.deprecation = Some("old".into()); }, vec![DataChange::VariantDocsChanged { type_name: "Mood".into(), variant_name: "Calm".into() }, DataChange::VariantDeprecationChanged { type_name: "Mood".into(), variant_name: "Calm".into() }]),
    ];
    for (mutate, expected) in cases {
        assert_data_mutation(mutate, expected);
    }
}

#[rustfmt::skip]
#[test]
fn capability_expression_changes_keep_existing_named_findings() {
    let base = structured_document();
    let mut submitted = base.clone();
    submitted.capabilities[0].input.leaf = TypeExpression::Option(Box::new(TypeExpression::Vec(Box::new(TypeExpression::Local("Profile".into())))));
    submitted.capabilities[0].output.leaf = TypeExpression::Local("Profile".into());
    submitted.revision = OTHER_REVISION.into();
    assert_exact_report(
        &classify(Some(&base), Some(&submitted)).unwrap(),
        &[
            e!("BXC0042", "hello.greet/input", "capability input type changed", Class::Incompatible, Some("Profile"), Some("Option<Vec<Profile>>"), None),
            e!("BXC0043", "hello.greet/output", "capability output type changed", Class::Incompatible, Some("Option<Vec<Profile>>"), Some("Profile"), None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn type_rename_is_remove_plus_unreferenced_add() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].name = "WaveError".to_owned();
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0028", "hello", "unclassified change", Class::Incompatible, None, None, None),
            e!("BXC0032", "hello/type/GreetError", "type removed", Class::Incompatible, Some("GreetError"), None, None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn variant_rename_is_remove_plus_add() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants[0].name = "Renamed".to_owned();
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0035", "hello/type/GreetError/variant/EmptyName", "variant removed", Class::Incompatible, Some("EmptyName"), None, None),
            e!("BXC0036", "hello/type/GreetError/variant/Renamed", "error variant added", Class::CompatibleWithConditions, None, Some("Renamed"), Some("unknown-variant tolerance")),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn named_payload_field_rows_are_exact_and_sorted() {
    let base = named_document();
    let mut submitted = base.clone();
    let fields = named_fields(&mut submitted);
    fields[0].docs.push("new docs".to_owned());
    fields[0].deprecation = Some("retired".to_owned());
    fields[0].ty = BoundaryLeaf::Bool;
    fields.remove(1);
    fields.push(SchemaField {
        docs: Vec::new(),
        deprecation: None,
        name: "third".to_owned(),
        ty: BoundaryLeaf::String,
    });
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            e!("BXC0033", "hello/type/GreetError/variant/EmptyName/field/first", "documentation changed", Class::Documentation, Some(""), Some("new docs"), None),
            e!("BXC0034", "hello/type/GreetError/variant/EmptyName/field/first", "deprecation changed", Class::Deprecation, None, Some("retired"), None),
            e!("BXC0051", "hello/type/GreetError/variant/EmptyName/field/first", "field type changed", Class::Incompatible, Some("String"), Some("bool"), None),
            e!("BXC0050", "hello/type/GreetError/variant/EmptyName/field/second", "field removed", Class::Incompatible, Some("second"), None, None),
            e!("BXC0049", "hello/type/GreetError/variant/EmptyName/field/third", "field added", Class::Additive, None, Some("third"), None),
        ],
        Class::Incompatible,
    );
}

#[rustfmt::skip]
#[test]
fn payload_value_metadata_and_type_classify_independently() {
    let mut base = document("hello");
    base.types[0].variants[0].payload = SchemaPayload::Value {
        docs: Vec::new(),
        deprecation: None,
        ty: BoundaryLeaf::String,
    };
    let mut submitted = base.clone();
    submitted.types[0].variants[0].payload = SchemaPayload::Value {
        docs: vec!["new docs".to_owned()],
        deprecation: Some("retired".to_owned()),
        ty: BoundaryLeaf::Bool,
    };
    submitted.revision = OTHER_REVISION.to_owned();
    assert_exact_report(
        &classify(Some(&base), Some(&submitted)).unwrap(),
        &[
            e!("BXC0052", "hello.greet/error", "error payload changed", Class::Incompatible, Some("String"), Some("bool"), None),
            e!("BXC0033", "hello/type/GreetError/variant/EmptyName", "documentation changed", Class::Documentation, Some(""), Some("new docs"), None),
            e!("BXC0034", "hello/type/GreetError/variant/EmptyName", "deprecation changed", Class::Deprecation, None, Some("retired"), None),
        ],
        Class::Incompatible,
    );
}

#[test]
fn named_payload_field_reorder_stays_fail_closed() {
    let base = named_document();
    let mut submitted = base.clone();
    named_fields(&mut submitted).swap(0, 1);
    submitted.revision = OTHER_REVISION.to_owned();
    assert_unclassified_pair(base, submitted);
}

#[test]
fn collection_shape_changes_fail_closed() {
    // Capability reorder and unreferenced type-graph reorder/add still fail closed.
    // Removing a referenced type is the named BXC0032 path, covered elsewhere.
    type Case = (usize, usize, fn(&mut SchemaDocument));
    let cases: &[Case] = &[
        (2, 1, |document| document.capabilities.swap(0, 1)),
        (1, 1, add_type),
        (1, 2, |document| document.types.swap(0, 1)),
    ];
    for &(capabilities, types, mutate) in cases {
        let base = shaped_document(capabilities, types);
        let mut submitted = shaped_document(capabilities, types);
        mutate(&mut submitted);
        submitted.revision = OTHER_REVISION.to_owned();
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
        kind: "contract introduced",
        class: Class::Additive,
        base_excerpt: None,
        submitted_excerpt: None,
        condition: None,
    };
    let high = Finding {
        code: "BXC0028",
        path: "hello".to_owned(),
        kind: "unclassified change",
        class: Class::Incompatible,
        base_excerpt: None,
        submitted_excerpt: None,
        condition: None,
    };
    assert_eq!(report(vec![low, high]).verdict, Class::Incompatible);

    let low = Finding {
        code: "BXC0026",
        path: "hello".to_owned(),
        kind: "contract introduced",
        class: Class::Additive,
        base_excerpt: None,
        submitted_excerpt: None,
        condition: None,
    };
    let high = Finding {
        code: "BXC0028",
        path: "hello".to_owned(),
        kind: "unclassified change",
        class: Class::Incompatible,
        base_excerpt: None,
        submitted_excerpt: None,
        condition: None,
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

fn assert_renderings(report: &ClassificationReport, text: &str, json: &str) {
    assert_eq!(render_text(report), text);
    assert_eq!(render_json(report), json);
    serde_json::from_str::<serde_json::Value>(json).unwrap();
}

fn finding_key_inventory(json: &str) -> Vec<Vec<&str>> {
    let mut inventories = Vec::new();
    for object in json.split("    {\n").skip(1) {
        let body = object.split("\n    }").next().unwrap_or("");
        let mut keys = Vec::new();
        for line in body.lines() {
            if let Some(key) = line.trim().strip_prefix('"')
                && let Some(name) = key.split('"').next()
            {
                keys.push(name);
            }
        }
        inventories.push(keys);
    }
    inventories
}

#[test]
fn report_renderings_are_byte_exact() {
    let introduced = classify(None, Some(&document("hello"))).unwrap();
    assert_renderings(
        &introduced,
        "classification additive\n\
finding BXC0026 path=\"hello\" additive kind=\"contract introduced\" base=- submitted=\"hello\"\n",
        r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "additive",
  "findings": [
    {
      "code": "BXC0026",
      "path": "hello",
      "kind": "contract introduced",
      "class": "additive",
      "base": null,
      "submitted": "hello"
    }
  ]
}
"#,
    );

    let removed = classify(Some(&document("hello")), None).unwrap();
    assert_renderings(
        &removed,
        "classification incompatible\n\
finding BXC0027 path=\"hello\" incompatible kind=\"contract removed\" base=\"hello\" submitted=-\n",
        r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "incompatible",
  "findings": [
    {
      "code": "BXC0027",
      "path": "hello",
      "kind": "contract removed",
      "class": "incompatible",
      "base": "hello",
      "submitted": null
    }
  ]
}
"#,
    );

    let unchanged = classify(Some(&document("hello")), Some(&document("hello"))).unwrap();
    assert_renderings(
        &unchanged,
        "classification unchanged\n",
        r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "unchanged",
  "findings": []
}
"#,
    );

    let base = document("hello");
    let mut renamed = base.clone();
    renamed.capabilities[0].input.name = "label".to_owned();
    renamed.revision = OTHER_REVISION.to_owned();
    let renamed = classify(Some(&base), Some(&renamed)).unwrap();
    assert_renderings(
        &renamed,
        "classification incompatible\n\
finding BXC0041 path=\"hello.greet/input\" incompatible kind=\"capability input parameter name changed\" base=\"name\" submitted=\"label\"\n",
        r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "incompatible",
  "findings": [
    {
      "code": "BXC0041",
      "path": "hello.greet/input",
      "kind": "capability input parameter name changed",
      "class": "incompatible",
      "base": "name",
      "submitted": "label"
    }
  ]
}
"#,
    );

    let base = document("hello");
    let mut variant_added = base.clone();
    variant_added.types[0].variants.push(variant("Other"));
    variant_added.revision = OTHER_REVISION.to_owned();
    let variant_added = classify(Some(&base), Some(&variant_added)).unwrap();
    assert_renderings(
        &variant_added,
        "classification compatible_with_conditions\n\
finding BXC0036 path=\"hello/type/GreetError/variant/Other\" compatible_with_conditions kind=\"error variant added\" base=- submitted=\"Other\" condition=\"unknown-variant tolerance\"\n",
        r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "compatible_with_conditions",
  "findings": [
    {
      "code": "BXC0036",
      "path": "hello/type/GreetError/variant/Other",
      "kind": "error variant added",
      "class": "compatible_with_conditions",
      "base": null,
      "submitted": "Other",
      "condition": "unknown-variant tolerance"
    }
  ]
}
"#,
    );

    let base = document("hello");
    let mut two_findings = base.clone();
    two_findings.types[0].docs.push("Extra docs".to_owned());
    two_findings.types[0].deprecation = Some("retired".to_owned());
    two_findings.revision = OTHER_REVISION.to_owned();
    let two_findings = classify(Some(&base), Some(&two_findings)).unwrap();
    assert_renderings(
        &two_findings,
        "classification deprecation\n\
finding BXC0033 path=\"hello/type/GreetError\" documentation kind=\"documentation changed\" base=\"Greet failures.\" submitted=\"Greet failures.\\nExtra docs\"\n\
finding BXC0034 path=\"hello/type/GreetError\" deprecation kind=\"deprecation changed\" base=- submitted=\"retired\"\n",
        r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "deprecation",
  "findings": [
    {
      "code": "BXC0033",
      "path": "hello/type/GreetError",
      "kind": "documentation changed",
      "class": "documentation",
      "base": "Greet failures.",
      "submitted": "Greet failures.\nExtra docs"
    },
    {
      "code": "BXC0034",
      "path": "hello/type/GreetError",
      "kind": "deprecation changed",
      "class": "deprecation",
      "base": null,
      "submitted": "retired"
    }
  ]
}
"#,
    );

    let hostile = ClassificationReport {
        findings: vec![Finding {
            code: "BXC0028",
            path: "hello/\"quoted\\path".to_owned(),
            kind: "unclassified change",
            class: Class::Incompatible,
            base_excerpt: Some("base \"quote\\slash".to_owned()),
            submitted_excerpt: Some("submitted \"quote\\slash".to_owned()),
            condition: None,
        }],
        verdict: Class::Incompatible,
    };
    assert_renderings(
        &hostile,
        "classification incompatible\n\
finding BXC0028 path=\"hello/\\\"quoted\\\\path\" incompatible kind=\"unclassified change\" base=\"base \\\"quote\\\\slash\" submitted=\"submitted \\\"quote\\\\slash\"\n",
        r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "incompatible",
  "findings": [
    {
      "code": "BXC0028",
      "path": "hello/\"quoted\\path",
      "kind": "unclassified change",
      "class": "incompatible",
      "base": "base \"quote\\slash",
      "submitted": "submitted \"quote\\slash"
    }
  ]
}
"#,
    );
}

#[test]
fn hostile_controls_render_exactly_in_both_formats() {
    // Literal expected escape bytes for C0, quote/backslash, DEL/C1, separators, and U+202E.
    const ESCAPED: &str = "\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007\\b\\t\\n\\u000b\\f\\r\\u000e\\u000f\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f\\\"\\\\\\u007f\\u0085\\u009b\\u2028\\u2029\\u202e";
    let mut excerpt = String::new();
    for code in 0u8..=0x1f {
        excerpt.push(code as char);
    }
    excerpt.push('"');
    excerpt.push('\\');
    excerpt.push('\u{7f}');
    excerpt.push('\u{85}');
    excerpt.push('\u{9b}');
    excerpt.push('\u{2028}');
    excerpt.push('\u{2029}');
    excerpt.push('\u{202e}');
    let report = ClassificationReport {
        findings: vec![Finding {
            code: "BXC0028",
            path: "hello".to_owned(),
            kind: "unclassified change",
            class: Class::Incompatible,
            base_excerpt: Some(excerpt.clone()),
            submitted_excerpt: Some(excerpt.clone()),
            condition: None,
        }],
        verdict: Class::Incompatible,
    };
    let text = format!(
        "classification incompatible\n\
finding BXC0028 path=\"hello\" incompatible kind=\"unclassified change\" base=\"{ESCAPED}\" submitted=\"{ESCAPED}\"\n"
    );
    let json = format!(
        "{{\n  \"schema\": \"boxology.classification-report@2\",\n  \"verdict\": \"incompatible\",\n  \"findings\": [\n    {{\n      \"code\": \"BXC0028\",\n      \"path\": \"hello\",\n      \"kind\": \"unclassified change\",\n      \"class\": \"incompatible\",\n      \"base\": \"{ESCAPED}\",\n      \"submitted\": \"{ESCAPED}\"\n    }}\n  ]\n}}\n"
    );
    assert_renderings(&report, &text, &json);
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["findings"][0]["base"], excerpt);
    assert_eq!(parsed["findings"][0]["submitted"], excerpt);
    assert!(text.contains("\\u007f\\u0085\\u009b\\u2028\\u2029\\u202e"));
}

#[test]
fn unicode_format_controls_match_ucd_17_and_emit_surrogate_pairs() {
    assert_eq!(std::char::UNICODE_VERSION, (17, 0, 0));
    // Literal expected bytes: previously missed BMP Cf and supplementary Cf as UTF-16 pairs.
    const ESCAPED: &str =
        "\\u0600\\u070f\\ud804\\udcbd\\ud82f\\udca0\\ud834\\udd73\\udb40\\udc01\\udb40\\udc7f";
    let excerpt = [
        '\u{0600}',
        '\u{070F}',
        '\u{110BD}',
        '\u{1BCA0}',
        '\u{1D173}',
        '\u{E0001}',
        '\u{E007F}',
    ]
    .into_iter()
    .collect::<String>();
    let report = ClassificationReport {
        findings: vec![Finding {
            code: "BXC0028",
            path: "hello".to_owned(),
            kind: "unclassified change",
            class: Class::Incompatible,
            base_excerpt: Some(excerpt.clone()),
            submitted_excerpt: Some(excerpt.clone()),
            condition: None,
        }],
        verdict: Class::Incompatible,
    };
    let text = format!(
        "classification incompatible\n\
finding BXC0028 path=\"hello\" incompatible kind=\"unclassified change\" base=\"{ESCAPED}\" submitted=\"{ESCAPED}\"\n"
    );
    let json = format!(
        "{{\n  \"schema\": \"boxology.classification-report@2\",\n  \"verdict\": \"incompatible\",\n  \"findings\": [\n    {{\n      \"code\": \"BXC0028\",\n      \"path\": \"hello\",\n      \"kind\": \"unclassified change\",\n      \"class\": \"incompatible\",\n      \"base\": \"{ESCAPED}\",\n      \"submitted\": \"{ESCAPED}\"\n    }}\n  ]\n}}\n"
    );
    assert_renderings(&report, &text, &json);
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["findings"][0]["base"], excerpt);
    assert_eq!(parsed["findings"][0]["submitted"], excerpt);
}

#[test]
fn hostile_identity_path_is_escaped_end_to_end() {
    let mut base = document("hello");
    let hostile_name = "GreetError\nfinding BXC9999 forged incompatible";
    base.types[0].name = hostile_name.to_owned();
    base.capabilities[0].error = hostile_name.to_owned();
    let mut submitted = base.clone();
    submitted.types[0].docs.push("Extra docs".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_renderings(
        &report,
        "classification documentation\n\
finding BXC0033 path=\"hello/type/GreetError\\nfinding BXC9999 forged incompatible\" documentation kind=\"documentation changed\" base=\"Greet failures.\" submitted=\"Greet failures.\\nExtra docs\"\n",
        r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "documentation",
  "findings": [
    {
      "code": "BXC0033",
      "path": "hello/type/GreetError\nfinding BXC9999 forged incompatible",
      "kind": "documentation changed",
      "class": "documentation",
      "base": "Greet failures.",
      "submitted": "Greet failures.\nExtra docs"
    }
  ]
}
"#,
    );
    let text = render_text(&report);
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("finding "))
            .count(),
        1
    );
    assert!(text.contains("path=\"hello/type/GreetError\\nfinding BXC9999 forged incompatible\""));
}

#[test]
fn supplementary_format_control_is_escaped_end_to_end() {
    assert_eq!(std::char::UNICODE_VERSION, (17, 0, 0));
    let mut base = document("hello");
    let cf = '\u{1BCA0}';
    let hostile_name = format!("GreetError{cf}");
    base.types[0].name = hostile_name.clone();
    base.capabilities[0].error = hostile_name;
    let mut submitted = base.clone();
    submitted.types[0].docs.push("Extra docs".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    const PAIR: &str = "\\ud82f\\udca0";
    let text = render_text(&report);
    let json = render_json(&report);
    assert_eq!(
        text.lines()
            .filter(|line| line.starts_with("finding "))
            .count(),
        1
    );
    assert_eq!(
        text,
        format!(
            "classification documentation\n\
finding BXC0033 path=\"hello/type/GreetError{PAIR}\" documentation kind=\"documentation changed\" base=\"Greet failures.\" submitted=\"Greet failures.\\nExtra docs\"\n"
        )
    );
    assert!(
        text.as_bytes()
            .windows(PAIR.len())
            .any(|w| w == PAIR.as_bytes())
    );
    let expected_json = format!(
        "{{\n  \"schema\": \"boxology.classification-report@2\",\n  \"verdict\": \"documentation\",\n  \"findings\": [\n    {{\n      \"code\": \"BXC0033\",\n      \"path\": \"hello/type/GreetError{PAIR}\",\n      \"kind\": \"documentation changed\",\n      \"class\": \"documentation\",\n      \"base\": \"Greet failures.\",\n      \"submitted\": \"Greet failures.\\nExtra docs\"\n    }}\n  ]\n}}\n"
    );
    assert_eq!(json, expected_json);
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        parsed["findings"][0]["path"],
        format!("hello/type/GreetError{cf}")
    );
}

#[test]
fn report_fields_render_exactly_once_and_inventory_is_fixed() {
    let with_condition = Finding {
        code: "BXC0036",
        path: "path/S1".to_owned(),
        kind: "kind/S2",
        class: Class::CompatibleWithConditions,
        base_excerpt: None,
        submitted_excerpt: Some("submitted/S3".to_owned()),
        condition: Some("condition/S4"),
    };
    let without_condition = Finding {
        code: "BXC0041",
        path: "path/S5".to_owned(),
        kind: "kind/S6",
        class: Class::Incompatible,
        base_excerpt: Some("base/S7".to_owned()),
        submitted_excerpt: None,
        condition: None,
    };
    let report = ClassificationReport {
        findings: vec![with_condition, without_condition],
        verdict: Class::Incompatible,
    };
    let text = render_text(&report);
    let json = render_json(&report);
    for sentinel in [
        "BXC0036",
        "path/S1",
        "kind/S2",
        "submitted/S3",
        "condition/S4",
        "BXC0041",
        "path/S5",
        "kind/S6",
        "base/S7",
    ] {
        assert_eq!(text.matches(sentinel).count(), 1, "text {sentinel}");
        assert_eq!(json.matches(sentinel).count(), 1, "json {sentinel}");
    }
    assert_eq!(text.matches("finding ").count(), report.findings().len());
    assert_eq!(json.matches("\"code\":").count(), report.findings().len());
    assert!(text.contains("base=-"));
    assert!(text.contains("submitted=-"));
    assert_eq!(json.matches("\"base\": null").count(), 1);
    assert_eq!(json.matches("\"submitted\": null").count(), 1);
    assert_eq!(
        finding_key_inventory(&json),
        [
            vec![
                "code",
                "path",
                "kind",
                "class",
                "base",
                "submitted",
                "condition"
            ],
            vec!["code", "path", "kind", "class", "base", "submitted"],
        ]
    );
    let missing_kind = json.replacen("\n      \"kind\": \"kind/S2\",", "", 1);
    assert_ne!(
        finding_key_inventory(&missing_kind)[0],
        finding_key_inventory(&json)[0]
    );
    let omitted_base = json.replacen("\n      \"base\": null,", "", 1);
    assert!(!finding_key_inventory(&omitted_base)[0].contains(&"base"));
}

#[test]
fn public_seam_is_send_sync_static() {
    fn bounds<T: Send + Sync + 'static>() {}
    bounds::<Class>();
    bounds::<Finding>();
    bounds::<ClassificationReport>();
}
