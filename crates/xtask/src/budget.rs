use std::path::Path;
use std::process::{Command, Output};
const LIMIT: u64 = 600;
// Bootstrap registry: S7 replaces this with manifest-derived classification (S0 D10).
const DERIVED_OUTPUT_PATHS: &[&str] = &["crates/fixtures/hello/generated/"];
struct Entry {
    path: String,
    added: u64,
    binary: bool,
    excluded: bool,
}
struct Report {
    base: String,
    merge_base: String,
    entries: Vec<Entry>,
}
impl Report {
    fn total(&self) -> u64 {
        self.entries
            .iter()
            .map(|entry| if entry.excluded { 0 } else { entry.added })
            .sum()
    }
    fn failure(&self) -> String {
        let total = self.total();
        let mut counted: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| !entry.excluded && entry.added > 0)
            .collect();
        counted.sort_by(|a, b| b.added.cmp(&a.added).then(a.path.cmp(&b.path)));
        let mut text = format!(
            "budget: FAIL ({total}/{LIMIT} hand-authored added lines)\nBASE: {}\nMB: {}\ncounted files:",
            self.base, self.merge_base
        );
        for entry in counted {
            text.push_str(&format!("\n  {:>4}  {}", entry.added, entry.path));
        }
        text.push_str("\nexcluded paths:");
        for entry in self.entries.iter().filter(|entry| entry.excluded) {
            text.push_str(&format!("\n  {}", entry.path));
        }
        text.push_str("\nbinary paths (0 added lines):");
        for entry in self.entries.iter().filter(|entry| entry.binary) {
            text.push_str(&format!("\n  {}", entry.path));
        }
        text.push_str(
            "\nThe 600-line limit is absolute and has no override; split the PR or re-scope the task.",
        );
        text
    }
}
pub(crate) fn command_result(root: &Path, revision: &str) -> (u8, String) {
    match compute(root, revision) {
        Ok(report) if report.total() <= LIMIT => (
            0,
            format!(
                "budget: PASS ({}/{LIMIT} hand-authored added lines)",
                report.total()
            ),
        ),
        Ok(report) => (1, report.failure()),
        Err(error) => (2, format!("budget: ERROR: {error}")),
    }
}
pub(crate) fn run(root: &Path, revision: &str) -> u8 {
    let (code, message) = command_result(root, revision);
    match code {
        0 => println!("{message}"),
        _ => eprintln!("{message}"),
    }
    code
}
fn compute(root: &Path, revision: &str) -> Result<Report, String> {
    let requested = format!("{revision}^{{commit}}");
    let base = git_text(root, &["rev-parse", "--verify", "--quiet", &requested]).map_err(|_| {
        format!(
            "unknown revision {revision:?} or its commit object is unavailable; {HISTORY_REMEDY}"
        )
    })?;
    let head = git_text(root, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
        .map_err(|_| String::from("cannot resolve HEAD to a commit"))?;
    let merge_base = git_text(root, &["merge-base", &base, &head]).map_err(|_| {
        format!(
            "no merge base exists for BASE {base} and HEAD {head}; {}",
            HISTORY_REMEDY
        )
    })?;
    let output = git(
        root,
        &[
            "-c",
            "diff.renames=true",
            "-c",
            "diff.renameLimit=0",
            "diff",
            "--no-ext-diff",
            "--find-renames",
            "--numstat",
            "-z",
            &merge_base,
            &head,
            "--",
        ],
    )?;
    if !output.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Report {
        base,
        merge_base,
        entries: parse_numstat(&output.stdout)?,
    })
}
pub(crate) const HISTORY_REMEDY: &str = "fetch the missing base object (use `git fetch --unshallow` for a shallow local clone); CI checkout must keep `fetch-depth: 0`";
pub(crate) fn git_text(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git(root, args)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "git returned non-UTF-8 output")?;
    Ok(text.trim().to_string())
}
pub(crate) fn git(root: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))
}
fn parse_numstat(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let mut rest = bytes;
    let mut entries = Vec::new();
    while !rest.is_empty() {
        let (record, next) = take_nul(rest)?;
        rest = next;
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let added = fields.next().ok_or("missing added dimension")?;
        let deleted = fields.next().ok_or("missing deleted dimension")?;
        let mut path = fields.next().ok_or("missing path")?;
        let added = dimension(added)?;
        let deleted = dimension(deleted)?;
        let binary = added.is_none() || deleted.is_none();
        if path.is_empty() {
            let (_, next) = take_nul(rest)?;
            let (postimage, next) = take_nul(next)?;
            path = postimage;
            rest = next;
        }
        let path = std::str::from_utf8(path)
            .map_err(|_| "git path is not UTF-8")?
            .to_string();
        entries.push(Entry {
            excluded: is_excluded(&path, DERIVED_OUTPUT_PATHS),
            path,
            added: if binary { 0 } else { added.unwrap() },
            binary,
        });
    }
    Ok(entries)
}
fn dimension(bytes: &[u8]) -> Result<Option<u64>, String> {
    if bytes == b"-" {
        return Ok(None);
    }
    std::str::from_utf8(bytes)
        .map_err(|_| String::from("non-UTF-8 numstat dimension"))?
        .parse()
        .map(Some)
        .map_err(|_| String::from("invalid numstat dimension"))
}
fn take_nul(bytes: &[u8]) -> Result<(&[u8], &[u8]), String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("unterminated git numstat record")?;
    Ok((&bytes[..end], &bytes[end + 1..]))
}
fn is_excluded(path: &str, derived: &[&str]) -> bool {
    path == "Cargo.lock"
        || derived.iter().any(|entry| {
            entry.strip_suffix('/').map_or(path == *entry, |directory| {
                path == directory || path.starts_with(&format!("{directory}/"))
            })
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Repo(std::path::PathBuf);
    impl Repo {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "boxology-budget-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            let repo = Self(path);
            repo.git(&["init", "-q"]);
            repo.git(&["config", "user.email", "budget@example.invalid"]);
            repo.git(&["config", "user.name", "Budget Test"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }
        fn git(&self, args: &[&str]) -> String {
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
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
            let path = self.0.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, bytes).unwrap();
        }
        fn commit(&self, message: &str) -> String {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "--allow-empty", "-m", message]);
            self.head()
        }
        fn head(&self) -> String {
            self.git(&["rev-parse", "HEAD"])
        }
    }
    fn lines(count: usize) -> String {
        (0..count).map(|n| format!("line {n}\n")).collect()
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn parses_nul_numstat_and_uses_rename_postimage() {
        let entries = parse_numstat(
            b"2\t1\tplain \xC3\xA9 name\0\
              3\t2\t\0old\0Cargo.lock\0\
              -\t-\tblob.bin\0\
              -\t-\t\0old.bin\0new.bin\0",
        )
        .unwrap();
        let got: Vec<_> = entries
            .iter()
            .map(|e| (e.path.as_str(), e.added, e.binary, e.excluded))
            .collect();
        let expected = [
            ("plain é name", 2, false, false),
            ("Cargo.lock", 3, false, true),
            ("blob.bin", 0, true, false),
            ("new.bin", 0, true, false),
        ];
        assert_eq!(got, expected);
    }
    #[test]
    fn exclusions_are_exact_or_directory_prefixes() {
        assert!(DERIVED_OUTPUT_PATHS.contains(&"crates/fixtures/hello/generated/"));
        let derived = ["generated/exact.rs", "generated/tree/"];
        assert!(is_excluded("Cargo.lock", &derived));
        assert!(is_excluded("generated/exact.rs", &derived));
        assert!(is_excluded("generated/tree/nested.rs", &derived));
        assert!(!is_excluded("nested/Cargo.lock", &derived));
        assert!(!is_excluded("generated/treehouse/x", &derived));
        assert!(!is_excluded(
            "crates/fixtures/generated-style-fmt/src/lib.rs",
            &derived
        ));
    }
    #[test]
    fn limit_is_absolute_at_six_hundred() {
        let repo = Repo::new();
        let base = repo.commit("base");
        repo.write("lines.txt", lines(600));
        repo.write("blob.bin", [0, 1, 2]);
        repo.commit("600");
        assert_eq!(
            command_result(&repo.0, &base),
            (
                0,
                String::from("budget: PASS (600/600 hand-authored added lines)")
            )
        );
        repo.write("lines.txt", lines(601));
        repo.commit("601");
        let (code, report) = command_result(&repo.0, &base);
        assert_eq!(code, 1);
        assert_eq!(
            report,
            format!(
                concat!(
                    "budget: FAIL (601/600 hand-authored added lines)\n",
                    "BASE: {base}\n",
                    "MB: {base}\n",
                    "counted files:\n",
                    "   601  lines.txt\n",
                    "excluded paths:\n",
                    "binary paths (0 added lines):\n",
                    "  blob.bin\n",
                    "The 600-line limit is absolute and has no override; split the PR or re-scope the task."
                ),
                base = base
            )
        );
    }
    #[test]
    fn configured_derived_outputs_are_excluded_from_budget() {
        let repo = Repo::new();
        let base = repo.commit("base");
        let path = "crates/fixtures/hello/generated/large.rs";
        repo.write(path, lines(601));
        repo.commit("derived output");

        let report = compute(&repo.0, &base).unwrap();
        assert_eq!(report.total(), 0);
        assert_eq!(report.entries.len(), 1);
        assert_eq!(
            (
                report.entries[0].path.as_str(),
                report.entries[0].added,
                report.entries[0].excluded
            ),
            (path, 601, true)
        );
        assert_eq!(
            report.failure(),
            format!(
                concat!(
                    "budget: FAIL (0/600 hand-authored added lines)\n",
                    "BASE: {base}\n",
                    "MB: {base}\n",
                    "counted files:\n",
                    "excluded paths:\n",
                    "  crates/fixtures/hello/generated/large.rs\n",
                    "binary paths (0 added lines):\n",
                    "The 600-line limit is absolute and has no override; split the PR or re-scope the task."
                ),
                base = base
            )
        );
        assert_eq!(
            command_result(&repo.0, &base),
            (
                0,
                String::from("budget: PASS (0/600 hand-authored added lines)")
            )
        );
    }
    #[test]
    fn rename_lock_binary_and_errors_are_handled() {
        let repo = Repo::new();
        repo.write("old.txt", lines(20));
        let base = repo.commit("base");
        fs::rename(repo.0.join("old.txt"), repo.0.join("new.txt")).unwrap();
        repo.commit("pure rename");
        assert_eq!(compute(&repo.0, &base).unwrap().total(), 0);
        let before_edit = repo.head();
        fs::rename(repo.0.join("new.txt"), repo.0.join("edited.txt")).unwrap();
        repo.write("edited.txt", lines(21));
        repo.commit("rename edit");
        assert_eq!(compute(&repo.0, &before_edit).unwrap().total(), 1);
        let before_ignored = repo.head();
        repo.write("Cargo.lock", "ignored\n");
        repo.commit("lock only");
        let report = compute(&repo.0, &before_ignored).unwrap();
        assert_eq!(report.total(), 0);
        assert!(report.entries[0].excluded && report.entries[0].path == "Cargo.lock");
        let (code, error) = command_result(&repo.0, "not-a-revision");
        assert_eq!(code, 2);
        assert!(error.contains("unknown revision") && error.contains("fetch-depth: 0"));
    }
    #[test]
    fn merge_base_handles_advanced_base_and_ci_merge_commit() {
        let repo = Repo::new();
        let ancestor = repo.commit("ancestor");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.write("feature.txt", "feature\n");
        repo.commit("feature");
        repo.git(&["checkout", "-q", "-b", "base", &ancestor]);
        repo.write("base.txt", "base\n");
        let advanced_base = repo.commit("advanced base");
        repo.git(&["checkout", "-q", "feature"]);
        let local = compute(&repo.0, &advanced_base).unwrap();
        assert_eq!(local.merge_base, ancestor);
        assert_eq!(local.total(), 1);
        repo.git(&["checkout", "-q", "base"]);
        repo.git(&["merge", "-q", "--no-ff", "feature", "-m", "merge ref"]);
        let ci = compute(&repo.0, &advanced_base).unwrap();
        assert_eq!(ci.merge_base, advanced_base);
        assert_eq!(ci.total(), 1);
        repo.git(&["update-ref", "-d", "HEAD"]);
        let missing_head = command_result(&repo.0, &advanced_base);
        assert!(missing_head.1.ends_with("cannot resolve HEAD to a commit"));
    }
}
