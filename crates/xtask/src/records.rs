use crate::budget;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const DIRECTORY: &str = "records";
const INDEX: &str = "README.md";
const FRICTION_LOG: &str = "ops/friction-log.md";
const FRICTION_LOG_FILE_DELETED: &str = "FRICTION_LOG_FILE_DELETED";
const FRICTION_LOG_INVALID_BASE: &str = "FRICTION_LOG_INVALID_BASE";
const FRICTION_LOG_ESTABLISHED_BYTES_CHANGED: &str = "FRICTION_LOG_ESTABLISHED_BYTES_CHANGED";
const FRICTION_LOG_INVALID_INSERTION: &str = "FRICTION_LOG_INVALID_INSERTION";
const FRICTION_LOG_DUPLICATE_CLASSIFICATION: &str = "FRICTION_LOG_DUPLICATE_CLASSIFICATION";
const FRICTION_LOG_DUPLICATE_ENTRY: &str = "FRICTION_LOG_DUPLICATE_ENTRY";
const FRICTION_LOG_INVALID_APPENDED_ENTRY: &str = "FRICTION_LOG_INVALID_APPENDED_ENTRY";

pub(crate) fn run(root: &Path, base: Option<&str>) -> u8 {
    match check(root, base) {
        Ok(problems) if problems.is_empty() => {
            println!("records: PASS");
            0
        }
        Ok(problems) => {
            eprintln!("records: FAIL");
            for problem in &problems {
                eprintln!("  {problem}");
            }
            eprintln!(
                "Records are append-only: a record is named YYYY-MM-DD-topic.md, is indexed in {DIRECTORY}/{INDEX}, and is never edited, renamed, or deleted after merge; corrections are new records citing the old (AGENTS.md, Operational records)."
            );
            1
        }
        Err(error) => {
            eprintln!("records: ERROR: {error}");
            2
        }
    }
}

fn check(root: &Path, base: Option<&str>) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    let directory = root.join(DIRECTORY);
    let mut on_disk = BTreeSet::new();
    let entries =
        fs::read_dir(&directory).map_err(|error| format!("cannot read {DIRECTORY}/: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read {DIRECTORY}/: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() {
            problems.push(format!(
                "{DIRECTORY}/{name}: subdirectories are not allowed"
            ));
        } else if name == INDEX {
            // The index is the one mutable file; validated against the set below.
        } else if is_record_name(&name) {
            on_disk.insert(name);
        } else {
            problems.push(format!(
                "{DIRECTORY}/{name}: name does not match YYYY-MM-DD-topic.md (lowercase topic of [a-z0-9] words separated by single hyphens)"
            ));
        }
    }
    match fs::read_to_string(directory.join(INDEX)) {
        Ok(index) => {
            let indexed: BTreeSet<String> = link_targets(&index)
                .into_iter()
                .filter(|target| is_record_name(target))
                .collect();
            for name in on_disk.difference(&indexed) {
                problems.push(format!(
                    "{DIRECTORY}/{name}: not linked from the {INDEX} index"
                ));
            }
            for name in indexed.difference(&on_disk) {
                problems.push(format!(
                    "{DIRECTORY}/{INDEX}: links {name}, which does not exist"
                ));
            }
        }
        Err(error) => problems.push(format!("{DIRECTORY}/{INDEX}: cannot read index: {error}")),
    }
    if let Some(revision) = base {
        append_only_problems(root, revision, &mut problems)?;
    }
    Ok(problems)
}

