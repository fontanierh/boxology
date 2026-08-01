const SCHEMA: &str = "boxology.classification-report@1";

/// Renders a classification report in the canonical human-readable form.
///
/// The first line is `classification <verdict>`, followed by one `finding` line per report
/// finding. Findings retain classifier report order, and conditional findings append their
/// `condition` value. The output is deterministic and has one trailing newline.
pub fn render_text(report: &super::ClassificationReport) -> String {
    let mut output = String::from("classification ");
    output.push_str(report.verdict().canonical_name());
    output.push('\n');
    for finding in report.findings() {
        output.push_str("finding ");
        output.push_str(finding.code());
        output.push(' ');
        output.push_str(finding.path());
        output.push(' ');
        output.push_str(finding.class().canonical_name());
        if let Some(condition) = finding.condition() {
            output.push_str(" condition=\"");
            output.push_str(condition);
            output.push('\"');
        }
        output.push('\n');
    }
    output
}

/// Renders a classification report as its canonical deterministic JSON mirror.
///
/// The field inventory is fixed as `schema`, `verdict`, and `findings` at the top level, and
/// `code`, `path`, `class`, and optional `condition` for each finding. The two-space indentation,
/// field order, report order, and trailing newline are part of the output contract. `SCHEMA` is
/// the sole version string: changing this inventory requires a schema version bump rather than
/// an in-place field edit.
pub fn render_json(report: &super::ClassificationReport) -> String {
    let mut output = String::from("{\n  \"schema\": \"");
    push_escaped(&mut output, SCHEMA);
    output.push_str("\",\n  \"verdict\": \"");
    push_escaped(&mut output, report.verdict().canonical_name());
    output.push_str("\",\n  \"findings\": ");
    if report.findings().is_empty() {
        output.push_str("[]\n}\n");
        return output;
    }
    output.push_str("[\n");
    for (index, finding) in report.findings().iter().enumerate() {
        if index != 0 {
            output.push_str(",\n");
        }
        output.push_str("    {\n      \"code\": \"");
        push_escaped(&mut output, finding.code());
        output.push_str("\",\n      \"path\": \"");
        push_escaped(&mut output, finding.path());
        output.push_str("\",\n      \"class\": \"");
        push_escaped(&mut output, finding.class().canonical_name());
        if let Some(condition) = finding.condition() {
            output.push_str("\",\n      \"condition\": \"");
            push_escaped(&mut output, condition);
        }
        output.push_str("\"\n    }");
    }
    output.push_str("\n  ]\n}\n");
    output
}

fn push_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            _ => output.push(character),
        }
    }
}
