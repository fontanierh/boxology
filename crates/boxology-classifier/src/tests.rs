use super::*;
use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use boxology_schema::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SchemaCapability, SchemaDocument, SchemaField,
    SchemaPayload, SchemaType, SchemaVariant, Shape,
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

fn assert_exact_report(
    report: &ClassificationReport,
    expected: &[(&str, &str, Class, Option<&str>)],
    verdict: Class,
) {
    assert_eq!(report.findings().len(), expected.len());
    for (finding, expected) in report.findings().iter().zip(expected) {
        let (code, path, class, condition) = *expected;
        assert_eq!(finding.code(), code);
        assert_eq!(finding.path(), path);
        assert_eq!(finding.class(), class);
        assert_eq!(finding.condition(), condition);
    }
    assert_eq!(report.verdict(), verdict);
}

fn assert_conditional(report: &ClassificationReport, paths: &[&str]) {
    assert_eq!(report.verdict(), Class::CompatibleWithConditions);
    assert_eq!(report.findings().len(), paths.len());
    for (finding, path) in report.findings().iter().zip(paths) {
        assert_eq!(
            (
                finding.code(),
                finding.path(),
                finding.class(),
                finding.condition()
            ),
            (
                "BXC0036",
                *path,
                Class::CompatibleWithConditions,
                Some("unknown-variant tolerance")
            )
        );
    }
}

fn assert_unclassified_pair(base: SchemaDocument, submitted: SchemaDocument) {
    assert_ne!(base, submitted);
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[("BXC0028", "hello", Class::Incompatible, None)],
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
            finding.condition(),
            introduced.verdict()
        ),
        ("BXC0026", "hello", Class::Additive, None, Class::Additive)
    );

    let removed = classify(Some(&document("hello")), None).unwrap();
    let finding = &removed.findings()[0];
    assert_eq!(
        (
            finding.code(),
            finding.path(),
            finding.class(),
            finding.condition(),
            removed.verdict()
        ),
        (
            "BXC0027",
            "hello",
            Class::Incompatible,
            None,
            Class::Incompatible
        )
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

#[test]
fn single_referenced_error_variant_addition_is_conditional() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants.push(variant("Other"));
    submitted.revision = OTHER_REVISION.to_owned();

    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_conditional(&report, &["hello/type/GreetError/variant/Other"]);
}

#[test]
fn multiple_referenced_error_variant_additions_are_sorted() {
    let base = two_error_document();
    let mut submitted = base.clone();
    submitted.types[1].variants.push(variant("GreetOther"));
    submitted.types[0].variants.push(variant("WaveOther"));
    submitted.revision = OTHER_REVISION.to_owned();

    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_conditional(
        &report,
        &[
            "hello/type/GreetError/variant/GreetOther",
            "hello/type/WaveError/variant/WaveOther",
        ],
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

#[test]
fn variant_removed_is_incompatible() {
    let mut base = document("hello");
    base.types[0].variants.push(variant("Other"));
    let submitted = with_flipped_revision(document("hello"));
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[(
            "BXC0035",
            "hello/type/GreetError/variant/Other",
            Class::Incompatible,
            None,
        )],
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
            ("BXC0028", "hello", Class::Incompatible, None),
            (
                "BXC0033",
                "hello/type/GreetError",
                Class::Documentation,
                None,
            ),
        ],
        Class::Incompatible,
    );
}

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
        &[("BXC0052", "hello.greet/error", Class::Incompatible, None)],
        Class::Incompatible,
    );
}

#[test]
fn type_docs_changed_is_documentation() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].docs.push("Extra type docs.".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[(
            "BXC0033",
            "hello/type/GreetError",
            Class::Documentation,
            None,
        )],
        Class::Documentation,
    );
}

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
        &[(
            "BXC0033",
            "hello/type/GreetError/variant/EmptyName",
            Class::Documentation,
            None,
        )],
        Class::Documentation,
    );
}

#[test]
fn type_deprecation_changed_is_deprecation() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].deprecation = Some("use another error".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[("BXC0034", "hello/type/GreetError", Class::Deprecation, None)],
        Class::Deprecation,
    );
}

