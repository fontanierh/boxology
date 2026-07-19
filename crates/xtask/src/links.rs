use std::{collections::BTreeMap, fs, path::Path, process::Command};

#[derive(Debug, PartialEq, Eq)]
struct Diagnostic {
    file: String,
    line: usize,
    message: String,
}

pub(crate) fn check(root: &Path) -> bool {
    let files = match load(root) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("links: ERROR: {error}");
            return false;
        }
    };
    let diagnostics = check_files(files);
    for item in &diagnostics {
        eprintln!("{}:{}: {}", item.file, item.line, item.message);
    }
    diagnostics.is_empty()
}

fn load(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files exited with {}", output.status));
    }
    let mut files = BTreeMap::new();
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
    {
        let name =
            std::str::from_utf8(raw).map_err(|_| "git returned a non-UTF-8 path".to_string())?;
        if markdown(name) {
            let bytes = fs::read(root.join(name)).map_err(|error| format!("{name}: {error}"))?;
            let text = String::from_utf8(bytes)
                .map_err(|_| format!("{name}: tracked Markdown is not UTF-8"))?;
            files.insert(name.to_string(), text);
        }
    }
    if files.is_empty() {
        return Err("repository has no tracked Markdown files".into());
    }
    Ok(files)
}

fn check_files(files: BTreeMap<String, String>) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for (file, text) in files {
        scan(&text, |line, finding| {
            if let Err(message) = finding.and_then(validate) {
                diagnostics.push(Diagnostic {
                    file: file.clone(),
                    line,
                    message,
                });
            }
        });
    }
    diagnostics
}

fn scan(text: &str, mut emit: impl FnMut(usize, Result<&str, String>)) {
    let mut fence = None;
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        if fenced(line, &mut fence) {
            continue;
        }
        let syntax = syntax(line);
        let indent = syntax.iter().take_while(|byte| **byte == b' ').count();
        let trimmed = &syntax[indent..];
        if indent <= 3
            && trimmed.first() == Some(&b'[')
            && balanced(trimmed, 0, b'[', b']')
                .is_some_and(|close| trimmed.get(close + 1) == Some(&b':'))
        {
            emit(
                number,
                Err("unsupported reference-style link definition".into()),
            );
            continue;
        }
        if indent <= 3
            && !trimmed.is_empty()
            && trimmed.iter().take_while(|byte| **byte == b'=').count() > 0
            && trimmed
                .iter()
                .all(|byte| *byte == b'=' || byte.is_ascii_whitespace())
        {
            emit(number, Err("unsupported setext '=' heading".into()));
            continue;
        }
        let mut cursor = 0;
        while cursor < syntax.len() {
            let open = if syntax[cursor] == b'[' {
                cursor
            } else if syntax[cursor] == b'!' && syntax.get(cursor + 1) == Some(&b'[') {
                cursor + 1
            } else {
                if syntax[cursor..].starts_with(b"](") || syntax[cursor..].starts_with(b"][") {
                    emit(
                        number,
                        Err("unsupported link-shaped Markdown residue".into()),
                    );
                    cursor += 2;
                } else {
                    cursor += 1;
                }
                continue;
            };
            let Some(close) = balanced(&syntax, open, b'[', b']') else {
                cursor = open + 1;
                continue;
            };
            if syntax.get(close + 1) != Some(&b'(') {
                if syntax.get(close + 1) == Some(&b'[') {
                    emit(
                        number,
                        Err("unsupported link-shaped Markdown residue".into()),
                    );
                }
                cursor = close + 1;
                continue;
            }
            if syntax[open + 1..close]
                .windows(2)
                .any(|pair| pair == b"](" || pair == b"][")
            {
                emit(
                    number,
                    Err("nested inline links or images are unsupported".into()),
                );
                cursor = close + 1;
                continue;
            }
            let Some(end) = balanced(&syntax, close + 1, b'(', b')') else {
                emit(
                    number,
                    Err("unsupported unterminated inline-link destination".into()),
                );
                break;
            };
            let destination = &line[close + 2..end];
            if destination.bytes().any(|byte| byte.is_ascii_whitespace()) {
                emit(
                    number,
                    Err(format!(
                        "unsupported destination {destination:?}: whitespace or titles are not supported"
                    )),
                );
            } else if destination.contains('<') || destination.contains('>') {
                emit(
                    number,
                    Err(format!(
                        "unsupported destination {destination:?}: angle destinations are not supported"
                    )),
                );
            } else {
                emit(number, Ok(destination));
            }
            cursor = end + 1;
        }
    }
}

fn fenced(line: &str, active: &mut Option<(u8, usize)>) -> bool {
    let bytes = line.as_bytes();
    let indent = bytes.iter().take_while(|byte| **byte == b' ').count();
    if let Some((marker, length)) = *active {
        if indent <= 3 {
            let run = bytes[indent..]
                .iter()
                .take_while(|byte| **byte == marker)
                .count();
            if run >= length && bytes[indent + run..].iter().all(u8::is_ascii_whitespace) {
                *active = None;
            }
        }
        return true;
    }
    if indent <= 3 && matches!(bytes.get(indent), Some(b'`' | b'~')) {
        let marker = bytes[indent];
        let length = bytes[indent..]
            .iter()
            .take_while(|byte| **byte == marker)
            .count();
        if length >= 3 {
            *active = Some((marker, length));
            return true;
        }
    }
    false
}

