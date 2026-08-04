use boxology_classifier::{ClassificationReport, classify, render_json, render_text};
use boxology_contract::{CapabilityName, ExposureLevel, Idempotency};
use boxology_schema::{BoundaryLeaf, SchemaDocument, SchemaField, SchemaPayload, SchemaVariant};
use serde_json::json;
use std::{fs, path::Path};

const HELLO_SCHEMA: &[u8] = include_bytes!("../../fixtures/hello/generated/schema.json");
const PING_SCHEMA: &[u8] = include_bytes!("../../fixtures/ping/generated/schema.json");

fn hello() -> Result<SchemaDocument, String> {
    SchemaDocument::parse(HELLO_SCHEMA).map_err(|error| format!("hello fixture: {error}"))
}

fn ping() -> Result<SchemaDocument, String> {
    SchemaDocument::parse(PING_SCHEMA).map_err(|error| format!("ping fixture: {error}"))
}

fn provenance_only() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    submitted.provenance = boxology_schema::Provenance::new(json!("determinism-subject"));
    Ok((base, submitted))
}

fn renamed_input() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    let capability = submitted
        .capabilities
        .get_mut(0)
        .ok_or("hello fixture has no capability")?;
    capability.input.name = String::from("label");
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn capability_only() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    let capability = submitted
        .capabilities
        .get_mut(0)
        .ok_or("hello fixture has no capability")?;
    capability.input.name = String::from("label");
    Ok((base, submitted))
}

fn types_only() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    let schema_type = submitted
        .types
        .get_mut(0)
        .ok_or("hello fixture has no type")?;
    schema_type.docs.push(String::from("types-only subject"));
    Ok((base, submitted))
}

fn revision_only() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn flip_revision(document: &mut SchemaDocument) -> Result<(), String> {
    let revision = document.revision.clone();
    let last = revision.chars().last().ok_or("revision is empty")?;
    let flipped = match last {
        '0' => '1',
        _ => '0',
    };
    document.revision = format!("{}{flipped}", &revision[..revision.len() - 1]);
    Ok(())
}

fn unit_variant(name: &str) -> SchemaVariant {
    SchemaVariant {
        name: String::from(name),
        docs: Vec::new(),
        deprecation: None,
        payload: SchemaPayload::Unit,
    }
}

fn wave_name() -> Result<CapabilityName, String> {
    CapabilityName::new("wave").map_err(|error| error.to_string())
}

fn add_wave_capability(document: &mut SchemaDocument) -> Result<(), String> {
    let mut capability = document.capabilities[0].clone();
    capability.name = wave_name()?;
    document.capabilities.push(capability);
    Ok(())
}

fn named_fields(document: &mut SchemaDocument) -> Result<&mut Vec<SchemaField>, String> {
    match &mut document.types[0].variants[0].payload {
        SchemaPayload::Named(fields) => Ok(fields),
        _ => Err(String::from("expected named payload")),
    }
}

fn with_named_payload(mut document: SchemaDocument) -> SchemaDocument {
    document.types[0].variants[0].payload = SchemaPayload::Named(vec![
        SchemaField {
            docs: Vec::new(),
            deprecation: None,
            name: String::from("first"),
            ty: BoundaryLeaf::String,
        },
        SchemaField {
            docs: Vec::new(),
            deprecation: None,
            name: String::from("second"),
            ty: BoundaryLeaf::I64,
        },
    ]);
    document
}

fn two_error_document() -> Result<SchemaDocument, String> {
    let mut document = hello()?;
    let mut capability = document.capabilities[0].clone();
    capability.name = wave_name()?;
    capability.error = String::from("WaveError");
    document.capabilities.push(capability);
    let greet_type = document.types[0].clone();
    let mut wave_type = greet_type.clone();
    wave_type.name = String::from("WaveError");
    document.types = vec![wave_type, greet_type];
    Ok(document)
}

