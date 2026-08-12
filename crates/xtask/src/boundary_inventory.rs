use std::{collections::BTreeSet, fs, path::Path};

use toml_edit::{DocumentMut, Item};

const MEMBERS: &[(&str, &str)] = &[
    ("crates/boxology-contract", "boxology-contract"),
    (
        "crates/boxology-contract-syntax",
        "boxology-contract-syntax",
    ),
    ("crates/boxology", "boxology"),
    ("crates/boxology-macros", "boxology-macros"),
    ("crates/boxology-generator", "boxology-generator"),
    (
        "crates/boxology-generator-model",
        "boxology-generator-model",
    ),
    (
        "crates/boxology-generator-writer",
        "boxology-generator-writer",
    ),
    ("crates/boxology-manifest", "boxology-manifest"),
    ("crates/boxology-init", "boxology-init"),
    ("crates/boxology-schema", "boxology-schema"),
    ("crates/boxology-classifier", "boxology-classifier"),
    (
        "crates/boxology-classifier/implementation",
        "classifier-implementation",
    ),
    (
        "crates/boxology-classifier/generated/contract",
        "classifier-contract",
    ),
    ("crates/boxology-check/generated/contract", "check-contract"),
    (
        "crates/boxology-check/implementation",
        "check-implementation",
    ),
    ("crates/boxology-workspace", "boxology-workspace"),
    ("crates/boxology-cli", "boxology-cli"),
    ("crates/boxology-cli-core", "boxology-cli-core"),
    ("crates/boxology-http", "boxology-http"),
    (
        "crates/boxology-http-conformance",
        "boxology-http-conformance",
    ),
    ("crates/boxology-runtime", "boxology-runtime"),
    ("crates/fixtures/fixture-tests", "boxology-fixture-tests"),
    ("crates/xtask", "xtask"),
];
const DEPENDENCIES: &[&str] = &[
    "boxology-classifier",
    "boxology-cli-core",
    "boxology-contract",
    "boxology-manifest",
    "boxology-runtime",
    "boxology-schema",
    "boxology-workspace",
    "check-implementation",
    "check_contract",
    "classifier-implementation",
    "classifier_contract",
    "serde_json",
    "serde",
];
const DEV_DEPENDENCIES: &[&str] = &["check_contract", "classifier_contract", "syn"];
const SKILLS: &[&str] = &["boxology"];
const PRIVATE_ROOTS: &[&str] = &["ops", "records"];
const ROOT_ENTRIES: &[&str] = &[
    ".agents",
    ".cargo",
    ".claude",
    ".editorconfig",
    ".gitattributes",
    ".github",
    ".gitignore",
    "AGENTS.md",
    "Cargo.lock",
    "Cargo.toml",
    "CODE_OF_CONDUCT.md",
    "CONTRIBUTING.md",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "README.md",
    "SECURITY.md",
    "boxology-details",
    "boxology-whitepaper.md",
    "boxology.toml",
    "crates",
    "deny.toml",
    "goldens",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "specs",
];

pub(crate) fn check(root: &Path) -> bool {
    check_result(root).map_or_else(
        |error| {
            eprintln!("boundary-inventory: {error}");
            false
        },
        |()| true,
    )
}

fn check_result(root: &Path) -> Result<(), String> {
    reject_private_roots(root)?;
    let roots = root_entries(root)?;
    let expected_roots = ROOT_ENTRIES.iter().map(|name| (*name).to_owned()).collect();
    if roots != expected_roots {
        return Err("repository root inventory differs from the public framework set".into());
    }
    let workspace = read(root.join("Cargo.toml"))?;
    let cli = read(root.join("crates/boxology-cli/Cargo.toml"))?;
    let composition = read(root.join("crates/boxology-cli/boxology.toml"))?;
    let skills = skill_names(&root.join(".agents/skills"))?;
    audit_documents(&workspace, &cli, &composition, &skills)?;
    for &(member, package) in MEMBERS {
        let manifest = parse(&read(root.join(member).join("Cargo.toml"))?)?;
        if string_at(&manifest, &["package", "name"]) != Some(package) {
            return Err(format!("{member} must declare package {package}"));
        }
    }
    Ok(())
}

fn reject_private_roots(root: &Path) -> Result<(), String> {
    for name in PRIVATE_ROOTS {
        if root.join(name).exists() {
            return Err(format!(
                "private repository surface must not contain {name}/"
            ));
        }
    }
    Ok(())
}

fn root_entries(root: &Path) -> Result<BTreeSet<String>, String> {
    fs::read_dir(root)
        .map_err(|error| format!("{}: {error}", root.display()))?
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_name() == ".git" || entry.file_name() == "target" => None,
            other => Some(other),
        })
        .map(|entry| {
            entry
                .map_err(|error| error.to_string())?
                .file_name()
                .into_string()
                .map_err(|_| "root entry is not UTF-8".into())
        })
        .collect()
}

