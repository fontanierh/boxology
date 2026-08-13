use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::{fs, path::Path};
use toml_edit::{DocumentMut, Item, TableLike};

const REPOSITORY: &str = "https://github.com/fontanierh/boxology";
const APACHE_SHA256: &str = "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30";
const MIT_SHA256: &str = "c6c7cd67105c326b5aa22c9285cf80e54f654e5203891fe1f0c65c567da5dbcb";
const CONTACT: &str = "https://github.com/fontanierh/boxology/security/advisories/new";
const COMMUNITY_FILES: &[&str] = &[
    "CONTRIBUTING.md",
    "SECURITY.md",
    "CODE_OF_CONDUCT.md",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    ".github/ISSUE_TEMPLATE/feature_request.yml",
    ".github/ISSUE_TEMPLATE/config.yml",
    ".github/pull_request_template.md",
    ".github/dependabot.yml",
];
const YAML_FILES: &[&str] = &[
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    ".github/ISSUE_TEMPLATE/feature_request.yml",
    ".github/ISSUE_TEMPLATE/config.yml",
    ".github/dependabot.yml",
    ".github/workflows/ci.yml",
];

#[rustfmt::skip]
const PRODUCTS: &[(&str, &str, &str)] = &[
    ("crates/boxology", "boxology", "Authoring facade"),
    ("crates/boxology-classifier", "boxology-classifier", "Compatibility classifier"),
    ("crates/boxology-cli", "boxology-cli", "Command-line interface"),
    ("crates/boxology-cli-core", "boxology-cli-core", "Reusable command-line boundary"),
    ("crates/boxology-contract", "boxology-contract", "Core identifiers, descriptors, and call types"),
    ("crates/boxology-contract-syntax", "boxology-contract-syntax", "Controlled contract parser and model"),
    ("crates/boxology-generator", "boxology-generator", "Deterministic generator"),
    ("crates/boxology-generator-model", "boxology-generator-model", "Pure generation planning"),
    ("crates/boxology-generator-writer", "boxology-generator-writer", "Filesystem publication"),
    ("crates/boxology-http", "boxology-http", "HTTP transport"),
    ("crates/boxology-init", "boxology-init", "Project initializer"),
    ("crates/boxology-macros", "boxology-macros", "Procedural macros"),
    ("crates/boxology-manifest", "boxology-manifest", "Strict manifest parser and model"),
    ("crates/boxology-runtime", "boxology-runtime", "Runtime composition and invocation"),
    ("crates/boxology-schema", "boxology-schema", "Canonical schema model and serializer"),
    ("crates/boxology-workspace", "boxology-workspace", "Pure workspace validation"),
];
#[rustfmt::skip]
const INTERNAL: &[(&str, &str)] = &[
    ("crates/boxology-classifier/generated/contract", "classifier-contract"),
    ("crates/boxology-classifier/implementation", "classifier-implementation"),
    ("crates/boxology-check/generated/contract", "check-contract"),
    ("crates/boxology-check/implementation", "check-implementation"),
    ("crates/boxology-http-conformance", "boxology-http-conformance"),
    ("crates/fixtures/fixture-tests", "boxology-fixture-tests"),
    ("crates/xtask", "xtask"),
];

#[rustfmt::skip]
pub(crate) fn run(root: &Path) -> u8 {
    match check(root) {
        Ok(()) => 0,
        Err(errors) => { for error in errors { eprintln!("oss-metadata: {error}"); } 1 }
    }
}

#[rustfmt::skip]
fn document(root: &Path, relative: &str) -> Result<DocumentMut, String> {
    fs::read_to_string(root.join(relative)).map_err(|e| format!("read {relative}: {e}"))?
        .parse().map_err(|e| format!("parse {relative}: {e}"))
}

fn string<'a>(table: &'a dyn TableLike, key: &str) -> Option<&'a str> {
    table.get(key)?.as_str()
}