fn variant_added() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    submitted.types[0].variants.push(unit_variant("Other"));
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn capability_added_with_type() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    add_wave_capability(&mut submitted)?;
    submitted.capabilities[1].error = String::from("WaveError");
    let mut wave = submitted.types[0].clone();
    wave.name = String::from("WaveError");
    submitted.types.push(wave);
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn capability_renamed() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    submitted.capabilities[0].name = wave_name()?;
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn capability_removed_with_type() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = two_error_document()?;
    let mut submitted = hello()?;
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn boundary_changed() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    let capability = &mut submitted.capabilities[0];
    capability.input.leaf = BoundaryLeaf::Bool;
    capability.output.leaf = BoundaryLeaf::Bool;
    capability.error = String::from("WaveError");
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn metadata_raised() -> Result<(SchemaDocument, SchemaDocument), String> {
    let mut base = hello()?;
    base.capabilities[0].max_exposure = ExposureLevel::CodeOnly;
    let mut submitted = base.clone();
    submitted.capabilities[0].max_exposure = ExposureLevel::External;
    submitted.capabilities[0].idempotency = Idempotency::Inherent;
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn metadata_lowered() -> Result<(SchemaDocument, SchemaDocument), String> {
    let mut low = hello()?;
    low.capabilities[0].max_exposure = ExposureLevel::CodeOnly;
    let mut high = low.clone();
    high.capabilities[0].max_exposure = ExposureLevel::External;
    high.capabilities[0].idempotency = Idempotency::Inherent;
    flip_revision(&mut low)?;
    Ok((high, low))
}

fn fields_changed() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = with_named_payload(hello()?);
    let mut submitted = base.clone();
    let fields = named_fields(&mut submitted)?;
    fields[0].ty = BoundaryLeaf::Bool;
    fields.remove(1);
    fields.push(SchemaField {
        docs: Vec::new(),
        deprecation: None,
        name: String::from("third"),
        ty: BoundaryLeaf::String,
    });
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn payload_kind_changed() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    submitted.types[0].variants[0].payload = SchemaPayload::Value {
        docs: Vec::new(),
        deprecation: None,
        ty: BoundaryLeaf::String,
    };
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn variant_removed() -> Result<(SchemaDocument, SchemaDocument), String> {
    let mut base = hello()?;
    base.types[0].variants.push(unit_variant("Other"));
    let mut submitted = hello()?;
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn docs_only() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    submitted.types[0]
        .docs
        .push(String::from("Extra type docs."));
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn deprecation_only() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    submitted.types[0].deprecation = Some(String::from("use another error"));
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn unclassified() -> Result<(SchemaDocument, SchemaDocument), String> {
    let mut base = hello()?;
    base.types[0].variants.push(unit_variant("Other"));
    let mut submitted = base.clone();
    submitted.types[0].variants.swap(0, 1);
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn report_pair(
    pair: Result<(SchemaDocument, SchemaDocument), String>,
) -> Result<ClassificationReport, String> {
    let (base, submitted) = pair?;
    classify(Some(&base), Some(&submitted)).map_err(|error| error.to_string())
}

fn report_introduced() -> Result<ClassificationReport, String> {
    classify(None, Some(&hello()?)).map_err(|error| error.to_string())
}

fn report_removed() -> Result<ClassificationReport, String> {
    classify(Some(&hello()?), None).map_err(|error| error.to_string())
}

fn report_unchanged() -> Result<ClassificationReport, String> {
    report_pair(provenance_only())
}

fn report_changed() -> Result<ClassificationReport, String> {
    report_pair(renamed_input())
}

fn report_capability_only() -> Result<String, String> {
    let (base, submitted) = capability_only()?;
    let error = classify(Some(&base), Some(&submitted))
        .err()
        .ok_or("capability-only pair classified")?;
    Ok(format!("{error}\n"))
}

fn report_types_only() -> Result<String, String> {
    let (base, submitted) = types_only()?;
    let error = classify(Some(&base), Some(&submitted))
        .err()
        .ok_or("types-only pair classified")?;
    Ok(format!("{error}\n"))
}

fn report_revision_only() -> Result<String, String> {
    let (base, submitted) = revision_only()?;
    let error = classify(Some(&base), Some(&submitted))
        .err()
        .ok_or("revision-only pair classified")?;
    Ok(format!("{error}\n"))
}

fn report_variant_added() -> Result<ClassificationReport, String> {
    report_pair(variant_added())
}
fn report_capability_added_with_type() -> Result<ClassificationReport, String> {
    report_pair(capability_added_with_type())
}
fn report_capability_renamed() -> Result<ClassificationReport, String> {
    report_pair(capability_renamed())
}
fn report_capability_removed_with_type() -> Result<ClassificationReport, String> {
    report_pair(capability_removed_with_type())
}
fn report_boundary_changed() -> Result<ClassificationReport, String> {
    report_pair(boundary_changed())
}
fn report_metadata_raised() -> Result<ClassificationReport, String> {
    report_pair(metadata_raised())
}
fn report_metadata_lowered() -> Result<ClassificationReport, String> {
    report_pair(metadata_lowered())
}
fn report_fields_changed() -> Result<ClassificationReport, String> {
    report_pair(fields_changed())
}
fn report_payload_kind_changed() -> Result<ClassificationReport, String> {
    report_pair(payload_kind_changed())
}
fn report_variant_removed() -> Result<ClassificationReport, String> {
    report_pair(variant_removed())
}
fn report_docs_only() -> Result<ClassificationReport, String> {
    report_pair(docs_only())
}
fn report_deprecation_only() -> Result<ClassificationReport, String> {
    report_pair(deprecation_only())
}
fn report_unclassified() -> Result<ClassificationReport, String> {
    report_pair(unclassified())
}

fn both_absent_error() -> Result<String, String> {
    let error = classify(None, None)
        .err()
        .ok_or("both-absent pair classified")?;
    Ok(format!("{error}\n"))
}

fn pairing_error() -> Result<String, String> {
    let error = classify(Some(&hello()?), Some(&ping()?))
        .err()
        .ok_or("hello/ping pair classified")?;
    Ok(format!("{error}\n"))
}

pub(crate) fn run(out: &Path) -> Result<(), String> {
    let reports = [
        ("report-introduced", report_introduced()?),
        ("report-removed", report_removed()?),
        ("report-unchanged", report_unchanged()?),
        ("report-changed", report_changed()?),
        ("report-variant-added", report_variant_added()?),
        (
            "capability-added-with-type",
            report_capability_added_with_type()?,
        ),
        ("capability-renamed", report_capability_renamed()?),
        (
            "capability-removed-with-type",
            report_capability_removed_with_type()?,
        ),
        ("boundary-changed", report_boundary_changed()?),
        ("metadata-raised", report_metadata_raised()?),
        ("metadata-lowered", report_metadata_lowered()?),
        ("fields-changed", report_fields_changed()?),
        ("payload-kind-changed", report_payload_kind_changed()?),
        ("variant-removed", report_variant_removed()?),
        ("docs-only", report_docs_only()?),
        ("deprecation-only", report_deprecation_only()?),
        ("unclassified", report_unclassified()?),
    ];
    for (name, report) in reports {
        fs::write(out.join(format!("{name}.txt")), render_text(&report))
            .map_err(|error| format!("write {name}.txt: {error}"))?;
        fs::write(out.join(format!("{name}.json")), render_json(&report))
            .map_err(|error| format!("write {name}.json: {error}"))?;
    }
    for (name, body) in [
        ("pairing-both-absent.txt", both_absent_error()?),
        ("report-capability-only.txt", report_capability_only()?),
        ("report-types-only.txt", report_types_only()?),
        ("report-revision-only.txt", report_revision_only()?),
        ("pairing-error.txt", pairing_error()?),
    ] {
        fs::write(out.join(name), body).map_err(|error| format!("write {name}: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_SUBJECT: AtomicU64 = AtomicU64::new(0);

    struct SubjectTemp(PathBuf);
    impl Drop for SubjectTemp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn subject_root(name: &str) -> SubjectTemp {
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/xtask-subject-tests");
        fs::create_dir_all(&parent).unwrap();
        let path = loop {
            let candidate = parent.join(format!(
                "{name}-{}-{}",
                std::process::id(),
                NEXT_SUBJECT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create subject root: {error}"),
            }
        };
        SubjectTemp(path)
    }

    fn assert_report_golden(
        report: ClassificationReport,
        again: ClassificationReport,
        text: &str,
        json: &str,
    ) {
        assert_eq!(render_text(&report), render_text(&again));
        assert_eq!(render_json(&report), render_json(&again));
        assert_eq!(render_text(&report), text);
        assert_eq!(render_json(&report), json);
    }
    #[test]
    fn subject_report_introduced_is_golden_and_repeatable() {
        assert_report_golden(
            report_introduced().expect("introduced pair renders"),
            report_introduced().expect("it renders again"),
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
    }

    #[test]
    fn subject_report_removed_is_golden_and_repeatable() {
        assert_report_golden(
            report_removed().expect("removed pair renders"),
            report_removed().expect("it renders again"),
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
    }

    #[test]
    fn subject_report_unchanged_is_golden_and_repeatable() {
        let (base, submitted) = provenance_only().expect("provenance pair builds");
        assert_ne!(base, submitted);
        assert_report_golden(
            report_unchanged().expect("unchanged pair renders"),
            report_unchanged().expect("it renders again"),
            "classification unchanged\n",
            r#"{
  "schema": "boxology.classification-report@2",
  "verdict": "unchanged",
  "findings": []
}
"#,
        );
    }

    #[test]
    fn subject_report_changed_is_golden_and_repeatable() {
        let (base, submitted) = renamed_input().expect("changed pair builds");
        assert_ne!(
            base.capabilities[0].input.name,
            submitted.capabilities[0].input.name
        );
        assert_eq!(base.capabilities[0].input.name, "name");
        assert_eq!(submitted.capabilities[0].input.name, "label");
        assert_ne!(base.revision, submitted.revision);
        assert_eq!(submitted.revision.len(), 71);
        assert!(submitted.revision.starts_with("sha256:"));
        assert_report_golden(
            report_changed().expect("changed pair renders"),
            report_changed().expect("it renders again"),
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
    }

    #[test]
    fn subject_report_capability_only_is_golden_and_repeatable() {
        let (base, submitted) = capability_only().expect("capability-only pair builds");
        assert_eq!(base.revision, submitted.revision);
        assert_ne!(
            base.capabilities[0].input.name,
            submitted.capabilities[0].input.name
        );
        let rendered = report_capability_only().expect("capability-only pair renders");
        assert_eq!(
            rendered,
            report_capability_only().expect("it renders again")
        );
        assert_eq!(
            rendered,
            "BXC0037 at=\"\" rule=\"findings under equal revisions mean the projection and the \
classifier disagree\" source=\"specs/s4-contract-change-classification.md D6\"\n"
        );
    }

    #[test]
    fn subject_report_types_only_is_golden_and_repeatable() {
        let (base, submitted) = types_only().expect("types-only pair builds");
        assert_eq!(base.revision, submitted.revision);
        assert_eq!(base.capabilities, submitted.capabilities);
        assert_eq!(base.provenance, submitted.provenance);
        assert_ne!(base.types, submitted.types);
        let rendered = report_types_only().expect("types-only pair renders");
        assert_eq!(rendered, report_types_only().expect("it renders again"));
        assert_eq!(
            rendered,
            "BXC0037 at=\"\" rule=\"findings under equal revisions mean the projection and the \
classifier disagree\" source=\"specs/s4-contract-change-classification.md D6\"\n"
        );
    }

    #[test]
    fn subject_report_revision_only_is_golden_and_repeatable() {
        let (base, submitted) = revision_only().expect("revision-only pair builds");
        assert_ne!(base.revision, submitted.revision);
        assert_eq!(base.box_id, submitted.box_id);
        assert_eq!(base.capabilities, submitted.capabilities);
        assert_eq!(base.types, submitted.types);
        assert_eq!(base.provenance, submitted.provenance);
        assert_eq!(submitted.revision.len(), 71);
        assert!(submitted.revision.starts_with("sha256:"));
        assert_eq!(&submitted.revision[..70], &base.revision[..70]);
        let rendered = report_revision_only().expect("revision-only pair renders");
        assert_eq!(rendered, report_revision_only().expect("it renders again"));
        assert_eq!(
            rendered,
            "BXC0038 at=\"\" rule=\"differing revisions with no finding mean the projection and the \
classifier disagree\" source=\"specs/s4-contract-change-classification.md D6\"\n"
        );
    }

    #[test]
    fn subject_report_variant_added_is_golden_and_repeatable() {
        let (base, submitted) = variant_added().expect("variant-added pair builds");
        assert_ne!(base.revision, submitted.revision);
        assert_ne!(base.types, submitted.types);
        assert_report_golden(
            report_variant_added().expect("variant-added pair renders"),
            report_variant_added().expect("it renders again"),
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
    }

    #[test]
    fn subject_pairing_error_is_golden_and_repeatable() {
        let rendered = pairing_error().expect("pairing error renders");
        assert_eq!(rendered, pairing_error().expect("it renders again"));
        assert_eq!(
            rendered,
            "BXC0025 at=\"/box_id\" rule=\"base and submitted must declare the same box id\" \
             source=\"specs/s4-contract-change-classification.md D2\"\n"
        );
    }

    #[test]
    fn subject_both_absent_pairing_error_is_golden_and_repeatable() {
        let rendered = both_absent_error().expect("both-absent error renders");
        assert_eq!(rendered, both_absent_error().expect("it renders again"));
        assert_eq!(
            rendered,
            "BXC0024 at=\"\" rule=\"classification requires a base or a submitted document\" \
             source=\"specs/s4-contract-change-classification.md D2\"\n"
        );
    }

    #[rustfmt::skip]
    mod golden_closure {
        use super::*;

        fn json_report(verdict: &str, findings: &[&str]) -> String {
            if findings.is_empty() {
                format!("{{\n  \"schema\": \"boxology.classification-report@2\",\n  \"verdict\": \"{verdict}\",\n  \"findings\": []\n}}\n")
            } else {
                format!("{{\n  \"schema\": \"boxology.classification-report@2\",\n  \"verdict\": \"{verdict}\",\n  \"findings\": [\n{}\n  ]\n}}\n", findings.join(",\n"))
            }
        }

        fn jf(code: &str, path: &str, kind: &str, class: &str, base: &str, submitted: &str) -> String {
            format!("    {{\n      \"code\": \"{code}\",\n      \"path\": \"{path}\",\n      \"kind\": \"{kind}\",\n      \"class\": \"{class}\",\n      \"base\": {base},\n      \"submitted\": {submitted}\n    }}")
        }

        macro_rules! golden_pair {
            ($test:ident, $pair:ident, $report:ident, $text:expr, $verdict:expr, [$($f:expr),+ $(,)?], |$base:ident, $submitted:ident| $body:block) => {
                #[test]
                fn $test() {
                    let ($base, $submitted) = $pair().expect("pair builds");
                    assert_ne!($base.revision, $submitted.revision);
                    $body
                    let findings = [$($f),+];
                    let findings_ref: Vec<&str> = findings.iter().map(String::as_str).collect();
                    assert_report_golden(
                        $report().expect("renders"),
                        $report().expect("again"),
                        $text,
                        &json_report($verdict, &findings_ref),
                    );
                }
            };
        }

        golden_pair!(
            subject_report_capability_added_with_type_is_golden_and_repeatable, capability_added_with_type, report_capability_added_with_type,
            "classification additive\nfinding BXC0039 path=\"hello.wave\" additive kind=\"capability added\" base=- submitted=\"wave\"\nfinding BXC0031 path=\"hello/type/WaveError\" additive kind=\"type added\" base=- submitted=\"WaveError\"\n",
            "additive",
            [jf("BXC0039", "hello.wave", "capability added", "additive", "null", "\"wave\""), jf("BXC0031", "hello/type/WaveError", "type added", "additive", "null", "\"WaveError\"")],
            |base, submitted| {
                assert_ne!(base.capabilities, submitted.capabilities);
                assert_ne!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.provenance, submitted.provenance);
                assert_eq!(submitted.capabilities[1].name.as_str(), "wave");
                assert_eq!(submitted.types[1].name, "WaveError");
            }
        );

        golden_pair!(
            subject_report_capability_renamed_is_golden_and_repeatable, capability_renamed, report_capability_renamed,
            "classification incompatible\nfinding BXC0040 path=\"hello.greet\" incompatible kind=\"capability removed\" base=\"greet\" submitted=-\nfinding BXC0039 path=\"hello.wave\" additive kind=\"capability added\" base=- submitted=\"wave\"\n",
            "incompatible",
            [jf("BXC0040", "hello.greet", "capability removed", "incompatible", "\"greet\"", "null"), jf("BXC0039", "hello.wave", "capability added", "additive", "null", "\"wave\"")],
            |base, submitted| {
                assert_ne!(base.capabilities, submitted.capabilities);
                assert_eq!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.capabilities[0].name.as_str(), "greet");
                assert_eq!(submitted.capabilities[0].name.as_str(), "wave");
            }
        );

        golden_pair!(
            subject_report_capability_removed_with_type_is_golden_and_repeatable, capability_removed_with_type, report_capability_removed_with_type,
            "classification incompatible\nfinding BXC0040 path=\"hello.wave\" incompatible kind=\"capability removed\" base=\"wave\" submitted=-\nfinding BXC0032 path=\"hello/type/WaveError\" incompatible kind=\"type removed\" base=\"WaveError\" submitted=-\n",
            "incompatible",
            [jf("BXC0040", "hello.wave", "capability removed", "incompatible", "\"wave\"", "null"), jf("BXC0032", "hello/type/WaveError", "type removed", "incompatible", "\"WaveError\"", "null")],
            |base, submitted| {
                assert_ne!(base.capabilities, submitted.capabilities);
                assert_ne!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!((base.capabilities.len(), submitted.capabilities.len()), (2, 1));
                assert_eq!(base.types[0].name, "WaveError");
            }
        );

        golden_pair!(
            subject_report_boundary_changed_is_golden_and_repeatable, boundary_changed, report_boundary_changed,
            "classification incompatible\nfinding BXC0044 path=\"hello.greet/error\" incompatible kind=\"capability declared error changed\" base=\"GreetError\" submitted=\"WaveError\"\nfinding BXC0042 path=\"hello.greet/input\" incompatible kind=\"capability input type changed\" base=\"String\" submitted=\"bool\"\nfinding BXC0043 path=\"hello.greet/output\" incompatible kind=\"capability output type changed\" base=\"String\" submitted=\"bool\"\n",
            "incompatible",
            [jf("BXC0044", "hello.greet/error", "capability declared error changed", "incompatible", "\"GreetError\"", "\"WaveError\""), jf("BXC0042", "hello.greet/input", "capability input type changed", "incompatible", "\"String\"", "\"bool\""), jf("BXC0043", "hello.greet/output", "capability output type changed", "incompatible", "\"String\"", "\"bool\"")],
            |base, submitted| {
                assert_ne!(base.capabilities, submitted.capabilities);
                assert_eq!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.capabilities[0].input.leaf, BoundaryLeaf::String);
                assert_eq!(submitted.capabilities[0].input.leaf, BoundaryLeaf::Bool);
                assert_eq!(submitted.capabilities[0].output.leaf, BoundaryLeaf::Bool);
                assert_eq!(submitted.capabilities[0].error, "WaveError");
            }
        );

        golden_pair!(
            subject_report_metadata_raised_is_golden_and_repeatable, metadata_raised, report_metadata_raised,
            "classification additive\nfinding BXC0045 path=\"hello.greet/exposure\" additive kind=\"max exposure raised\" base=\"code_only\" submitted=\"external\"\nfinding BXC0047 path=\"hello.greet/idempotency\" additive kind=\"idempotency strengthened\" base=\"none\" submitted=\"inherent\"\n",
            "additive",
            [jf("BXC0045", "hello.greet/exposure", "max exposure raised", "additive", "\"code_only\"", "\"external\""), jf("BXC0047", "hello.greet/idempotency", "idempotency strengthened", "additive", "\"none\"", "\"inherent\"")],
            |base, submitted| {
                assert_ne!(base.capabilities, submitted.capabilities);
                assert_eq!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.capabilities[0].max_exposure, ExposureLevel::CodeOnly);
                assert_eq!(submitted.capabilities[0].max_exposure, ExposureLevel::External);
                assert_eq!(base.capabilities[0].idempotency, Idempotency::None);
                assert_eq!(submitted.capabilities[0].idempotency, Idempotency::Inherent);
            }
        );

        golden_pair!(
            subject_report_metadata_lowered_is_golden_and_repeatable, metadata_lowered, report_metadata_lowered,
            "classification incompatible\nfinding BXC0046 path=\"hello.greet/exposure\" incompatible kind=\"max exposure lowered\" base=\"external\" submitted=\"code_only\"\nfinding BXC0048 path=\"hello.greet/idempotency\" incompatible kind=\"idempotency weakened\" base=\"inherent\" submitted=\"none\"\n",
            "incompatible",
            [jf("BXC0046", "hello.greet/exposure", "max exposure lowered", "incompatible", "\"external\"", "\"code_only\""), jf("BXC0048", "hello.greet/idempotency", "idempotency weakened", "incompatible", "\"inherent\"", "\"none\"")],
            |base, submitted| {
                assert_ne!(base.capabilities, submitted.capabilities);
                assert_eq!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.capabilities[0].max_exposure, ExposureLevel::External);
                assert_eq!(submitted.capabilities[0].max_exposure, ExposureLevel::CodeOnly);
                assert_eq!(base.capabilities[0].idempotency, Idempotency::Inherent);
                assert_eq!(submitted.capabilities[0].idempotency, Idempotency::None);
            }
        );

        #[test]
        fn subject_report_fields_changed_is_golden_and_repeatable() {
            let (base, submitted) = fields_changed().expect("pair builds");
            assert_ne!(base.revision, submitted.revision);
            assert_eq!(base.capabilities, submitted.capabilities);
            assert_ne!(base.types, submitted.types);
            assert_eq!(base.box_id, submitted.box_id);
            let p = "hello/type/GreetError/variant/EmptyName/field";
            let findings = [
                jf("BXC0051", &format!("{p}/first"), "field type changed", "incompatible", "\"String\"", "\"bool\""),
                jf("BXC0050", &format!("{p}/second"), "field removed", "incompatible", "\"second\"", "null"),
                jf("BXC0049", &format!("{p}/third"), "field added", "additive", "null", "\"third\""),
            ];
            let findings_ref: Vec<&str> = findings.iter().map(String::as_str).collect();
            assert_report_golden(
                report_fields_changed().expect("renders"),
                report_fields_changed().expect("again"),
                &format!("classification incompatible\nfinding BXC0051 path=\"{p}/first\" incompatible kind=\"field type changed\" base=\"String\" submitted=\"bool\"\nfinding BXC0050 path=\"{p}/second\" incompatible kind=\"field removed\" base=\"second\" submitted=-\nfinding BXC0049 path=\"{p}/third\" additive kind=\"field added\" base=- submitted=\"third\"\n"),
                &json_report("incompatible", &findings_ref),
            );
        }

        golden_pair!(
            subject_report_payload_kind_changed_is_golden_and_repeatable, payload_kind_changed, report_payload_kind_changed,
            "classification incompatible\nfinding BXC0052 path=\"hello.greet/error\" incompatible kind=\"error payload changed\" base=\"unit\" submitted=\"value\"\n",
            "incompatible",
            [jf("BXC0052", "hello.greet/error", "error payload changed", "incompatible", "\"unit\"", "\"value\"")],
            |base, submitted| {
                assert_eq!(base.capabilities, submitted.capabilities);
                assert_ne!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.types[0].variants[0].payload, SchemaPayload::Unit);
                assert!(matches!(submitted.types[0].variants[0].payload, SchemaPayload::Value { .. }));
            }
        );

        golden_pair!(
            subject_report_variant_removed_is_golden_and_repeatable, variant_removed, report_variant_removed,
            "classification incompatible\nfinding BXC0035 path=\"hello/type/GreetError/variant/Other\" incompatible kind=\"variant removed\" base=\"Other\" submitted=-\n",
            "incompatible",
            [jf("BXC0035", "hello/type/GreetError/variant/Other", "variant removed", "incompatible", "\"Other\"", "null")],
            |base, submitted| {
                assert_ne!(base.types, submitted.types);
                assert_eq!(base.capabilities, submitted.capabilities);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.types[0].variants.len(), 2);
                assert_eq!(submitted.types[0].variants.len(), 1);
            }
        );

        golden_pair!(
            subject_report_docs_only_is_golden_and_repeatable, docs_only, report_docs_only,
            "classification documentation\nfinding BXC0033 path=\"hello/type/GreetError\" documentation kind=\"documentation changed\" base=\"\" submitted=\"Extra type docs.\"\n",
            "documentation",
            [jf("BXC0033", "hello/type/GreetError", "documentation changed", "documentation", "\"\"", "\"Extra type docs.\"")],
            |base, submitted| {
                assert_eq!(base.capabilities, submitted.capabilities);
                assert_ne!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.provenance, submitted.provenance);
                assert!(base.types[0].docs.is_empty());
                assert_eq!(submitted.types[0].docs, ["Extra type docs."]);
            }
        );

        golden_pair!(
            subject_report_deprecation_only_is_golden_and_repeatable, deprecation_only, report_deprecation_only,
            "classification deprecation\nfinding BXC0034 path=\"hello/type/GreetError\" deprecation kind=\"deprecation changed\" base=- submitted=\"use another error\"\n",
            "deprecation",
            [jf("BXC0034", "hello/type/GreetError", "deprecation changed", "deprecation", "null", "\"use another error\"")],
            |base, submitted| {
                assert_eq!(base.capabilities, submitted.capabilities);
                assert_ne!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.types[0].deprecation, None);
                assert_eq!(submitted.types[0].deprecation.as_deref(), Some("use another error"));
            }
        );

        golden_pair!(
            subject_report_unclassified_is_golden_and_repeatable, unclassified, report_unclassified,
            "classification incompatible\nfinding BXC0028 path=\"hello\" incompatible kind=\"unclassified change\" base=- submitted=-\n",
            "incompatible",
            [jf("BXC0028", "hello", "unclassified change", "incompatible", "null", "null")],
            |base, submitted| {
                assert_eq!(base.capabilities, submitted.capabilities);
                assert_ne!(base.types, submitted.types);
                assert_eq!(base.box_id, submitted.box_id);
                assert_eq!(base.types[0].variants[0].name, "EmptyName");
                assert_eq!(submitted.types[0].variants[0].name, "Other");
            }
        );

        #[test]
        fn subject_run_emits_exact_sorted_filename_inventory() {
            let root = subject_root("classifier-report-inventory");
            run(&root.0).expect("subject run emits");
            let names: BTreeSet<_> = fs::read_dir(&root.0)
                .expect("read subject out")
                .map(|entry| entry.expect("dirent").file_name().into_string().expect("utf8"))
                .collect();
            let mut expected = BTreeSet::new();
            for name in [
                "boundary-changed", "capability-added-with-type", "capability-removed-with-type",
                "capability-renamed", "deprecation-only", "docs-only", "fields-changed",
                "metadata-lowered", "metadata-raised", "payload-kind-changed", "report-changed",
                "report-introduced", "report-removed", "report-unchanged", "report-variant-added",
                "unclassified", "variant-removed",
            ] {
                expected.insert(format!("{name}.json"));
                expected.insert(format!("{name}.txt"));
            }
            for name in ["pairing-both-absent.txt", "pairing-error.txt", "report-capability-only.txt", "report-revision-only.txt", "report-types-only.txt"] {
                expected.insert(String::from(name));
            }
            assert_eq!(names, expected);
            assert_eq!(names.len(), 39);
        }
    }
}
