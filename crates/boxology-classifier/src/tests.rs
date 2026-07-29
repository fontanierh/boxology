use super::*;
use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use boxology_schema::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SchemaCapability, SchemaDocument, SchemaField,
    SchemaPayload, SchemaType, SchemaVariant, Shape,
};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

const REVISION: &str = "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";

fn metadata_target_is_live(metadata: &Value, manifest_dir: &Path) -> bool {
    let manifest = manifest_dir.join("Cargo.toml");
    let Some(package) = metadata["packages"].as_array().and_then(|packages| {
        packages.iter().find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| Path::new(path) == manifest)
        })
    }) else {
        return false;
    };
    let Some(targets) = package["targets"].as_array() else {
        return false;
    };
    let kind = |target: &Value, expected| target["kind"] == json!([expected]);
    let library = targets.iter().find(|target| kind(target, "lib"));
    let surface = targets
        .iter()
        .find(|target| kind(target, "test") && target["name"].as_str() == Some("surface_lock"));
    let Some((library, surface)) = library.zip(surface) else {
        return false;
    };
    targets.len() == 2
        && library["test"] == true
        && library["src_path"]
            .as_str()
            .is_some_and(|path| Path::new(path) == manifest_dir.join("src/lib.rs"))
        && surface["crate_types"] == json!(["bin"])
        && surface["src_path"]
            .as_str()
            .is_some_and(|path| Path::new(path) == manifest_dir.join("tests/surface_lock.rs"))
        && surface["test"] == true
        && surface
            .get("required-features")
            .is_none_or(|features| features == &json!([]))
}

fn cargo_metadata(manifest_dir: &Path) -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest_dir.join("Cargo.toml"))
        .output()
        .expect("cargo metadata can run");
    assert!(output.status.success(), "cargo metadata failed");
    serde_json::from_slice(&output.stdout).expect("cargo metadata is JSON")
}

fn surface_lock_lists_tests(manifest_dir: &Path) -> bool {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["test", "--locked", "--manifest-path"])
        .arg(manifest_dir.join("Cargo.toml"))
        .args(["--test", "surface_lock", "--", "--list"])
        .output();
    output.is_ok_and(|output| {
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .contains("production_inventory_and_code_anchors_are_fail_closed")
    })
}

#[test]
fn surface_lock_target_is_parsed_and_executable() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let metadata = cargo_metadata(manifest_dir);
    assert!(metadata_target_is_live(&metadata, manifest_dir));
    assert!(surface_lock_lists_tests(manifest_dir));
}

fn mutate_surface_target(metadata: &mut Value, mutate: fn(&mut Value)) {
    let target = metadata["packages"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|package| package["targets"].as_array_mut().into_iter().flatten())
        .find(|target| target["name"].as_str() == Some("surface_lock"))
        .unwrap();
    mutate(target);
}

#[test]
fn surface_lock_target_mutants_are_active_negative_controls() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let metadata = cargo_metadata(manifest_dir);
    let mut autotests_disabled = metadata.clone();
    for package in autotests_disabled["packages"].as_array_mut().unwrap() {
        if let Some(targets) = package["targets"].as_array_mut() {
            targets.retain(|target| target["name"].as_str() != Some("surface_lock"));
        }
    }
    assert!(!metadata_target_is_live(&autotests_disabled, manifest_dir));
    type TargetMutant = (&'static str, fn(&mut Value));
    let mutants: &[TargetMutant] = &[
        ("required feature", |target| {
            target["required-features"] = json!(["hidden"])
        }),
        ("alternate path", |target| {
            target["src_path"] = json!("tests/other.rs")
        }),
        ("disabled test", |target| target["test"] = json!(false)),
    ];
    for (name, mutate) in mutants {
        let mut mutant = metadata.clone();
        mutate_surface_target(&mut mutant, *mutate);
        assert!(!metadata_target_is_live(&mutant, manifest_dir), "{name}");
    }
}

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

fn named_fields(document: &mut SchemaDocument) -> &mut [SchemaField] {
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
                "BXC0029",
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
fn single_referenced_error_variant_addition_is_conditional() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants.push(variant("Other"));
    submitted.revision.push('x');

    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_conditional(&report, &["hello/type/GreetError/variant/Other"]);
}