fn is_record_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".md") else {
        return false;
    };
    let bytes = stem.as_bytes();
    if bytes.len() < 12 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'-' {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| {
        bytes[range.clone()]
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| stem[range].parse::<u32>().unwrap())
    };
    let (Some(_), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return false;
    }
    let topic = &stem[11..];
    topic.split('-').all(|word| {
        !word.is_empty()
            && word
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

fn link_targets(markdown: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut rest = markdown;
    while let Some(open) = rest.find("](") {
        rest = &rest[open + 2..];
        if let Some(close) = rest.find(')') {
            targets.push(rest[..close].to_string());
            rest = &rest[close + 1..];
        }
    }
    targets
}

fn append_only_problems(
    root: &Path,
    revision: &str,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let requested = format!("{revision}^{{commit}}");
    let base = budget::git_text(root, &["rev-parse", "--verify", "--quiet", &requested])
        .map_err(|_| format!("unknown revision {revision:?}; {}", budget::HISTORY_REMEDY))?;
    let head = budget::git_text(root, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
        .map_err(|_| String::from("cannot resolve HEAD to a commit"))?;
    let merge_base = budget::git_text(root, &["merge-base", &base, &head]).map_err(|_| {
        format!(
            "no merge base for {base} and {head}; {}",
            budget::HISTORY_REMEDY
        )
    })?;
    let output = budget::git(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--find-renames",
            "--name-status",
            "-z",
            &merge_base,
            &head,
            "--",
            &format!("{DIRECTORY}/"),
        ],
    )?;
    if !output.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let index_path = format!("{DIRECTORY}/{INDEX}");
    let mut fields = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| String::from_utf8_lossy(field).into_owned());
    while let Some(status) = fields.next() {
        let path = fields.next().ok_or("truncated git name-status record")?;
        match status.as_bytes().first() {
            Some(b'A') | Some(b'C') => {}
            Some(b'R') => {
                fields.next().ok_or("truncated git rename record")?;
                if path != index_path {
                    problems.push(format!("{path}: existed at the merge base and was renamed"));
                }
            }
            Some(b'D') if path != index_path => {
                problems.push(format!("{path}: existed at the merge base and was deleted"));
            }
            Some(b'D') => {}
            _ if path != index_path => {
                problems.push(format!(
                    "{path}: existed at the merge base and was modified"
                ));
            }
            _ => {}
        }
    }
    friction_log_problems(root, &merge_base, &head, problems)?;
    Ok(())
}

fn friction_log_problems(
    root: &Path,
    merge_base: &str,
    head: &str,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let base = committed_file(root, merge_base)?;
    let submitted = committed_file(root, head)?;
    let Some(base) = base else {
        if let Some(submitted) = submitted
            && let Err(error) = parse_friction_log(&submitted)
        {
            friction_finding(problems, FRICTION_LOG_INVALID_APPENDED_ENTRY, error.reason);
        }
        return Ok(());
    };
    let Some(submitted) = submitted else {
        friction_finding(
            problems,
            FRICTION_LOG_FILE_DELETED,
            "path is absent at HEAD",
        );
        return Ok(());
    };

    let base = match parse_friction_log(&base) {
        Ok(document) => document,
        Err(error) => {
            friction_finding(problems, FRICTION_LOG_INVALID_BASE, error.reason);
            return Ok(());
        }
    };
    let submitted = match parse_friction_log(&submitted) {
        Ok(document) => document,
        Err(error) => {
            let appended = error.entry.is_some_and(|entry| entry >= base.entries.len());
            let code = match error.class {
                FrictionParseClass::DuplicateEntry => FRICTION_LOG_DUPLICATE_ENTRY,
                FrictionParseClass::DuplicateClassification => {
                    FRICTION_LOG_DUPLICATE_CLASSIFICATION
                }
                FrictionParseClass::InvalidInsertion if appended => {
                    FRICTION_LOG_INVALID_APPENDED_ENTRY
                }
                FrictionParseClass::InvalidInsertion => FRICTION_LOG_INVALID_INSERTION,
                FrictionParseClass::Changed if appended => FRICTION_LOG_INVALID_APPENDED_ENTRY,
                FrictionParseClass::Changed => FRICTION_LOG_ESTABLISHED_BYTES_CHANGED,
            };
            friction_finding(problems, code, error.reason);
            return Ok(());
        }
    };

    if base.preamble != submitted.preamble || submitted.entries.len() < base.entries.len() {
        friction_finding(
            problems,
            FRICTION_LOG_ESTABLISHED_BYTES_CHANGED,
            "preamble or established entry set changed",
        );
        return Ok(());
    }
    for (base_entry, submitted_entry) in base.entries.iter().zip(&submitted.entries) {
        if base_entry.heading != submitted_entry.heading
            || base_entry.classification != submitted_entry.classification
            || base_entry.observation != submitted_entry.observation
            || base_entry.evidence != submitted_entry.evidence
            || !statuses_preserved(base_entry, submitted_entry)
        {
            friction_finding(
                problems,
                FRICTION_LOG_ESTABLISHED_BYTES_CHANGED,
                "an established entry byte was modified, deleted, duplicated, or reordered",
            );
            return Ok(());
        }
    }
    Ok(())
}

fn friction_finding(problems: &mut Vec<String>, code: &str, detail: &str) {
    problems.push(format!("{FRICTION_LOG}: {code} ({detail})"));
}

fn committed_file(root: &Path, revision: &str) -> Result<Option<Vec<u8>>, String> {
    let object = format!("{revision}:{FRICTION_LOG}");
    let exists = budget::git(root, &["cat-file", "-e", &object])?;
    if !exists.status.success() {
        return Ok(None);
    }
    let output = budget::git(root, &["show", &object])?;
    if !output.status.success() {
        return Err(format!(
            "cannot read {FRICTION_LOG} at {revision}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Some(output.stdout))
}

#[derive(Clone, Copy, Debug)]
enum FrictionParseClass {
    Changed,
    InvalidInsertion,
    DuplicateClassification,
    DuplicateEntry,
}

#[derive(Clone, Copy, Debug)]
struct FrictionParseError {
    class: FrictionParseClass,
    entry: Option<usize>,
    reason: &'static str,
}

impl FrictionParseError {
    fn at(class: FrictionParseClass, entry: usize, reason: &'static str) -> Self {
        Self {
            class,
            entry: Some(entry),
            reason,
        }
    }
}

struct FrictionDocument<'a> {
    preamble: Vec<&'a str>,
    entries: Vec<FrictionEntry<'a>>,
}

struct FrictionEntry<'a> {
    heading: &'a str,
    classification: &'a str,
    observation: &'a str,
    evidence: &'a str,
    statuses: [Vec<&'a str>; 3],
}

fn parse_friction_log(bytes: &[u8]) -> Result<FrictionDocument<'_>, FrictionParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| FrictionParseError {
        class: FrictionParseClass::Changed,
        entry: None,
        reason: "log is not UTF-8",
    })?;
    if text.contains('\r') {
        return Err(FrictionParseError {
            class: FrictionParseClass::Changed,
            entry: None,
            reason: "only LF line endings are valid",
        });
    }
    if !text.ends_with('\n') {
        return Err(FrictionParseError {
            class: FrictionParseClass::Changed,
            entry: None,
            reason: "terminal newline is missing",
        });
    }
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    let Some(first_entry) = lines.iter().position(|line| line.starts_with("## ")) else {
        return Err(FrictionParseError {
            class: FrictionParseClass::Changed,
            entry: None,
            reason: "log has no entry",
        });
    };
    let mut cursor = first_entry;
    let mut entries = Vec::new();
    let mut identities = BTreeSet::new();
    while cursor < lines.len() {
        let entry = entries.len();
        let heading = lines[cursor];
        if !valid_entry_heading(heading) {
            return Err(FrictionParseError::at(
                FrictionParseClass::Changed,
                entry,
                "entry heading is invalid",
            ));
        }
        cursor += 1;
        if lines.get(cursor) != Some(&"\n") {
            return Err(FrictionParseError::at(
                FrictionParseClass::Changed,
                entry,
                "entry heading lacks one blank line",
            ));
        }
        cursor += 1;

        let classification = *lines.get(cursor).ok_or_else(|| {
            FrictionParseError::at(
                FrictionParseClass::Changed,
                entry,
                "classification is missing or invalid",
            )
        })?;
        if !matches!(
            classification,
            "- Classification: `mechanical`\n" | "- Classification: `semantic`\n"
        ) {
            return Err(FrictionParseError::at(
                FrictionParseClass::Changed,
                entry,
                "classification is missing or invalid",
            ));
        }
        if !identities.insert(heading) {
            return Err(FrictionParseError::at(
                FrictionParseClass::DuplicateEntry,
                entry,
                "entry identity/classification is duplicated",
            ));
        }
        cursor += 1;
        let after_classification = take_statuses(&lines, &mut cursor, entry)?;

        let observation = required_field(
            &lines,
            &mut cursor,
            entry,
            "- Observation: ",
            "observation is missing or invalid",
        )?;
        let after_observation = take_statuses(&lines, &mut cursor, entry)?;

        let evidence = required_field(
            &lines,
            &mut cursor,
            entry,
            "- Evidence: ",
            "evidence is missing or invalid",
        )?;
        let after_evidence = take_statuses(&lines, &mut cursor, entry)?;
        let statuses = [after_classification, after_observation, after_evidence];
        let mut seen = Vec::new();
        for status in statuses.iter().flatten() {
            if seen.contains(status) {
                return Err(FrictionParseError::at(
                    FrictionParseClass::InvalidInsertion,
                    entry,
                    "status annotation is duplicated",
                ));
            }
            seen.push(*status);
        }
        entries.push(FrictionEntry {
            heading,
            classification,
            observation,
            evidence,
            statuses,
        });

        if cursor == lines.len() {
            break;
        }
        let line = lines[cursor];
        if line.starts_with("- Classification: ") {
            return Err(FrictionParseError::at(
                FrictionParseClass::DuplicateClassification,
                entry,
                "entry has a second classification",
            ));
        }
        if line.starts_with("- Status") {
            return Err(FrictionParseError::at(
                FrictionParseClass::InvalidInsertion,
                entry,
                "status annotation is malformed",
            ));
        }
        if line != "\n" {
            return Err(FrictionParseError::at(
                FrictionParseClass::InvalidInsertion,
                entry,
                "entry contains an unauthorized line",
            ));
        }
        cursor += 1;
        if cursor == lines.len() {
            return Err(FrictionParseError::at(
                FrictionParseClass::Changed,
                entry,
                "log ends with an extra blank line",
            ));
        }
    }
    Ok(FrictionDocument {
        preamble: lines[..first_entry].to_vec(),
        entries,
    })
}