#[rustfmt::skip]
fn inherited(table: &dyn TableLike, key: &str) -> bool {
    table.get(key).and_then(Item::as_table_like)
        .and_then(|value| value.get("workspace")).and_then(Item::as_bool) == Some(true)
}

#[rustfmt::skip]
fn check_readme(readme: &str, errors: &mut Vec<String>) {
    for truth in ["early-stage framework", "V0 was completed on 2026-08-09",
        "Applications built with Boxology are separate products and are not included in this repository.",
        "unpublished development packages at version `0.0.0`", "cargo install --git https://github.com/fontanierh/boxology",
        "requires Rust 1.97.1", "dual-licensed under [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE)"] {
        if !readme.contains(truth) { errors.push(format!("README.md: missing required truth {truth:?}")); }
    }
    for stale in ["published on crates.io", "cargo install boxology", "committed flagship application", "current product critical path"] {
        if readme.contains(stale) { errors.push(format!("README.md: stale claim {stale:?}")); }
    }
}

fn require(relative: &str, text: &str, fragments: &[&str], errors: &mut Vec<String>) {
    for fragment in fragments {
        if !text.contains(fragment) {
            errors.push(format!("{relative}: missing required content {fragment:?}"));
        }
    }
    let lower = text.to_ascii_lowercase();
    for placeholder in [
        "todo",
        "tbd",
        "<insert",
        "insert here",
        "your email",
        "your contact",
    ] {
        if lower
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '<'))
            .any(|word| word == placeholder)
            || lower.contains(placeholder)
        {
            errors.push(format!(
                "{relative}: placeholder {placeholder:?} is forbidden"
            ));
        }
    }
}

fn key<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.as_mapping()?.get(Value::String(name.into()))
}

fn issue_form(value: &Value, name: &str, label: &str, ids: &[&str]) -> bool {
    if key(value, "name").and_then(Value::as_str) != Some(name)
        || !key(value, "labels")
            .and_then(Value::as_sequence)
            .is_some_and(|labels| labels.iter().any(|value| value.as_str() == Some(label)))
    {
        return false;
    }
    let Some(body) = key(value, "body").and_then(Value::as_sequence) else {
        return false;
    };
    ids.iter().all(|id| {
        body.iter().any(|item| {
            key(item, "id").and_then(Value::as_str) == Some(id)
                && key(item, "validations")
                    .and_then(|v| key(v, "required"))
                    .and_then(Value::as_bool)
                    == Some(true)
        })
    })
}

fn dependabot(value: &Value) -> bool {
    if key(value, "version").and_then(Value::as_u64) != Some(2) {
        return false;
    }
    let Some(updates) = key(value, "updates").and_then(Value::as_sequence) else {
        return false;
    };
    updates.len() == 2
        && ["cargo", "github-actions"].iter().all(|ecosystem| {
            updates
                .iter()
                .filter(|entry| {
                    key(entry, "package-ecosystem").and_then(Value::as_str) == Some(ecosystem)
                })
                .count()
                == 1
                && updates.iter().any(|entry| {
                    key(entry, "package-ecosystem").and_then(Value::as_str) == Some(ecosystem)
                        && key(entry, "directory").and_then(Value::as_str) == Some("/")
                        && key(entry, "target-branch").and_then(Value::as_str) == Some("main")
                        && key(entry, "open-pull-requests-limit").and_then(Value::as_u64) == Some(5)
                        && key(entry, "schedule")
                            .and_then(|v| key(v, "interval"))
                            .and_then(Value::as_str)
                            == Some("weekly")
                })
        })
}

