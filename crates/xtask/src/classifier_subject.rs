use boxology_classifier::{ClassificationReport, classify, render_json, render_text};
use boxology_schema::SchemaDocument;
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

fn variant_added() -> Result<(SchemaDocument, SchemaDocument), String> {
    let base = hello()?;
    let mut submitted = hello()?;
    let schema_type = submitted
        .types
        .get_mut(0)
        .ok_or("hello fixture has no type")?;
    schema_type.variants.push(boxology_schema::SchemaVariant {
        name: String::from("Other"),
        docs: Vec::new(),
        deprecation: None,
        payload: boxology_schema::SchemaPayload::Unit,
    });
    flip_revision(&mut submitted)?;
    Ok((base, submitted))
}

fn report_introduced() -> Result<ClassificationReport, String> {
    let submitted = hello()?;
    let report = classify(None, Some(&submitted)).map_err(|error| error.to_string())?;
    Ok(report)
}

fn report_removed() -> Result<ClassificationReport, String> {
    let base = hello()?;
    let report = classify(Some(&base), None).map_err(|error| error.to_string())?;
    Ok(report)
}

fn report_unchanged() -> Result<ClassificationReport, String> {
    let (base, submitted) = provenance_only()?;
    let report = classify(Some(&base), Some(&submitted)).map_err(|error| error.to_string())?;
    Ok(report)
}

fn report_changed() -> Result<ClassificationReport, String> {
    let (base, submitted) = renamed_input()?;
    let report = classify(Some(&base), Some(&submitted)).map_err(|error| error.to_string())?;
    Ok(report)
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
    let (base, submitted) = variant_added()?;
    let report = classify(Some(&base), Some(&submitted)).map_err(|error| error.to_string())?;
    Ok(report)
}

fn pairing_error() -> Result<String, String> {
    let base = hello()?;
    let submitted = ping()?;
    let error = classify(Some(&base), Some(&submitted))
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
    ];
    for (name, report) in reports {
        fs::write(out.join(format!("{name}.txt")), render_text(&report))
            .map_err(|error| format!("write {name}.txt: {error}"))?;
        fs::write(out.join(format!("{name}.json")), render_json(&report))
            .map_err(|error| format!("write {name}.json: {error}"))?;
    }
    let errors = [
        ("report-capability-only.txt", report_capability_only()?),
        ("report-types-only.txt", report_types_only()?),
        ("report-revision-only.txt", report_revision_only()?),
        ("pairing-error.txt", pairing_error()?),
    ];
    for (name, body) in errors {
        fs::write(out.join(name), body).map_err(|error| format!("write {name}: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "classification additive\nfinding BXC0026 hello additive\n",
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
    }

    #[test]
    fn subject_report_removed_is_golden_and_repeatable() {
        assert_report_golden(
            report_removed().expect("removed pair renders"),
            report_removed().expect("it renders again"),
            "classification incompatible\nfinding BXC0027 hello incompatible\n",
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
  "schema": "boxology.classification-report@1",
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
            "classification incompatible\nfinding BXC0028 hello incompatible\n",
            r#"{
  "schema": "boxology.classification-report@1",
  "verdict": "incompatible",
  "findings": [
    {
      "code": "BXC0028",
      "path": "hello",
      "class": "incompatible"
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
finding BXC0036 hello/type/GreetError/variant/Other compatible_with_conditions condition=\"unknown-variant tolerance\"\n",
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
}