fn audit_documents(
    workspace: &str,
    cli: &str,
    composition: &str,
    skills: &BTreeSet<String>,
) -> Result<(), String> {
    let workspace = parse(workspace)?;
    let actual_members = strings_at(&workspace, &["workspace", "members"])?;
    let expected_members = MEMBERS.iter().map(|(path, _)| *path).collect::<Vec<_>>();
    exact(&actual_members, &expected_members, "workspace members")?;

    let cli = parse(cli)?;
    let bins = cli
        .get("bin")
        .and_then(Item::as_array_of_tables)
        .ok_or("CLI [[bin]] is missing")?;
    let bin = bins.iter().next().ok_or("CLI [[bin]] is empty")?;
    if bins.len() != 1
        || bin.get("name").and_then(Item::as_str) != Some("boxology")
        || bin.get("path").and_then(Item::as_str) != Some("src/main.rs")
    {
        return Err("CLI must expose exactly the boxology binary".into());
    }
    exact(
        &table_keys(&cli, "dependencies")?,
        DEPENDENCIES,
        "CLI dependencies",
    )?;
    exact(
        &table_keys(&cli, "dev-dependencies")?,
        DEV_DEPENDENCIES,
        "CLI dev-dependencies",
    )?;

    let composition = parse(composition)?;
    exact(
        &strings_at(&composition, &["composition", "boxes"])?,
        &["check", "classifier"],
        "composition boxes",
    )?;
    let bindings = composition
        .get("composition")
        .and_then(Item::as_table_like)
        .and_then(|table| table.get("bindings"))
        .and_then(Item::as_array)
        .ok_or("composition bindings are missing")?;
    let actual = bindings
        .iter()
        .map(|value| {
            let binding = value.as_inline_table()?;
            Some((
                binding.get("box")?.as_str()?,
                binding.get("capability")?.as_str()?,
                binding.get("transport")?.as_str()?,
            ))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or("composition binding is malformed")?;
    let expected = [
        ("check", "check.check", "in-process"),
        ("classifier", "classifier.classify", "in-process"),
    ];
    if actual != expected {
        return Err("composition must contain exactly the two framework bindings".into());
    }

    let expected_skills = SKILLS.iter().map(|name| (*name).to_owned()).collect();
    if skills != &expected_skills {
        return Err("top-level skill inventory differs from the framework set".into());
    }
    Ok(())
}

fn parse(text: &str) -> Result<DocumentMut, String> {
    text.parse()
        .map_err(|error| format!("invalid TOML: {error}"))
}

fn read(path: impl AsRef<Path>) -> Result<String, String> {
    fs::read_to_string(path.as_ref())
        .map_err(|error| format!("{}: {error}", path.as_ref().display()))
}

fn string_at<'a>(document: &'a DocumentMut, path: &[&str]) -> Option<&'a str> {
    let mut item = document.as_item();
    for key in path {
        item = item.get(key)?;
    }
    item.as_str()
}

fn strings_at<'a>(document: &'a DocumentMut, path: &[&str]) -> Result<Vec<&'a str>, String> {
    let mut item = document.as_item();
    for key in path {
        item = item
            .get(key)
            .ok_or_else(|| format!("missing {}", path.join(".")))?;
    }
    item.as_array()
        .ok_or_else(|| format!("{} must be an array", path.join(".")))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| format!("{} must contain strings", path.join(".")))
        })
        .collect()
}

fn table_keys<'a>(document: &'a DocumentMut, name: &str) -> Result<Vec<&'a str>, String> {
    Ok(document
        .get(name)
        .and_then(Item::as_table_like)
        .ok_or_else(|| format!("missing [{name}]"))?
        .iter()
        .map(|(key, _)| key)
        .collect())
}

fn exact<T: PartialEq + std::fmt::Debug>(
    actual: &[T],
    expected: &[T],
    label: &str,
) -> Result<(), String> {
    (actual == expected)
        .then_some(())
        .ok_or_else(|| format!("{label} differ: expected {expected:?}, got {actual:?}"))
}

fn skill_names(root: &Path) -> Result<BTreeSet<String>, String> {
    fs::read_dir(root)
        .map_err(|error| format!("{}: {error}", root.display()))?
        .map(|entry| {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
            {
                return Err(format!(
                    "{} must contain only skill directories",
                    root.display()
                ));
            }
            entry
                .file_name()
                .into_string()
                .map_err(|_| "skill name is not UTF-8".into())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORKSPACE: &str = include_str!("../../../Cargo.toml");
    const CLI: &str = include_str!("../../boxology-cli/Cargo.toml");
    const COMPOSITION: &str = include_str!("../../boxology-cli/boxology.toml");

    fn skills() -> BTreeSet<String> {
        SKILLS.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn retained_framework_boundary_is_exact_and_mutation_resistant() {
        assert_eq!(
            audit_documents(WORKSPACE, CLI, COMPOSITION, &skills()),
            Ok(())
        );

        let restored = WORKSPACE.replace(
            "    \"crates/boxology-workspace\",",
            "    \"crates/boxology-agent-harness\",\n    \"crates/boxology-workspace\",",
        );
        assert!(audit_documents(&restored, CLI, COMPOSITION, &skills()).is_err());

        let second_bin = format!("{CLI}\n[[bin]]\nname = \"app\"\npath = \"src/app.rs\"\n");
        assert!(audit_documents(WORKSPACE, &second_bin, COMPOSITION, &skills()).is_err());
        let app_dependency = CLI.replace(
            "[dependencies]",
            "[dependencies]\ntelegram = { version = \"=0.0.0\" }",
        );
        assert!(audit_documents(WORKSPACE, &app_dependency, COMPOSITION, &skills()).is_err());

        let third_box = COMPOSITION.replace(
            "boxes = [\"check\", \"classifier\"]",
            "boxes = [\"check\", \"classifier\", \"telegram\"]",
        );
        assert!(audit_documents(WORKSPACE, CLI, &third_box, &skills()).is_err());

        let mut private_skill = skills();
        private_skill.insert("private-operator-skill".into());
        assert!(audit_documents(WORKSPACE, CLI, COMPOSITION, &private_skill).is_err());
    }

    #[test]
    fn private_repository_roots_are_rejected() {
        let root =
            std::env::temp_dir().join(format!("boxology-public-roots-{}", std::process::id()));
        fs::create_dir_all(root.join("ops")).unwrap();
        assert!(reject_private_roots(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