fn required_field<'a>(
    lines: &[&'a str],
    cursor: &mut usize,
    entry: usize,
    prefix: &str,
    missing: &'static str,
) -> Result<&'a str, FrictionParseError> {
    let line = *lines
        .get(*cursor)
        .ok_or_else(|| FrictionParseError::at(FrictionParseClass::Changed, entry, missing))?;
    if line.starts_with("- Classification: ") {
        return Err(FrictionParseError::at(
            FrictionParseClass::DuplicateClassification,
            entry,
            "entry has a second classification",
        ));
    }
    let value = line
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix('\n'));
    if value.is_none_or(str::is_empty) {
        return Err(FrictionParseError::at(
            FrictionParseClass::Changed,
            entry,
            missing,
        ));
    }
    *cursor += 1;
    Ok(line)
}

fn take_statuses<'a>(
    lines: &[&'a str],
    cursor: &mut usize,
    entry: usize,
) -> Result<Vec<&'a str>, FrictionParseError> {
    let mut statuses = Vec::new();
    while let Some(line) = lines.get(*cursor) {
        if valid_status(line) {
            statuses.push(*line);
            *cursor += 1;
        } else if line.starts_with("- Status") {
            return Err(FrictionParseError::at(
                FrictionParseClass::InvalidInsertion,
                entry,
                "status annotation is malformed",
            ));
        } else {
            break;
        }
    }
    Ok(statuses)
}