#[test]
fn multiple_referenced_error_variant_additions_are_sorted() {
    let base = two_error_document();
    let mut submitted = base.clone();
    submitted.types[1].variants.push(variant("GreetOther"));
    submitted.types[0].variants.push(variant("WaveOther"));
    submitted.revision.push('x');

    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_conditional(
        &report,
        &[
            "hello/type/GreetError/variant/GreetOther",
            "hello/type/WaveError/variant/WaveOther",
        ],
    );
}

fn assert_variant_incompatible(base: SchemaDocument, mutate: impl FnOnce(&mut SchemaDocument)) {
    let mut submitted = base.clone();
    mutate(&mut submitted);
    submitted.revision.push('x');
    assert_unclassified_pair(base, submitted);
}

#[test]
fn variant_changes_outside_named_addition_fail_closed() {
    let mut base = document("hello");
    base.types[0].variants.push(variant("Other"));
    assert_variant_incompatible(base, |document| {
        document.types[0].variants.swap(0, 1);
    });

    let mutations: &[fn(&mut SchemaDocument)] = &[
        |document| {
            document.types[0].variants.pop();
        },
        |document| document.types[0].variants[0].name = "Other".to_owned(),
        |document| {
            document.types[0].variants.push(variant("Other"));
            document.capabilities[0].docs.push("More docs.".to_owned());
        },
        |document| {
            document.types[0].variants.push(variant("Other"));
            document.types[0]
                .docs
                .push("Different type docs.".to_owned());
        },
        |document| {
            document.types[0].variants[0].name = "Renamed".to_owned();
            document.types[0].variants.push(variant("Other"));
        },
    ];
    for mutate in mutations {
        assert_variant_incompatible(document("hello"), *mutate);
    }

    assert_variant_incompatible(two_error_document(), |document| {
        document.types[1].variants.push(variant("Other"));
        document.types[0].name = "RenamedError".to_owned();
    });
    let mut base = document("hello");
    let mut unreferenced = base.types[0].clone();
    unreferenced.name = "UnusedError".to_owned();
    base.types.push(unreferenced);
    assert_variant_incompatible(base, |document| {
        document.types[1].variants.push(variant("Other"));
    });
}

#[test]
fn equal_revision_variant_addition_is_incompatible() {
    let base = document("hello");
    let mut submitted = base.clone();
    submitted.types[0].variants.push(variant("Other"));

    let report = classify(Some(&base), Some(&submitted)).unwrap();
    assert_exact_report(
        &report,
        &[("BXC0028", "hello", Class::Incompatible, None)],
        Class::Incompatible,
    );
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
            document.types[0].variants[0].payload = SchemaPayload::Value {
                docs: Vec::new(),
                deprecation: None,
                ty: BoundaryLeaf::String,
            }
        },
        |document| document.types[0].variants[0].payload = SchemaPayload::Named(Vec::new()),
    ];
    for mutate in mutations {
        let mut submitted = document("hello");
        mutate(&mut submitted);
        assert_unclassified_pair(document("hello"), submitted);
    }
    // Shape has only `Unary` in the current format-1 vocabulary, so it has no effective mutation.
}

#[test]
fn named_payload_fields_fail_closed() {
    let mutations: &[fn(&mut SchemaDocument)] = &[
        |document| named_fields(document)[0].docs.push("new docs".to_owned()),
        |document| named_fields(document)[0].deprecation = Some("retired".to_owned()),
        |document| named_fields(document)[0].name = "renamed".to_owned(),
        |document| named_fields(document)[0].ty = BoundaryLeaf::Bool,
        |document| named_fields(document).swap(0, 1),
    ];
    for mutate in mutations {
        let base = named_document();
        let mut submitted = base.clone();
        mutate(&mut submitted);
        assert_unclassified_pair(base, submitted);
    }
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

#[test]
fn public_seam_is_send_sync_static() {
    fn bounds<T: Send + Sync + 'static>() {}
    bounds::<Class>();
    bounds::<Finding>();
    bounds::<ClassificationReport>();
}
