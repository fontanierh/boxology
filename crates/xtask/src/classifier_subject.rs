use boxology_classifier::{ClassificationReport, Finding, classify};
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
    let revision = submitted.revision.clone();
    let last = revision.chars().last().ok_or("hello revision is empty")?;
    let flipped = match last {
        '0' => '1',
        _ => '0',
    };
    submitted.revision = format!("{}{flipped}", &revision[..revision.len() - 1]);
    Ok((base, submitted))
}

fn render_report(report: &ClassificationReport) -> String {
    let mut body = format!("verdict {}\n", report.verdict().canonical_name());
    for finding in report.findings() {
        body.push_str(&render_finding(finding));
    }
    body
}

fn render_finding(finding: &Finding) -> String {
    format!(
        "finding {} {} {}\n",
        finding.code(),
        finding.path(),
        finding.class().canonical_name()
    )
}

fn report_introduced() -> Result<String, String> {
    let submitted = hello()?;
    let report = classify(None, Some(&submitted)).map_err(|error| error.to_string())?;
    Ok(render_report(&report))
}

fn report_removed() -> Result<String, String> {
    let base = hello()?;
    let report = classify(Some(&base), None).map_err(|error| error.to_string())?;
    Ok(render_report(&report))
}

fn report_unchanged() -> Result<String, String> {
    let (base, submitted) = provenance_only()?;
    let report = classify(Some(&base), Some(&submitted)).map_err(|error| error.to_string())?;
    Ok(render_report(&report))
}

fn report_changed() -> Result<String, String> {
    let (base, submitted) = renamed_input()?;
    let report = classify(Some(&base), Some(&submitted)).map_err(|error| error.to_string())?;
    Ok(render_report(&report))
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
    let written = [
        ("report-introduced.txt", report_introduced()?),
        ("report-removed.txt", report_removed()?),
        ("report-unchanged.txt", report_unchanged()?),
        ("report-changed.txt", report_changed()?),
        ("pairing-error.txt", pairing_error()?),
    ];
    for (name, body) in written {
        fs::write(out.join(name), body).map_err(|error| format!("write {name}: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_report_introduced_is_golden_and_repeatable() {
        let rendered = report_introduced().expect("introduced pair renders");
        assert_eq!(rendered, report_introduced().expect("it renders again"));
        assert_eq!(
            rendered,
            "verdict additive\nfinding BXC0026 hello additive\n"
        );
    }

    #[test]
    fn subject_report_removed_is_golden_and_repeatable() {
        let rendered = report_removed().expect("removed pair renders");
        assert_eq!(rendered, report_removed().expect("it renders again"));
        assert_eq!(
            rendered,
            "verdict incompatible\nfinding BXC0027 hello incompatible\n"
        );
    }

    #[test]
    fn subject_report_unchanged_is_golden_and_repeatable() {
        let (base, submitted) = provenance_only().expect("provenance pair builds");
        assert_ne!(base, submitted);
        let rendered = report_unchanged().expect("unchanged pair renders");
        assert_eq!(rendered, report_unchanged().expect("it renders again"));
        assert_eq!(rendered, "verdict unchanged\n");
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
        let rendered = report_changed().expect("changed pair renders");
        assert_eq!(rendered, report_changed().expect("it renders again"));
        assert_eq!(
            rendered,
            "verdict incompatible\nfinding BXC0028 hello incompatible\n"
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
