use crate::budget;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const DIRECTORY: &str = "records";
const INDEX: &str = "README.md";

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
    Ok(())
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
}
