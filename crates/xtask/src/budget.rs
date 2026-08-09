use boxology_manifest::{GlobPattern, Manifest, RelativePath};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use toml_edit::{DocumentMut, Item};
const LIMIT: u64 = 600;
#[derive(Clone)]
struct DerivedPattern {
    base: String,
    pattern: GlobPattern,
}
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
    let derived = derived_output_patterns(root)?;
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
        entries: parse_numstat(&output.stdout, &derived)?,
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
fn parse_numstat(bytes: &[u8], derived: &[DerivedPattern]) -> Result<Vec<Entry>, String> {
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
            excluded: is_excluded(&path, derived),
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
fn is_excluded(path: &str, derived: &[DerivedPattern]) -> bool {
    // Every Cargo.lock is derived: cargo writes it, nobody hand-authors it. The
    // rule was root-only while no nested workspace existed; fixture projects own
    // their own workspaces now, so a nested lockfile is as derived as the root's.
    path == "Cargo.lock"
        || path.ends_with("/Cargo.lock")
        || derived.iter().any(|entry| {
            let relative = if entry.base.is_empty() {
                Some(path)
            } else {
                path.strip_prefix(&entry.base)
                    .and_then(|rest| rest.strip_prefix('/'))
            };
            relative
                .and_then(|relative| RelativePath::new(relative.to_owned()).ok())
                .is_some_and(|relative| entry.pattern.matches(&relative))
        })
}

fn derived_output_patterns(root: &Path) -> Result<Vec<DerivedPattern>, String> {
    let root_path = PathBuf::from("boxology.toml");
    let root_manifest = read_manifest(root, &root_path)?;
    let mut manifests = BTreeSet::from([root_path]);
    for fixture in root_manifest.fixtures() {
        let Some(project_name) = fixture.as_str().strip_suffix("/**") else {
            continue;
        };
        let project = root.join(project_name);
        let cargo_path = project.join("Cargo.toml");
        let manifest_path = project.join("boxology.toml");
        if !cargo_path.is_file() || !manifest_path.is_file() {
            continue;
        }
        manifests.insert(PathBuf::from(project_name).join("boxology.toml"));
        for member in cargo_workspace_members(&cargo_path)? {
            let member = Path::new(&member);
            if member.is_absolute()
                || member
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(format!(
                    "invalid workspace member in {}",
                    cargo_path.display()
                ));
            }
            let mut directory = project.join(member);
            loop {
                let candidate = directory.join("boxology.toml");
                if candidate.is_file() {
                    let relative = candidate.strip_prefix(root).map_err(|_| {
                        format!("manifest escaped repository: {}", candidate.display())
                    })?;
                    manifests.insert(relative.to_owned());
                    break;
                }
                if directory == project || !directory.pop() || !directory.starts_with(&project) {
                    break;
                }
            }
        }
    }
    let mut patterns = Vec::new();
    for path in manifests {
        let manifest = read_manifest(root, &path)?;
        let base = path
            .parent()
            .and_then(Path::to_str)
            .unwrap_or_default()
            .to_owned();
        for output in manifest.derived() {
            for pattern in output.outputs() {
                patterns.push(DerivedPattern {
                    base: base.clone(),
                    pattern: pattern.clone(),
                });
            }
        }
    }
    Ok(patterns)
}

fn read_manifest(root: &Path, path: &Path) -> Result<Manifest, String> {
    let logical = path
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 manifest path: {}", path.display()))?;
    let at = RelativePath::new(logical.to_owned())
        .map_err(|_| format!("invalid manifest path: {logical}"))?;
    let bytes =
        fs::read(root.join(path)).map_err(|error| format!("cannot read {logical}: {error}"))?;
    Manifest::parse(at, &bytes).map_err(|diagnostics| format!("invalid {logical}: {diagnostics}"))
}

fn cargo_workspace_members(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let document = text
        .parse::<DocumentMut>()
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
    let Some(members) = document
        .get("workspace")
        .and_then(Item::as_table)
        .and_then(|workspace| workspace.get("members"))
        .and_then(Item::as_array)
    else {
        return Ok(Vec::new());
    };
    members
        .iter()
        .map(|member| {
            member
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("non-string workspace member in {}", path.display()))
        })
        .collect()
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
            repo.write("Cargo.toml", "[workspace]\nmembers = []\n");
            repo.write(
                "boxology.toml",
                "schema = 1\nid = \"test\"\nkind = \"platform\"\nowned = [\"**\"]\n",
            );
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
            &[],
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
    fn cargo_locks_are_the_only_non_manifest_exclusion() {
        let derived = [];
        assert!(is_excluded("Cargo.lock", &derived));
        assert!(is_excluded("nested/Cargo.lock", &derived));
        assert!(!is_excluded("nested/Cargo.locked", &derived));
        assert!(!is_excluded(
            "goldens/generated-project/Cargo.toml",
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
        repo.write(
            "boxology.toml",
            concat!(
                "schema = 1\nid = \"test\"\nkind = \"platform\"\nowned = [\"**\"]\n",
                "fixtures = [\"crates/fixtures/hello/**\"]\n"
            ),
        );
        repo.write(
            "crates/fixtures/hello/Cargo.toml",
            "[workspace]\nmembers = [\"implementation\", \"generated/contract\"]\n",
        );
        repo.write(
            "crates/fixtures/hello/boxology.toml",
            concat!(
                "schema = 1\nid = \"hello\"\nkind = \"box\"\nowned = [\"boxology.toml\", \"implementation/**\"]\n",
                "[[derived]]\nid = \"contract\"\ngenerator = \"boxology-contract\"\n",
                "inputs = [\"boxology.toml\"]\noutputs = [\"generated/**\"]\n"
            ),
        );
        let base = repo.commit("base");
        let path = "crates/fixtures/hello/generated/large.rs";
        repo.write(path, lines(601));
        repo.write("goldens/generated-project/Cargo.toml", "authored\n");
        repo.commit("derived output");

        let report = compute(&repo.0, &base).unwrap();
        assert_eq!(report.total(), 1);
        assert_eq!(report.entries.len(), 2);
        let derived = report
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap();
        assert_eq!(
            (derived.path.as_str(), derived.added, derived.excluded),
            (path, 601, true)
        );
        assert_eq!(
            report.failure(),
            format!(
                concat!(
                    "budget: FAIL (1/600 hand-authored added lines)\n",
                    "BASE: {base}\n",
                    "MB: {base}\n",
                    "counted files:\n",
                    "     1  goldens/generated-project/Cargo.toml\n",
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
                String::from("budget: PASS (1/600 hand-authored added lines)")
            )
        );

        repo.write(
            "crates/fixtures/hello/boxology.toml",
            "schema = 1\nid = \"hello\"\nkind = \"box\"\nowned = [\"**\"]\n",
        );
        repo.commit("derived declaration removed");
        let report = compute(&repo.0, &base).unwrap();
        assert!(
            report
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .is_some_and(|entry| !entry.excluded)
        );
        assert!(report.total() > 600);
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