fn valid_entry_heading(line: &str) -> bool {
    let Some(body) = line
        .strip_prefix("## ")
        .and_then(|line| line.strip_suffix('\n'))
    else {
        return false;
    };
    let Some((date, issue)) = body.split_once(" — #") else {
        return false;
    };
    valid_date(date) && !issue.is_empty() && issue.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_status(line: &str) -> bool {
    let Some(body) = line
        .strip_prefix("- Status (")
        .and_then(|line| line.strip_suffix('\n'))
    else {
        return false;
    };
    let Some((date, detail)) = body.split_once("): ") else {
        return false;
    };
    valid_date(date) && !detail.is_empty()
}

fn valid_date(date: &str) -> bool {
    is_record_name(&format!("{date}-x.md"))
}

fn statuses_preserved(base: &FrictionEntry<'_>, submitted: &FrictionEntry<'_>) -> bool {
    let all_base: Vec<_> = base.statuses.iter().flatten().copied().collect();
    base.statuses
        .iter()
        .zip(&submitted.statuses)
        .all(|(base_group, submitted_group)| {
            let mut next = 0;
            for status in submitted_group {
                if base_group.get(next) == Some(status) {
                    next += 1;
                } else if all_base.contains(status) {
                    return false;
                }
            }
            next == base_group.len()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn record_name_grammar_is_exact() {
        assert!(is_record_name("2026-07-19-s0-sitrep.md"));
        assert!(is_record_name("2026-12-31-a1.md"));
        for invalid in [
            "README.md",
            "2026-07-19-.md",
            "2026-07-19.md",
            "2026-13-01-topic.md",
            "2026-07-32-topic.md",
            "2026-07-19-Topic.md",
            "2026-07-19-double--hyphen.md",
            "2026-7-19-topic.md",
            "2026-07-19-topic.txt",
            "x026-07-19-topic.md",
        ] {
            assert!(!is_record_name(invalid), "{invalid}");
        }
    }

    #[test]
    fn link_targets_are_extracted() {
        assert_eq!(
            link_targets("[a](x.md) text [b](../y.md#z)"),
            vec!["x.md", "../y.md#z"]
        );
    }

    struct Repo(std::path::PathBuf);
    impl Repo {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "boxology-records-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            let repo = Self(path);
            repo.git(&["init", "-q"]);
            repo.git(&["config", "user.email", "records@example.invalid"]);
            repo.git(&["config", "user.name", "Records Test"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }
        fn git(&self, args: &[&str]) {
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        fn write(&self, path: &str, text: &str) {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        fn read(&self, path: &str) -> String {
            fs::read_to_string(self.0.join(path)).unwrap()
        }
        fn replace_once(&self, anchor: &str, replacement: &str) {
            let before = self.read(FRICTION_LOG);
            assert_eq!(
                before.matches(anchor).count(),
                1,
                "anchor must be exact once"
            );
            let after = before.replacen(anchor, replacement, 1);
            assert_ne!(after, before, "mutation must change the target file");
            self.write(FRICTION_LOG, &after);
        }
        fn commit(&self) {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", "step"]);
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn static_checks_gate_grammar_and_index() {
        let repo = Repo::new();
        repo.write("records/README.md", "[r](2026-07-19-topic.md)\n");
        repo.write("records/2026-07-19-topic.md", "record\n");
        assert!(check(&repo.0, None).unwrap().is_empty());
        repo.write("records/2026-07-20-unindexed.md", "record\n");
        repo.write("records/Bad Name.md", "record\n");
        let problems = check(&repo.0, None).unwrap();
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().any(|p| p.contains("not linked from")));
        assert!(problems.iter().any(|p| p.contains("does not match")));
        repo.write(
            "records/README.md",
            "[r](2026-07-19-topic.md) [g](2026-07-21-ghost.md)\n",
        );
        let problems = check(&repo.0, None).unwrap();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("ghost.md, which does not exist"))
        );
    }

    #[test]
    fn merged_records_are_append_only() {
        let repo = Repo::new();
        repo.write("records/README.md", "[r](2026-07-19-topic.md)\n");
        repo.write("records/2026-07-19-topic.md", "record\n");
        repo.commit();
        repo.write(
            "records/README.md",
            "[r](2026-07-19-topic.md) [n](2026-07-20-next.md)\n",
        );
        repo.write("records/2026-07-20-next.md", "new record\n");
        repo.commit();
        assert!(check(&repo.0, Some("HEAD~1")).unwrap().is_empty());
        repo.write("records/2026-07-19-topic.md", "rewritten\n");
        repo.commit();
        let problems = check(&repo.0, Some("HEAD~2")).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("was modified"));
        repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
        fs::remove_file(repo.0.join("records/2026-07-19-topic.md")).unwrap();
        repo.write("records/README.md", "[n](2026-07-20-next.md)\n");
        repo.commit();
        let problems = check(&repo.0, Some("HEAD~1")).unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("was deleted"));
        assert_eq!(run(&repo.0, Some("not-a-revision")), 2);
    }

    const BASE_FRICTION_LOG: &str = "# Friction log\n\n## 2026-07-26 — #1\n\n- Classification: `mechanical`\n- Observation: immutable first\n- Evidence: issue one\n- Status (2026-07-26): open\n\n## 2026-07-27 — #2\n\n- Classification: `semantic`\n- Observation: immutable second\n- Evidence: issue two\n";

    fn friction_log_fixture(repo: &Repo) {
        repo.write("records/README.md", "[r](2026-07-19-topic.md)\n");
        repo.write("records/2026-07-19-topic.md", "record\n");
        repo.write(FRICTION_LOG, BASE_FRICTION_LOG);
        repo.commit();
        assert!(check(&repo.0, Some("HEAD")).unwrap().is_empty());
    }

    fn assert_friction_mutation_fails(code: &str, detail: &str, anchor: &str, replacement: &str) {
        let repo = Repo::new();
        friction_log_fixture(&repo);
        repo.replace_once(anchor, replacement);
        repo.commit();
        let problems = check(&repo.0, Some("HEAD~1")).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains(code), "{problems:?}");
        assert!(problems[0].contains(detail), "{problems:?}");
    }

    #[test]
    fn friction_log_allows_complete_eof_entry_and_exact_status_annotation() {
        let repo = Repo::new();
        friction_log_fixture(&repo);
        repo.write(
            FRICTION_LOG,
            &format!(
                "{}\n## 2026-07-28 — #3\n\n- Classification: `mechanical`\n- Observation: complete append\n- Evidence: issue three\n",
                repo.read(FRICTION_LOG)
            ),
        );
        let appended = repo.read(FRICTION_LOG);
        assert_eq!(appended.matches("## 2026-07-28 — #3").count(), 1);
        repo.commit();
        assert!(check(&repo.0, Some("HEAD~1")).unwrap().is_empty());

        repo.replace_once(
            "- Observation: immutable first\n",
            "- Observation: immutable first\n- Status (2026-07-28): tracked\n",
        );
        repo.commit();
        assert!(check(&repo.0, Some("HEAD~1")).unwrap().is_empty());
    }

    #[test]
    fn friction_log_rejects_every_established_byte_mutation() {
        for (detail, anchor, replacement) in [
            (
                "established entry byte",
                "- Observation: immutable first\n",
                "- Observation: rewritten first\n",
            ),
            (
                "observation is missing",
                "- Observation: immutable first\n",
                "",
            ),
            (
                "observation is missing",
                "- Observation: immutable first\n- Evidence: issue one\n",
                "- Evidence: issue one\n- Observation: immutable first\n- Evidence: issue one\n",
            ),
            (
                "only LF line endings",
                "- Observation: immutable first\n",
                "- Observation: immutable first\r\n",
            ),
            (
                "terminal newline is missing",
                "- Evidence: issue two\n",
                "- Evidence: issue two",
            ),
        ] {
            assert_friction_mutation_fails(
                FRICTION_LOG_ESTABLISHED_BYTES_CHANGED,
                detail,
                anchor,
                replacement,
            );
        }
    }

    #[test]
    fn friction_log_rejects_unauthorized_and_incomplete_insertions() {
        assert_friction_mutation_fails(
            FRICTION_LOG_INVALID_INSERTION,
            "unauthorized line",
            "- Evidence: issue one\n- Status (2026-07-26): open\n",
            "- Evidence: issue one\n- Note: arbitrary insertion\n- Status (2026-07-26): open\n",
        );
        assert_friction_mutation_fails(
            FRICTION_LOG_DUPLICATE_CLASSIFICATION,
            "second classification",
            "- Classification: `mechanical`\n",
            "- Classification: `mechanical`\n- Classification: `semantic`\n",
        );
        assert_friction_mutation_fails(
            FRICTION_LOG_INVALID_APPENDED_ENTRY,
            "evidence is missing",
            "- Evidence: issue two\n",
            "- Evidence: issue two\n\n## 2026-07-28 — #3\n\n- Classification: `mechanical`\n- Observation: incomplete\n",
        );
    }

    #[test]
    fn friction_log_rejects_duplicate_entry_identity() {
        let repo = Repo::new();
        friction_log_fixture(&repo);
        let entry = "## 2026-07-27 — #2\n\n- Classification: `semantic`\n- Observation: immutable second\n- Evidence: issue two\n";
        repo.replace_once(entry, &format!("{entry}\n{entry}"));
        assert_eq!(repo.read(FRICTION_LOG).matches(entry).count(), 2);
        repo.commit();
        let problems = check(&repo.0, Some("HEAD~1")).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains(FRICTION_LOG_DUPLICATE_ENTRY));
        assert!(problems[0].contains("entry identity/classification is duplicated"));
    }

    #[rustfmt::skip]
    const SOURCE_INVENTORY: &[(&str, &str, &[&str])] = &[
        ("crates/xtask/src/main.rs", include_str!("main.rs"), &["mod records;", "records::run(&root(), base)", "[command] if command == \"records\" => records::run(&root(), None),", "[command, flag, base]\n            if command == \"records\"\n                && flag == \"--base\"\n                && !base.is_empty()\n                && !base.starts_with('-') =>"]),
        ("crates/xtask/src/records.rs", include_str!("records.rs"), &[concat!("fn friction_log_", "problems("), concat!("fn parse_", "friction_log(")]),
    ];

    #[rustfmt::skip]
    fn source_inventory_locked(inventory: &[(&str, &str, &[&str])]) -> bool { inventory.iter().all(|(_, source, anchors)| anchors.iter().all(|anchor| source.matches(anchor).count() == 1)) }

    #[test]
    #[rustfmt::skip]
    fn ci_registers_records_guard_source_inventory() {
        assert!(source_inventory_locked(SOURCE_INVENTORY));
        let (name, source, anchors) = SOURCE_INVENTORY[0];
        for &anchor in &anchors[2..] {
            let removed = source.replacen(anchor, "", 1);
            assert_eq!(removed.matches(anchor).count(), 0);
            assert!(!source_inventory_locked(&[(name, removed.as_str(), anchors), SOURCE_INVENTORY[1]]));
        }
    }

    #[test]
    fn friction_log_rejects_file_deletion_and_invalid_base() {
        let repo = Repo::new();
        friction_log_fixture(&repo);
        fs::remove_file(repo.0.join(FRICTION_LOG)).unwrap();
        repo.commit();
        let problems = check(&repo.0, Some("HEAD~1")).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains(FRICTION_LOG_FILE_DELETED));

        let repo = Repo::new();
        repo.write("records/README.md", "[r](2026-07-19-topic.md)\n");
        repo.write("records/2026-07-19-topic.md", "record\n");
        repo.write(
            FRICTION_LOG,
            "# Friction log\n\n## 2026-07-26 — #1\n\n- Classification: `mechanical`\n- Evidence: missing observation\n",
        );
        repo.commit();
        repo.write("marker", "new head\n");
        repo.commit();
        let problems = check(&repo.0, Some("HEAD~1")).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains(FRICTION_LOG_INVALID_BASE));
        assert!(problems[0].contains("observation is missing"));
    }
}