fn check_yaml(root: &Path, errors: &mut Vec<String>) {
    for relative in YAML_FILES {
        let value: Value = match fs::read_to_string(root.join(relative))
            .map_err(|e| e.to_string())
            .and_then(|text| serde_yaml::from_str(&text).map_err(|e| e.to_string()))
        {
            Ok(value) => value,
            Err(error) => {
                errors.push(format!("{relative}: invalid YAML: {error}"));
                continue;
            }
        };
        let valid = match *relative {
            ".github/ISSUE_TEMPLATE/bug_report.yml" => issue_form(
                &value,
                "Bug report",
                "bug",
                &["revision", "problem", "reproduce", "environment"],
            ),
            ".github/ISSUE_TEMPLATE/feature_request.yml" => issue_form(
                &value,
                "Feature request",
                "enhancement",
                &["problem", "proposal", "alternatives", "impact"],
            ),
            ".github/ISSUE_TEMPLATE/config.yml" => {
                key(&value, "blank_issues_enabled").and_then(Value::as_bool) == Some(false)
                    && key(&value, "contact_links")
                        .and_then(Value::as_sequence)
                        .is_some_and(|links| {
                            links.len() == 1
                                && key(&links[0], "url").and_then(Value::as_str) == Some(CONTACT)
                        })
            }
            ".github/dependabot.yml" => dependabot(&value),
            ".github/workflows/ci.yml" => {
                key(&value, "name").and_then(Value::as_str) == Some("ci")
                    && key(&value, "permissions")
                        .and_then(|v| key(v, "contents"))
                        .and_then(Value::as_str)
                        == Some("read")
                    && key(&value, "jobs")
                        .and_then(Value::as_mapping)
                        .is_some_and(|jobs| jobs.contains_key(Value::String("validate".into())))
            }
            _ => unreachable!(),
        };
        if !valid {
            errors.push(format!(
                "{relative}: parsed structure violates the required schema"
            ));
        }
    }
}

