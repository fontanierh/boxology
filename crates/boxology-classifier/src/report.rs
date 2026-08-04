const SCHEMA: &str = "boxology.classification-report@2";

/// Renders a classification report in the canonical human-readable form.
///
/// The first line is `classification <verdict>`, followed by one `finding` line per report
/// finding. Findings retain classifier report order and always carry a quoted `path` field,
/// plus `kind`, `base`, and `submitted` excerpts (`-` when absent). Conditional findings append
/// their `condition` value. Every quoted textual field applies the printable-character escape
/// policy so hostile contract content cannot break the one-finding-per-line contract. The output
/// is deterministic and has one trailing newline.
pub fn render_text(report: &super::ClassificationReport) -> String {
    let mut output = String::from("classification ");
    output.push_str(report.verdict().canonical_name());
    output.push('\n');
    for finding in report.findings() {
        output.push_str("finding ");
        output.push_str(finding.code());
        output.push_str(" path=\"");
        push_escaped(&mut output, finding.path());
        output.push_str("\" ");
        output.push_str(finding.class().canonical_name());
        output.push_str(" kind=\"");
        push_escaped(&mut output, finding.kind());
        output.push_str("\" base=");
        push_text_excerpt(&mut output, finding.base_excerpt());
        output.push_str(" submitted=");
        push_text_excerpt(&mut output, finding.submitted_excerpt());
        if let Some(condition) = finding.condition() {
            output.push_str(" condition=\"");
            push_escaped(&mut output, condition);
            output.push('\"');
        }
        output.push('\n');
    }
    output
}

/// Renders a classification report as its canonical deterministic JSON mirror.
///
/// The field inventory is fixed as `schema`, `verdict`, and `findings` at the top level, and
/// `code`, `path`, `kind`, `class`, `base`, `submitted`, and optional `condition` for each
/// finding. `base` and `submitted` are always present (`null` when absent). The two-space
/// indentation, field order, report order, and trailing newline are part of the output contract.
/// `SCHEMA` is the sole version string: changing this inventory requires a schema version bump
/// rather than an in-place field edit. Every string value applies the printable-character escape
/// policy.
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
        output.push_str("\",\n      \"kind\": \"");
        push_escaped(&mut output, finding.kind());
        output.push_str("\",\n      \"class\": \"");
        push_escaped(&mut output, finding.class().canonical_name());
        output.push_str("\",\n      \"base\": ");
        push_json_excerpt(&mut output, finding.base_excerpt());
        output.push_str(",\n      \"submitted\": ");
        push_json_excerpt(&mut output, finding.submitted_excerpt());
        if let Some(condition) = finding.condition() {
            output.push_str(",\n      \"condition\": \"");
            push_escaped(&mut output, condition);
            output.push('\"');
        }
        output.push_str("\n    }");
    }
    output.push_str("\n  ]\n}\n");
    output
}

fn push_text_excerpt(output: &mut String, excerpt: Option<&str>) {
    match excerpt {
        None => output.push('-'),
        Some(value) => {
            output.push('\"');
            push_escaped(output, value);
            output.push('\"');
        }
    }
}

fn push_json_excerpt(output: &mut String, excerpt: Option<&str>) {
    match excerpt {
        None => output.push_str("null"),
        Some(value) => {
            output.push('\"');
            push_escaped(output, value);
            output.push('\"');
        }
    }
}

/// Escapes a string for human quoted fields and JSON string values.
///
/// Printable-character policy: preserve ordinary Unicode text (letters, marks, numbers,
/// punctuation, symbols, and ASCII space). Quotation mark and reverse solidus use JSON short
/// escapes. Every excluded scalar uses a JSON-valid encoding (`\b` `\t` `\n` `\f` `\r` or
/// `\uXXXX`, with supplementary scalars as a UTF-16 surrogate pair of `\uXXXX` units):
/// - C0 controls U+0000–U+001F
/// - DEL U+007F and C1 controls U+0080–U+009F (`char::is_control`)
/// - non-ASCII whitespace and line/paragraph separators (`char::is_whitespace` except U+0020)
/// - the complete Unicode 17.0.0 General_Category=Format (Cf) inventory
fn push_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\u{0c}' => output.push_str("\\f"),
            '\r' => output.push_str("\\r"),
            ch if must_escape(ch) => push_unicode_escape(output, ch),
            ch => output.push(ch),
        }
    }
}

fn must_escape(character: char) -> bool {
    character.is_control()
        || (character != ' ' && character.is_whitespace())
        || is_layout_or_spoofing_format(character)
}

/// Complete Unicode 17.0.0 General_Category=Cf inventory from UCD UnicodeData.txt.
/// Pinned to the repository toolchain where `std::char::UNICODE_VERSION == (17, 0, 0)`.
fn is_layout_or_spoofing_format(character: char) -> bool {
    character == '\u{00AD}'
        || ('\u{0600}'..='\u{0605}').contains(&character)
        || character == '\u{061C}'
        || character == '\u{06DD}'
        || character == '\u{070F}'
        || ('\u{0890}'..='\u{0891}').contains(&character)
        || character == '\u{08E2}'
        || character == '\u{180E}'
        || ('\u{200B}'..='\u{200F}').contains(&character)
        || ('\u{202A}'..='\u{202E}').contains(&character)
        || ('\u{2060}'..='\u{2064}').contains(&character)
        || ('\u{2066}'..='\u{206F}').contains(&character)
        || character == '\u{FEFF}'
        || ('\u{FFF9}'..='\u{FFFB}').contains(&character)
        || character == '\u{110BD}'
        || character == '\u{110CD}'
        || ('\u{13430}'..='\u{1343F}').contains(&character)
        || ('\u{1BCA0}'..='\u{1BCA3}').contains(&character)
        || ('\u{1D173}'..='\u{1D17A}').contains(&character)
        || character == '\u{E0001}'
        || ('\u{E0020}'..='\u{E007F}').contains(&character)
}

fn push_unicode_escape(output: &mut String, character: char) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let push_unit = |output: &mut String, unit: u16| {
        output.push_str("\\u");
        output.push(HEX[((unit >> 12) & 0xf) as usize] as char);
        output.push(HEX[((unit >> 8) & 0xf) as usize] as char);
        output.push(HEX[((unit >> 4) & 0xf) as usize] as char);
        output.push(HEX[(unit & 0xf) as usize] as char);
    };
    let code = u32::from(character);
    if code < 0x1_0000 {
        push_unit(output, code as u16);
    } else {
        let adjusted = code - 0x1_0000;
        let high = 0xD800 + ((adjusted >> 10) as u16);
        let low = 0xDC00 + ((adjusted & 0x3FF) as u16);
        push_unit(output, high);
        push_unit(output, low);
    }
}