// This deliberate loud subset does not model four-space indented code, fences indented
// over three spaces, multiline code spans, or heading structure: link shapes there are
// parsed as prose. HTML comments and blockquotes are likewise not suppressed.
fn syntax(line: &str) -> Vec<u8> {
    let raw = line.as_bytes();
    let mut visible = raw.to_vec();
    let mut cursor = 0;
    while cursor + 1 < raw.len() {
        if raw[cursor] == b'\\' && raw[cursor + 1].is_ascii_punctuation() {
            visible[cursor] = 0;
            visible[cursor + 1] = 0;
            if raw[cursor + 1] == b'['
                && let Some(close) = balanced(raw, cursor + 1, b'[', b']')
            {
                visible[close] = 0;
            }
            cursor += 2;
        } else {
            cursor += 1;
        }
    }
    cursor = 0;
    while cursor < visible.len() {
        if visible[cursor] != b'`' {
            cursor += 1;
            continue;
        }
        let length = visible[cursor..]
            .iter()
            .take_while(|byte| **byte == b'`')
            .count();
        let mut search = cursor + length;
        let mut close = None;
        while search < visible.len() {
            if visible[search] != b'`' {
                search += 1;
                continue;
            }
            let run = visible[search..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if run == length {
                close = Some(search + run);
                break;
            }
            search += run;
        }
        if let Some(end) = close {
            visible[cursor..end].fill(b' ');
            cursor = end;
        } else {
            cursor += length;
        }
    }
    visible
}

fn balanced(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0;
    for (offset, byte) in bytes[start..].iter().enumerate() {
        if *byte == open {
            depth += 1;
        } else if *byte == close {
            depth -= 1;
            if depth == 0 {
                return Some(start + offset);
            }
        }
    }
    None
}

// Stack layer one validates syntax only. Tracked-target and anchor resolution follow in PR 2.
fn validate(destination: &str) -> Result<&str, String> {
    if external(destination) {
        return Ok(destination);
    }
    let (before_fragment, fragment) = destination
        .split_once('#')
        .map_or((destination, None), |(path, fragment)| {
            (path, Some(fragment))
        });
    let path = before_fragment
        .split_once('?')
        .map_or(before_fragment, |pair| pair.0);
    if path.is_empty() && fragment.is_none() {
        return Err(format!("empty link destination {destination:?}"));
    }
    if path.starts_with('/') {
        return Err(format!(
            "root-relative destination {destination:?} is unsupported"
        ));
    }
    if destination.contains('%') {
        return Err(format!(
            "unsupported destination {destination:?}: percent encoding is not supported"
        ));
    }
    if destination.contains('\\') {
        return Err(format!(
            "unsupported destination {destination:?}: backslashes are not supported"
        ));
    }
    Ok(destination)
}

fn external(destination: &str) -> bool {
    if destination.starts_with("//") {
        return true;
    }
    let Some((scheme, _)) = destination.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
}

fn markdown(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(text: &str) -> Vec<Diagnostic> {
        check_files(BTreeMap::from([("test.md".into(), text.into())]))
    }

    #[test]
    fn unsupported_shapes_fail_closed() {
        let text = "[ref]: x\noops](x)\n[a][ref]\n[a](<x>)\n[a](x \"title\")\n[a]()\n[a](/x)\n[a](x%20y)\n[a](x\\y)\n====\n";
        let got = checked(text);
        assert_eq!(got.len(), 10);
        for needle in [
            "reference",
            "residue",
            "angle",
            "whitespace",
            "empty",
            "root-relative",
            "percent",
            "backslash",
            "setext",
        ] {
            assert!(got.iter().any(|item| item.message.contains(needle)));
        }
    }

    #[test]
    fn fences_spans_escapes_and_external_destinations_are_skipped() {
        let text = "```\n[x](bad title)\n````\n~~~\n[x](bad title)\n~~~~\n`[x](bad title)` ``[x](bad title)`` \\[x](bad title)\n[x](https://host/a%20b) [x](//host/a%20b) [ok](relative.md)\n```\n[x](bad title)\n";
        assert!(checked(text).is_empty());
        assert!(checked("````\n[x](bad title)\n```\n[x](bad title)\n````\n").is_empty());
        assert_eq!(checked("    ```\n[x](bad title)\n").len(), 1);
    }

    #[test]
    fn nested_constructs_escaped_backticks_and_backslashes_are_loud() {
        let got = checked(
            "[outer [inner](x)](y)\n[outer ![image](x)](y)\n\\` [x](bad title) `\n[x](bad\\path)\n",
        );
        assert_eq!(got.len(), 4);
        assert_eq!(
            got.iter()
                .filter(|item| item.message.contains("nested"))
                .count(),
            2
        );
        assert!(got.iter().any(|item| item.message.contains("whitespace")));
        assert!(got.iter().any(|item| item.message.contains("backslash")));
    }

    #[test]
    fn live_tracked_markdown_is_nonempty_and_shape_clean() {
        let files = load(&crate::root()).unwrap();
        assert!(!files.is_empty());
        assert!(check_files(files).is_empty());
    }
}