fn check_community(root: &Path, errors: &mut Vec<String>) {
    let expected_templates: BTreeSet<_> = ["bug_report.yml", "feature_request.yml", "config.yml"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    match fs::read_dir(root.join(".github/ISSUE_TEMPLATE")) {
        Err(error) => errors.push(format!("read .github/ISSUE_TEMPLATE: {error}")),
        Ok(entries) => {
            let actual: BTreeSet<_> = entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect();
            if actual != expected_templates {
                errors.push(".github/ISSUE_TEMPLATE: inventory is not exact".into());
            }
        }
    }
    for relative in COMMUNITY_FILES {
        let text = match fs::read_to_string(root.join(relative)) {
            Ok(text) => text,
            Err(error) => {
                errors.push(format!("read {relative}: {error}"));
                continue;
            }
        };
        require(relative, &text, &[], errors);
        match *relative {
            "CONTRIBUTING.md" => require(
                relative,
                &text,
                &[
                    "cargo xtask ci --no-budget",
                    "cargo xtask budget --base origin/main",
                    "600 hand-authored added lines",
                    "MIT OR Apache-2.0",
                    "SECURITY.md",
                ],
                errors,
            ),
            "SECURITY.md" => require(
                relative,
                &text,
                &[
                    "version `0.0.0`",
                    "latest `main` branch",
                    "Do not disclose",
                    CONTACT,
                    "enabled before the repository becomes public",
                    "accepts Code of Conduct reports",
                ],
                errors,
            ),
            "CODE_OF_CONDUCT.md" => require(
                relative,
                &text,
                &[
                    "Contributor Covenant",
                    "version 2.1",
                    "Enforcement Guidelines",
                    CONTACT,
                    "reported confidentially",
                    "enabled before the repository becomes public",
                ],
                errors,
            ),
            ".github/ISSUE_TEMPLATE/bug_report.yml" => {
                require(relative, &text, &["SECURITY.md"], errors)
            }
            ".github/ISSUE_TEMPLATE/feature_request.yml" => {}
            ".github/ISSUE_TEMPLATE/config.yml" => require(relative, &text, &[CONTACT], errors),
            ".github/pull_request_template.md" => require(
                relative,
                &text,
                &[
                    "## Summary",
                    "## Contract and compatibility",
                    "cargo xtask ci --no-budget",
                    "cargo xtask budget --base origin/main",
                    "no credentials",
                ],
                errors,
            ),
            ".github/dependabot.yml" => {}
            _ => unreachable!(),
        }
    }
    check_yaml(root, errors);
}

#[rustfmt::skip]
fn check(root: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let root_doc = document(root, "Cargo.toml").map_err(|e| vec![e])?;
    let Some(workspace) = root_doc.get("workspace").and_then(Item::as_table_like)
        else { return Err(vec!["Cargo.toml: missing [workspace]".into()]); };
    let Some(package) = workspace.get("package").and_then(Item::as_table_like)
        else { return Err(vec!["Cargo.toml: missing [workspace.package]".into()]); };
    let expected = [("version", "0.0.0"), ("edition", "2024"), ("rust-version", "1.97.1"),
        ("license", "MIT OR Apache-2.0"), ("repository", REPOSITORY),
        ("homepage", REPOSITORY), ("readme", "README.md")];
    let keys: BTreeSet<_> = package.iter().map(|(key, _)| key).collect();
    let expected_keys: BTreeSet<_> = expected.iter().map(|(key, _)| *key).collect();
    if keys != expected_keys { errors.push("Cargo.toml: [workspace.package] keys are not exact".into()); }
    for (key, value) in expected {
        if string(package, key) != Some(value) { errors.push(format!("Cargo.toml: workspace {key} must be {value:?}")); }
    }
    let members: BTreeSet<_> = workspace.get("members").and_then(Item::as_array)
        .map(|items| items.iter().filter_map(|item| item.as_str()).collect())
        .unwrap_or_default();
    let classified: BTreeSet<_> = PRODUCTS.iter().map(|(path, _, _)| *path)
        .chain(INTERNAL.iter().map(|(path, _)| *path))
        .collect();
    if members != classified { errors.push("Cargo.toml: workspace members are not exactly classified as product or internal".into()); }

    for &(path, name, description) in PRODUCTS {
        match document(root, &format!("{path}/Cargo.toml")) {
            Err(error) => errors.push(error),
            Ok(doc) => {
                let Some(pkg) = doc.get("package").and_then(Item::as_table_like)
                    else { errors.push(format!("{path}: missing [package]")); continue; };
                let keys: BTreeSet<_> = pkg.iter().map(|(key, _)| key).collect();
                let expected: BTreeSet<_> = ["name", "description", "version", "edition", "rust-version", "license", "repository", "homepage", "readme", "publish"].into_iter().collect();
                if keys != expected { errors.push(format!("{path}: [package] keys are not exact")); }
                if string(pkg, "name") != Some(name) { errors.push(format!("{path}: package name must be {name}")); }
                if string(pkg, "description") != Some(description) { errors.push(format!("{path}: description must be {description:?}")); }
                for key in ["version", "edition", "rust-version", "license", "repository", "homepage", "readme"] {
                    if !inherited(pkg, key) { errors.push(format!("{path}: {key} must inherit workspace metadata")); }
                }
                if pkg.get("publish").and_then(Item::as_bool) != Some(false) { errors.push(format!("{path}: publish must be false")); }
                if pkg.get("authors").is_some() { errors.push(format!("{path}: authors are forbidden")); }
            }
        }
    }
    for &(path, name) in INTERNAL {
        match document(root, &format!("{path}/Cargo.toml")) {
            Err(error) => errors.push(error),
            Ok(doc) => {
                let Some(pkg) = doc.get("package").and_then(Item::as_table_like)
                    else { errors.push(format!("{path}: missing [package]")); continue; };
                if string(pkg, "name") != Some(name) { errors.push(format!("{path}: internal package name must be {name}")); }
                if string(pkg, "version") != Some("0.0.0") { errors.push(format!("{path}: internal version must be 0.0.0")); }
                if pkg.get("publish").and_then(Item::as_bool) != Some(false) { errors.push(format!("{path}: internal publish must be false")); }
            }
        }
    }
    for (file, digest) in [("LICENSE-APACHE", APACHE_SHA256), ("LICENSE-MIT", MIT_SHA256)] {
        match fs::read(root.join(file)) {
            Ok(bytes) if format!("{:x}", Sha256::digest(&bytes)) == digest => {}
            Ok(_) => errors.push(format!("{file}: bytes are not canonical")),
            Err(error) => errors.push(format!("read {file}: {error}")),
        }
    }
    match fs::read_to_string(root.join("README.md")) {
        Err(error) => errors.push(format!("read README.md: {error}")),
        Ok(readme) => check_readme(&readme, &mut errors),
    }
    check_community(root, &mut errors);
    errors.is_empty().then_some(()).ok_or(errors)
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*; use std::path::PathBuf;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("boxology-oss-metadata-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path); fs::create_dir(&path).unwrap();
            let live = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            for file in ["Cargo.toml", "README.md", "LICENSE-MIT", "LICENSE-APACHE"]
                .into_iter().chain(PRODUCTS.iter().map(|(p, _, _)| *p)).chain(INTERNAL.iter().map(|(p, _)| *p)) {
                let relative = if file.starts_with("crates/") { format!("{file}/Cargo.toml") } else { file.into() };
                let target = path.join(&relative); fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::copy(live.join(&relative), target).unwrap();
            }
            for relative in COMMUNITY_FILES.iter().chain(YAML_FILES) {
                let target = path.join(relative); fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::copy(live.join(relative), target).unwrap();
            }
            Self(path)
        }
        fn remove(&self, file: &str) { fs::remove_file(self.0.join(file)).unwrap(); }
        fn mutate(&self, file: &str, from: &str, to: &str) {
            let path = self.0.join(file); let bytes = fs::read_to_string(&path).unwrap();
            assert!(bytes.contains(from)); fs::write(path, bytes.replacen(from, to, 1)).unwrap();
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    #[test]
    fn live_metadata_passes_and_required_mutations_fail_closed() {
        assert_eq!(run(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")), 0);
        for (file, from, to) in [
            ("crates/boxology/Cargo.toml", "publish = false", "publish = true"),
            ("crates/boxology/Cargo.toml", "publish = false", "publish = false\nlicense-file = \"LICENSE\""),
            ("Cargo.toml", "MIT OR Apache-2.0", "MIT"),
            ("crates/boxology/Cargo.toml", "Authoring facade", "Facade"),
            ("Cargo.toml", REPOSITORY, "https://example.invalid/boxology"),
            ("Cargo.toml", "\"crates/xtask\",", "\"crates/xtask\",\n    \"crates/unknown\","),
            ("crates/xtask/Cargo.toml", "publish = false", "publish = true"),
            ("crates/xtask/Cargo.toml", "version = \"0.0.0\"", "version = \"1.2.3\""),
            ("LICENSE-MIT", "Henry Fontanier", "H. Fontanier"),
            ("README.md", "unpublished development packages", "published crates"),
        ] {
            let fixture = Fixture::new(); fixture.mutate(file, from, to);
            assert_eq!(run(&fixture.0), 1, "mutation survived: {file} {from}");
        }
        let fixture = Fixture::new(); fixture.remove("SECURITY.md"); assert_eq!(run(&fixture.0), 1);
        let fixture = Fixture::new(); fixture.mutate("CODE_OF_CONDUCT.md", "Contributor Covenant", "TODO"); assert_eq!(run(&fixture.0), 1);
        let fixture = Fixture::new(); fixture.mutate("SECURITY.md", CONTACT, "https://example.invalid/report"); assert_eq!(run(&fixture.0), 1);
        let fixture = Fixture::new(); fixture.mutate(".github/ISSUE_TEMPLATE/bug_report.yml", "name: Bug report", "name: ["); assert_eq!(run(&fixture.0), 1);
        for ecosystem in ["cargo", "github-actions"] {
            let fixture = Fixture::new();
            fixture.mutate(".github/dependabot.yml", &format!("package-ecosystem: {ecosystem}"), "package-ecosystem: missing");
            assert_eq!(run(&fixture.0), 1, "missing dependabot entry survived: {ecosystem}");
        }
    }
}