#[test]
fn variant_deprecation_changed_is_deprecation() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants[0].deprecation = Some("use another variant".to_owned());
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[(
            "BXC0034",
            "hello/type/GreetError/variant/EmptyName",
            Class::Deprecation,
            None,
        )],
        Class::Deprecation,
    );
}

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
            ("BXC0039", "hello.wave", Class::Additive, None),
            ("BXC0031", "hello/type/WaveError", Class::Additive, None),
        ],
        Class::Additive,
    );
}

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
            ("BXC0040", "hello.wave", Class::Incompatible, None),
            ("BXC0032", "hello/type/WaveError", Class::Incompatible, None),
        ],
        Class::Incompatible,
    );
}

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
        &[("BXC0039", "hello.wave", Class::Additive, None)],
        Class::Additive,
    );
}

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
        &[("BXC0040", "hello.wave", Class::Incompatible, None)],
        Class::Incompatible,
    );
}

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
            ("BXC0040", "hello.greet", Class::Incompatible, None),
            ("BXC0039", "hello.wave", Class::Additive, None),
        ],
        Class::Incompatible,
    );
}

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
        &[("BXC0041", "hello.greet/input", Class::Incompatible, None)],
        Class::Incompatible,
    );
}

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
        &[("BXC0042", "hello.greet/input", Class::Incompatible, None)],
        Class::Incompatible,
    );
}

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
        &[("BXC0043", "hello.greet/output", Class::Incompatible, None)],
        Class::Incompatible,
    );
}

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
            ("BXC0041", "hello.greet/input", Class::Incompatible, None),
            ("BXC0042", "hello.greet/input", Class::Incompatible, None),
        ],
        Class::Incompatible,
    );
}

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
            ("BXC0033", "hello.greet", Class::Documentation, None),
            ("BXC0034", "hello.greet", Class::Deprecation, None),
            ("BXC0044", "hello.greet/error", Class::Incompatible, None),
            ("BXC0046", "hello.greet/exposure", Class::Incompatible, None),
            ("BXC0047", "hello.greet/idempotency", Class::Additive, None),
        ],
        Class::Incompatible,
    );
}

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
            ("BXC0045", "hello.greet/exposure", Class::Additive, None),
            ("BXC0047", "hello.greet/idempotency", Class::Additive, None),
        ],
        Class::Additive,
    );
    low.revision = OTHER_REVISION.to_owned();
    high.revision = REVISION.to_owned();
    assert_exact_report(
        &classify(Some(&high), Some(&low)).unwrap(),
        &[
            ("BXC0046", "hello.greet/exposure", Class::Incompatible, None),
            (
                "BXC0048",
                "hello.greet/idempotency",
                Class::Incompatible,
                None,
            ),
        ],
        Class::Incompatible,
    );
}

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
            (
                "BXC0033",
                "hello/type/GreetError",
                Class::Documentation,
                None,
            ),
            (
                "BXC0036",
                "hello/type/GreetError/variant/Other",
                Class::CompatibleWithConditions,
                Some("unknown-variant tolerance"),
            ),
        ],
        Class::CompatibleWithConditions,
    );
}

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
            ("BXC0033", "hello.greet", Class::Documentation, None),
            (
                "BXC0036",
                "hello/type/GreetError/variant/Other",
                Class::CompatibleWithConditions,
                Some("unknown-variant tolerance"),
            ),
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
fn type_rename_is_remove_plus_unreferenced_add() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].name = "WaveError".to_owned();
    submitted.revision = OTHER_REVISION.to_owned();
    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[
            ("BXC0028", "hello", Class::Incompatible, None),
            (
                "BXC0032",
                "hello/type/GreetError",
                Class::Incompatible,
                None,
            ),
        ],
        Class::Incompatible,
    );
}

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
            (
                "BXC0035",
                "hello/type/GreetError/variant/EmptyName",
                Class::Incompatible,
                None,
            ),
            (
                "BXC0036",
                "hello/type/GreetError/variant/Renamed",
                Class::CompatibleWithConditions,
                Some("unknown-variant tolerance"),
            ),
        ],
        Class::Incompatible,
    );
}

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
            (
                "BXC0033",
                "hello/type/GreetError/variant/EmptyName/field/first",
                Class::Documentation,
                None,
            ),
            (
                "BXC0034",
                "hello/type/GreetError/variant/EmptyName/field/first",
                Class::Deprecation,
                None,
            ),
            (
                "BXC0051",
                "hello/type/GreetError/variant/EmptyName/field/first",
                Class::Incompatible,
                None,
            ),
            (
                "BXC0050",
                "hello/type/GreetError/variant/EmptyName/field/second",
                Class::Incompatible,
                None,
            ),
            (
                "BXC0049",
                "hello/type/GreetError/variant/EmptyName/field/third",
                Class::Additive,
                None,
            ),
        ],
        Class::Incompatible,
    );
}

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
            ("BXC0052", "hello.greet/error", Class::Incompatible, None),
            (
                "BXC0033",
                "hello/type/GreetError/variant/EmptyName",
                Class::Documentation,
                None,
            ),
            (
                "BXC0034",
                "hello/type/GreetError/variant/EmptyName",
                Class::Deprecation,
                None,
            ),
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
        class: Class::Additive,
        condition: None,
    };
    let high = Finding {
        code: "BXC0028",
        path: "hello".to_owned(),
        class: Class::Incompatible,
        condition: None,
    };
    assert_eq!(report(vec![low, high]).verdict, Class::Incompatible);

    let low = Finding {
        code: "BXC0026",
        path: "hello".to_owned(),
        class: Class::Additive,
        condition: None,
    };
    let high = Finding {
        code: "BXC0028",
        path: "hello".to_owned(),
        class: Class::Incompatible,
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
}

