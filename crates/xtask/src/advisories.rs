use crate::deny::VERSION;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
const TITLE: &str = "[Security] Rust dependency advisories";
pub fn run(root: &Path, repo: &str, simulate: Option<&str>) -> u8 {
    let result: Result<(), String> = (|| {
        validate_repo(repo)?;
        if let Some(id) = simulate {
            validate_advisory(id)?;
        }
        crate::deny::require_version(root)?;
        let mut advisories = cargo_deny(root)?;
        if let Some(id) = simulate {
            advisories.insert(id.into(), "Simulated workflow advisory".into());
        }
        if advisories.is_empty() {
            println!("advisories: PASS (none found)");
            return Ok(());
        }
        let body = render(&advisories);
        let action = upsert(&mut GhApi { repo: repo.into() }, TITLE, &body)?;
        println!("advisories: PASS ({action:?})");
        Ok(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("advisories: FAIL: {error}");
            1
        }
    }
}
fn cargo_deny(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let output = Command::new("cargo")
        .args([
            "deny",
            "--format",
            "json",
            "--locked",
            "check",
            "advisories",
        ])
        .current_dir(root)
        .output()
        .map_err(|error| format!("cannot run cargo-deny: {error}"))?;
    let advisories = parse_json_lines(&output.stderr)?;
    if !matches!(output.status.code(), Some(0 | 1))
        || (!output.status.success() && advisories.is_empty())
    {
        return crate::deny::command_error(output, "cargo-deny");
    }
    Ok(advisories)
}
fn parse_json_lines(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "cargo-deny output is not UTF-8")?;
    let mut found = BTreeMap::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_str(line)
            .map_err(|error| format!("invalid cargo-deny JSON: {error}"))?;
        if value.get("type").and_then(Value::as_str) != Some("diagnostic") {
            continue;
        }
        let Some(id) = value.pointer("/fields/advisory/id").and_then(Value::as_str) else {
            continue;
        };
        let title = value
            .pointer("/fields/advisory/title")
            .and_then(Value::as_str)
            .or_else(|| value.pointer("/fields/message").and_then(Value::as_str))
            .unwrap_or("RustSec advisory");
        found.insert(id.into(), title.into());
    }
    Ok(found)
}
fn render(advisories: &BTreeMap<String, String>) -> String {
    let mut body = format!("Automated `cargo-deny {VERSION}` advisory report.\n\n");
    for (id, title) in advisories {
        body.push_str(&format!("- `{id}`: {title}\n"));
    }
    body.push_str("\nManaged by `cargo xtask advisories`; manual edits are replaced.\n");
    body
}
fn validate_repo(repo: &str) -> Result<(), String> {
    let Some((owner, name)) = repo.split_once('/') else {
        return Err("--repo must be owner/name".into());
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    if !valid(owner) || !valid(name) || name.contains('/') {
        return Err("--repo must be owner/name".into());
    }
    Ok(())
}
fn validate_advisory(id: &str) -> Result<(), String> {
    let valid = id
        .strip_prefix("RUSTSEC-")
        .and_then(|rest| rest.split_once('-'))
        .is_some_and(|(year, sequence)| {
            year.len() == 4
                && sequence.len() == 4
                && year
                    .bytes()
                    .chain(sequence.bytes())
                    .all(|byte| byte.is_ascii_digit())
        });
    if valid {
        Ok(())
    } else {
        Err("--simulate must be RUSTSEC-YYYY-NNNN".into())
    }
}
#[derive(Debug, PartialEq)]
enum Action {
    Created,
    Updated(u64),
    Unchanged(u64),
}
struct Issue {
    number: u64,
    title: String,
    body: String,
    pull_request: bool,
}
trait Api {
    fn list_page(&mut self, page: u32) -> Result<Vec<Issue>, String>;
    fn create(&mut self, title: &str, body: &str) -> Result<(), String>;
    fn update(&mut self, number: u64, body: &str) -> Result<(), String>;
}
fn upsert(api: &mut impl Api, title: &str, body: &str) -> Result<Action, String> {
    let mut found = None;
    for page_number in 1..=100 {
        let issues = api.list_page(page_number)?;
        let has_next = issues.len() == 100;
        for issue in issues {
            if !issue.pull_request && issue.title == title && found.replace(issue).is_some() {
                return Err(format!("multiple open issues have exact title {title:?}"));
            }
        }
        if !has_next {
            return match found {
                None => {
                    api.create(title, body)?;
                    Ok(Action::Created)
                }
                Some(issue) if issue.body == body => Ok(Action::Unchanged(issue.number)),
                Some(issue) => {
                    api.update(issue.number, body)?;
                    Ok(Action::Updated(issue.number))
                }
            };
        }
    }
    Err("GitHub issue pagination exceeded 100 pages".into())
}
struct GhApi {
    repo: String,
}

impl GhApi {
    fn call(&self, method: &str, endpoint: &str, fields: &[(&str, &str)]) -> Result<Value, String> {
        let mut command = Command::new("gh");
        command.args(["api", "--method", method, endpoint]);
        for (name, value) in fields {
            command.args(["--raw-field", &format!("{name}={value}")]);
        }
        let output = command
            .output()
            .map_err(|error| format!("cannot run gh: {error}"))?;
        if !output.status.success() {
            return crate::deny::command_error(output, "gh api");
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("invalid GitHub JSON: {error}"))
    }
}

impl Api for GhApi {
    fn list_page(&mut self, page: u32) -> Result<Vec<Issue>, String> {
        let endpoint = format!(
            "/repos/{}/issues?state=open&per_page=100&page={page}",
            self.repo
        );
        let value = self.call("GET", &endpoint, &[])?;
        let array = value
            .as_array()
            .ok_or("GitHub issue response is not an array")?;
        let mut issues = Vec::new();
        for value in array {
            issues.push(Issue {
                number: value["number"].as_u64().ok_or("issue number is missing")?,
                title: value["title"]
                    .as_str()
                    .ok_or("issue title is missing")?
                    .into(),
                body: value["body"].as_str().unwrap_or("").into(),
                pull_request: value.get("pull_request").is_some(),
            });
        }
        Ok(issues)
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        self.call(
            "POST",
            &format!("/repos/{}/issues", self.repo),
            &[("title", title), ("body", body)],
        )?;
        Ok(())
    }

    fn update(&mut self, number: u64, body: &str) -> Result<(), String> {
        self.call(
            "PATCH",
            &format!("/repos/{}/issues/{number}", self.repo),
            &[("body", body)],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Fake {
        pages: VecDeque<Vec<Issue>>,
        creates: Vec<(String, String)>,
        updates: Vec<(u64, String)>,
    }

    impl Api for Fake {
        fn list_page(&mut self, _: u32) -> Result<Vec<Issue>, String> {
            self.pages.pop_front().ok_or("unexpected page".into())
        }
        fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
            self.creates.push((title.into(), body.into()));
            Ok(())
        }
        fn update(&mut self, number: u64, body: &str) -> Result<(), String> {
            self.updates.push((number, body.into()));
            Ok(())
        }
    }

    fn issue(number: u64, title: &str, body: &str, pull_request: bool) -> Issue {
        Issue {
            number,
            title: title.into(),
            body: body.into(),
            pull_request,
        }
    }

    #[test]
    fn parses_captured_cargo_deny_json() {
        let fixture = include_bytes!("../fixtures/cargo-deny-advisory.jsonl");
        let found = parse_json_lines(fixture).unwrap();
        assert_eq!(
            found.get("RUSTSEC-2020-0071").unwrap(),
            "Potential segfault in the time crate"
        );
        let workflow = include_str!("../../../.github/workflows/advisories.yml");
        let pins: Vec<_> = workflow
            .lines()
            .filter_map(|line| line.trim().strip_prefix("CARGO_DENY_VERSION: "))
            .collect();
        assert_eq!(pins, [format!("\"{VERSION}\"")]);
    }

    #[test]
    fn creates_when_no_exact_issue_exists() {
        let mut fake = Fake::default();
        fake.pages.push_back(vec![issue(1, "similar", "", false)]);
        assert_eq!(upsert(&mut fake, TITLE, "body"), Ok(Action::Created));
        assert_eq!(fake.creates, vec![(TITLE.into(), "body".into())]);
        assert!(fake.updates.is_empty());
    }

    #[test]
    fn paginates_filters_prs_updates_and_avoids_body_churn() {
        let mut first: Vec<_> = (0..99)
            .map(|number| issue(number, "other", "", false))
            .collect();
        first.push(issue(100, TITLE, "body", true));
        let second = vec![issue(7, TITLE, "old", false)];
        let mut update = Fake {
            pages: [first, second].into(),
            ..Fake::default()
        };
        assert_eq!(upsert(&mut update, TITLE, "body"), Ok(Action::Updated(7)));
        assert_eq!(update.updates, vec![(7, "body".into())]);

        let mut unchanged = Fake::default();
        unchanged
            .pages
            .push_back(vec![issue(7, TITLE, "body", false)]);
        assert_eq!(
            upsert(&mut unchanged, TITLE, "body"),
            Ok(Action::Unchanged(7))
        );
        assert!(unchanged.creates.is_empty() && unchanged.updates.is_empty());
        assert!(validate_repo("owner/repo.name").is_ok());
        assert!(validate_repo("owner/repo/extra").is_err());
        assert!(validate_advisory("RUSTSEC-2026-0123").is_ok());
        assert!(validate_advisory("RUSTSEC-latest").is_err());
    }
}
