use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

#[derive(Debug, PartialEq, Eq)]
struct Diagnostic {
    file: String,
    line: usize,
    message: String,
}

pub(crate) fn check(root: &Path) -> bool {
    let (files, tracked) = match load(root) {
        Ok(tree) => tree,
        Err(error) => {
            eprintln!("links: ERROR: {error}");
            return false;
        }
    };
    let diagnostics = check_files(files, tracked);
    for item in &diagnostics {
        eprintln!("{}:{}: {}", item.file, item.line, item.message);
    }
    diagnostics.is_empty()
}

fn load(root: &Path) -> Result<(BTreeMap<String, String>, BTreeSet<String>), String> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files exited with {}", output.status));
    }
    let mut files = BTreeMap::new();
    let mut tracked = BTreeSet::new();
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
    {
        let name =
            std::str::from_utf8(raw).map_err(|_| "git returned a non-UTF-8 path".to_string())?;
        tracked.insert(name.to_string());
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
    Ok((files, tracked))
}

fn check_files(files: BTreeMap<String, String>, tracked: BTreeSet<String>) -> Vec<Diagnostic> {
    let anchors: BTreeMap<_, _> = files
        .iter()
        .map(|(name, text)| (name.clone(), headings(text)))
        .collect();
    let directories = directories(&tracked);
    let mut diagnostics = Vec::new();
    for (file, text) in &files {
        scan(text, |line, finding| {
            let result = finding.and_then(|(image, destination)| {
                validate(file, image, destination, &tracked, &directories, &anchors)
            });
            if let Err(message) = result {
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

fn scan(text: &str, mut emit: impl FnMut(usize, Result<(bool, &str), String>)) {
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
            let (image, open) = if syntax[cursor] == b'[' {
                (false, cursor)
            } else if syntax[cursor] == b'!' && syntax.get(cursor + 1) == Some(&b'[') {
                (true, cursor + 1)
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
            if syntax.get(close + 1) == Some(&b':') {
                emit(
                    number,
                    Err("unsupported reference-style link definition".into()),
                );
                cursor = close + 1;
                continue;
            }
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
                emit(number, Ok((image, destination)));
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
// parsed as prose. HTML comments and blockquotes are not suppressed; raw HTML anchors can
// pass silently until the parser's supported shape grows.
fn syntax(line: &str) -> Vec<u8> {
    let raw = line.as_bytes();
    let mut visible = raw.to_vec();
    let mut cursor = 0;
    while cursor < raw.len() {
        if cursor + 1 < raw.len() && raw[cursor] == b'\\' && raw[cursor + 1].is_ascii_punctuation()
        {
            visible[cursor] = 0;
            visible[cursor + 1] = 0;
            if raw[cursor + 1] == b'['
                && let Some(close) = balanced(raw, cursor + 1, b'[', b']')
            {
                visible[close] = 0;
            }
            cursor += 2;
        } else {
            if raw[cursor] != b'`' {
                cursor += 1;
                continue;
            }
            let length = raw[cursor..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            let mut search = cursor + length;
            let mut close = None;
            while search < raw.len() {
                if raw[search] != b'`' {
                    search += 1;
                    continue;
                }
                let run = raw[search..]
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

fn validate(
    file: &str,
    image: bool,
    destination: &str,
    tracked: &BTreeSet<String>,
    directories: &BTreeSet<String>,
    anchors: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), String> {
    if external(destination) {
        return Ok(());
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
    let target =
        resolve(file, path).map_err(|error| format!("destination {destination:?}: {error}"))?;
    if image && fragment.is_some() {
        return Err(format!(
            "image destination {destination:?} resolves to {target:?}, but image fragments are unsupported"
        ));
    }
    let is_file = tracked.contains(&target);
    let is_directory = directories.contains(&target);
    if !is_file && !is_directory {
        return Err(format!(
            "broken destination {destination:?}: resolved target {target:?} is not tracked"
        ));
    }
    let Some(fragment) = fragment else {
        return Ok(());
    };
    if is_directory {
        return Err(format!(
            "destination {destination:?} resolves to directory {target:?}, which cannot have a fragment"
        ));
    }
    if !markdown(&target) {
        return Err(format!(
            "destination {destination:?} resolves to non-Markdown target {target:?}, which cannot have a fragment"
        ));
    }
    if fragment.is_empty() {
        return Err(format!(
            "destination {destination:?} has an empty fragment for target {target:?}"
        ));
    }
    if !anchors
        .get(&target)
        .is_some_and(|set| set.contains(fragment))
    {
        return Err(format!(
            "broken fragment in destination {destination:?}: target {target:?} has no anchor {fragment:?}"
        ));
    }
    Ok(())
}

fn resolve(file: &str, path: &str) -> Result<String, &'static str> {
    if path.is_empty() {
        return Ok(file.to_string());
    }
    let mut parts: Vec<_> = file.split('/').collect();
    parts.pop();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.pop().is_none() => return Err("path traverses above repository root"),
            ".." => {}
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

fn directories(tracked: &BTreeSet<String>) -> BTreeSet<String> {
    let mut directories = BTreeSet::from([String::new()]);
    for name in tracked {
        let mut current = name.as_str();
        while let Some((parent, _)) = current.rsplit_once('/') {
            directories.insert(parent.to_string());
            current = parent;
        }
    }
    directories
}

fn headings(text: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut fence = None;
    for line in text.lines() {
        if fenced(line, &mut fence) {
            continue;
        }
        let indent = line.bytes().take_while(|byte| *byte == b' ').count();
        if indent > 3 {
            continue;
        }
        let tail = &line[indent..];
        let hashes = tail.bytes().take_while(|byte| *byte == b'#').count();
        if !(1..=6).contains(&hashes)
            || !tail[hashes..].is_empty() && !matches!(tail.as_bytes()[hashes], b' ' | b'\t')
        {
            continue;
        }
        let mut title = tail[hashes..].trim();
        let closing = title.bytes().rev().take_while(|byte| *byte == b'#').count();
        if closing > 0 && title[..title.len() - closing].ends_with(char::is_whitespace) {
            title = title[..title.len() - closing].trim_end();
        }
        let base = slug(&render_heading(title));
        let mut candidate = base.clone();
        let mut suffix = 1;
        while result.contains(&candidate) {
            candidate = format!("{base}-{suffix}");
            suffix += 1;
        }
        result.insert(candidate);
    }
    result
}

fn render_heading(title: &str) -> String {
    let visible = syntax(title);
    let mut rendered = String::new();
    let mut copied = 0;
    let mut cursor = 0;
    while cursor < visible.len() {
        let (prefix, open) = if visible[cursor] == b'[' {
            (cursor, cursor)
        } else if visible[cursor] == b'!' && visible.get(cursor + 1) == Some(&b'[') {
            (cursor, cursor + 1)
        } else {
            cursor += 1;
            continue;
        };
        let Some(close) = balanced(&visible, open, b'[', b']') else {
            cursor = open + 1;
            continue;
        };
        if visible.get(close + 1) != Some(&b'(') {
            cursor = close + 1;
            continue;
        }
        let Some(end) = balanced(&visible, close + 1, b'(', b')') else {
            break;
        };
        rendered.push_str(&title[copied..prefix]);
        rendered.push_str(&title[open + 1..close]);
        copied = end + 1;
        cursor = copied;
    }
    rendered.push_str(&title[copied..]);
    rendered
}

// GitHub-compatible for the current corpus; combining marks, emoji, and emphasis-heavy
// headings remain divergences. Dash-setext/thematic-break lines deliberately create no
// anchor, so a fragment cannot silently pass; '=' setext is rejected by the shape parser.
// ATX-looking lines inside HTML comments may create phantom anchors under the accepted
// raw-HTML boundary.
fn slug(title: &str) -> String {
    let mut escaped = false;
    title
        .chars()
        .filter(|character| {
            if escaped {
                escaped = false;
                return true;
            }
            if *character == '\\' {
                escaped = true;
                return false;
            }
            *character != '`'
        })
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric() || matches!(character, ' ' | '-' | '_'))
        .map(|character| if character == ' ' { '-' } else { character })
        .collect()
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
    matches!(path.rsplit_once('.'), Some((_, extension)) if extension.eq_ignore_ascii_case("md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(text: &str) -> Vec<Diagnostic> {
        check_files(
            BTreeMap::from([("test.md".into(), text.into())]),
            BTreeSet::from(["test.md".into(), "relative.md".into()]),
        )
    }

    #[test]
    fn unsupported_shapes_fail_closed() {
        let text = "[ref]: x\n> [quote]: x\n- [list]: x\noops](x)\n[a][ref]\n[a](<x>)\n[a](x \"title\")\n[a]()\n[a](/x)\n[a](x%20y)\n[a](x\\y)\n====\n";
        let got = checked(text);
        assert_eq!(got.len(), 12);
        let references = got.iter().filter(|item| item.message.contains("reference"));
        assert_eq!(references.count(), 3);
        for needle in [
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
    fn escaped_backtick_can_close_a_span_without_hiding_a_live_link() {
        let got = checked(r#"The pattern `\` matches; see [x](gone.md "title") and `code`."#);
        assert_eq!(got.len(), 1);
        assert!(got[0].message.contains("whitespace"));
    }

    #[test]
    fn heading_slugs_use_immutable_base_suffixes() {
        let third_a = headings("# A\n# A\n# A\n");
        let expected = ["a", "a-1", "a-2"];
        assert_eq!(third_a, expected.into_iter().map(str::to_string).collect());

        let colliding = headings("# A\n# A\n# A-1\n# S0 — Product-Repo Bootstrap and CI\n");
        let expected = ["a", "a-1", "a-1-1", "s0--product-repo-bootstrap-and-ci"];
        assert_eq!(
            colliding,
            expected.into_iter().map(str::to_string).collect()
        );
    }

    #[test]
    fn heading_links_slug_their_rendered_labels() {
        let text = "# [Product](p.md)\n# [Boxes](b.md)\n# [Packages](p.md)\n# [Runtime](r.md)\n# [Evolution](e.md)\n# [Software Factory](f.md) — the flagship application\n# [Quality and Authority](q.md)\n";
        let expected = [
            "product",
            "boxes",
            "packages",
            "runtime",
            "evolution",
            "software-factory--the-flagship-application",
            "quality-and-authority",
        ];
        assert_eq!(
            headings(text),
            expected.into_iter().map(str::to_string).collect()
        );
        let got = checked("# [Product](missing.md)\n");
        assert!(got[0].message.contains("missing.md"));
    }

    #[test]
    fn tracked_relative_self_query_directory_and_paren_targets_pass() {
        let source = "# Here\n[relative](../target.md#target) [self](#here) [query](../target.md?q=1#target)\n[directory](../assets) [directory-slash](../assets/) [paren](../assets/(name).bin) ![image](../image.png)\n";
        let files = BTreeMap::from([
            ("docs/source.md".into(), source.into()),
            ("target.md".into(), "# Target\n".into()),
        ]);
        let tracked = [
            "docs/source.md",
            "target.md",
            "assets/file.bin",
            "assets/(name).bin",
            "image.png",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert!(check_files(files, tracked).is_empty());
    }

    #[test]
    fn exact_tracked_targets_and_fragments_fail_loudly() {
        let source = "[missing](../missing.md)\n[case](../Target.md)\n[untracked](../ghost.md)\n[above](../../bad)\n[anchor](../target.md#missing)\n[empty](../target.md#)\n[dir](../assets#x)\n[non-md](../image.png#x)\n![image](../target.md#target)\n";
        let files = BTreeMap::from([
            ("docs/source.md".into(), source.into()),
            ("target.md".into(), "# Target\n".into()),
            ("ghost.md".into(), "# Ghost\n".into()),
        ]);
        let tracked = ["docs/source.md", "target.md", "assets/file", "image.png"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let got = check_files(files, tracked);
        assert_eq!(got.len(), 9);
        for needle in [
            "missing.md",
            "Target.md",
            "ghost.md",
            "above repository",
            "#missing",
            "empty fragment",
            "directory",
            "non-Markdown",
            "image fragments",
        ] {
            assert!(got.iter().any(|item| item.message.contains(needle)));
        }
        assert!(got[1].message.contains("resolved target \"Target.md\""));
    }

    #[test]
    fn dash_setext_does_not_create_a_silent_anchor() {
        let files = BTreeMap::from([("dash.md".into(), "Title\n---\n[bad](#title)\n".into())]);
        let got = check_files(files, BTreeSet::from(["dash.md".into()]));
        assert_eq!(got.len(), 1);
        assert!(got[0].message.contains("no anchor \"title\""));
    }

    #[test]
    fn live_tracked_markdown_is_nonempty_and_shape_clean() {
        let (files, tracked) = load(&crate::root()).unwrap();
        assert!(!files.is_empty());
        assert!(tracked.len() >= files.len());
        assert!(check_files(files, tracked).is_empty());
    }
}