#[test]
fn report_renderings_are_byte_exact() {
    let introduced = classify(None, Some(&document("hello"))).unwrap();
    assert_renderings(
        &introduced,
        r#"classification additive
finding BXC0026 hello additive
"#,
        r#"{
  "schema": "boxology.classification-report@1",
  "verdict": "additive",
  "findings": [
    {
      "code": "BXC0026",
      "path": "hello",
      "class": "additive"
    }
  ]
}
"#,
    );

    let removed = classify(Some(&document("hello")), None).unwrap();
    assert_renderings(
        &removed,
        r#"classification incompatible
finding BXC0027 hello incompatible
"#,
        r#"{
  "schema": "boxology.classification-report@1",
  "verdict": "incompatible",
  "findings": [
    {
      "code": "BXC0027",
      "path": "hello",
      "class": "incompatible"
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
  "schema": "boxology.classification-report@1",
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
        r#"classification incompatible
finding BXC0041 hello.greet/input incompatible
"#,
        r#"{
  "schema": "boxology.classification-report@1",
  "verdict": "incompatible",
  "findings": [
    {
      "code": "BXC0041",
      "path": "hello.greet/input",
      "class": "incompatible"
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
        r#"classification compatible_with_conditions
finding BXC0036 hello/type/GreetError/variant/Other compatible_with_conditions condition="unknown-variant tolerance"
"#,
        r#"{
  "schema": "boxology.classification-report@1",
  "verdict": "compatible_with_conditions",
  "findings": [
    {
      "code": "BXC0036",
      "path": "hello/type/GreetError/variant/Other",
      "class": "compatible_with_conditions",
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
        r#"classification deprecation
finding BXC0033 hello/type/GreetError documentation
finding BXC0034 hello/type/GreetError deprecation
"#,
        r#"{
  "schema": "boxology.classification-report@1",
  "verdict": "deprecation",
  "findings": [
    {
      "code": "BXC0033",
      "path": "hello/type/GreetError",
      "class": "documentation"
    },
    {
      "code": "BXC0034",
      "path": "hello/type/GreetError",
      "class": "deprecation"
    }
  ]
}
"#,
    );

    let hostile = ClassificationReport {
        findings: vec![Finding {
            code: "BXC0028",
            path: "hello/\"quoted\\path".to_owned(),
            class: Class::Incompatible,
            condition: None,
        }],
        verdict: Class::Incompatible,
    };
    assert_renderings(
        &hostile,
        "classification incompatible\nfinding BXC0028 hello/\"quoted\\path incompatible\n",
        r#"{
  "schema": "boxology.classification-report@1",
  "verdict": "incompatible",
  "findings": [
    {
      "code": "BXC0028",
      "path": "hello/\"quoted\\path",
      "class": "incompatible"
    }
  ]
}
"#,
    );
}

#[test]
fn public_seam_is_send_sync_static() {
    fn bounds<T: Send + Sync + 'static>() {}
    bounds::<Class>();
    bounds::<Finding>();
    bounds::<ClassificationReport>();
}
