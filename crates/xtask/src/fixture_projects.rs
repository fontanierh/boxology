use boxology_cli_core::{cargo_metadata_command, fmt_packages, walk};
use boxology_workspace::WorkspaceInputs;
use std::{collections::BTreeSet, fs, path::Path, process::Command};

use toml_edit::{DocumentMut, Item, value};

struct FixtureProject {
    root: &'static str,
}

const FIXTURE_PROJECTS: &[FixtureProject] = &[
    FixtureProject {
        root: "crates/fixtures/greeter",
    },
    FixtureProject {
        root: "crates/fixtures/hello",
    },
    FixtureProject {
        root: "crates/fixtures/ping",
    },
    FixtureProject {
        root: "crates/fixtures/ping-app",
    },
];

const STAYING_ROOT_MEMBERS: &[&str] = &["crates/fixtures/fixture-tests"];
const MIGRATED_ROOT_MEMBERS: &[&str] = &[
    "crates/fixtures/greeter/generated/contract",
    "crates/fixtures/greeter/implementation",
    "crates/fixtures/hello/generated/contract",
    "crates/fixtures/hello/implementation",
    "crates/fixtures/ping/generated/contract",
    "crates/fixtures/ping/implementation",
    "crates/fixtures/ping-app/composition",
];

// Fixture projects validate outside the root workspace because stage-2 fixture opacity removes
// their crates from root membership (specs/s7-skill-acceptance-self-hosting.md D4). The gate is
// retained xtask scope because `boxology check` subsumes nothing here (the same spec's D5).
pub(crate) fn run(root: &Path, deep: bool) -> bool {
    let mut passed = check_workspace_membership(root);
    for project in FIXTURE_PROJECTS {
        let manifest = root.join(project.root).join("Cargo.toml");
        let packages = match fixture_fmt_packages(root, project.root) {
            Ok(packages) => packages,
            Err(error) => {
                eprintln!("fixture-projects: {error}");
                passed = false;
                Vec::new()
            }
        };
        if !packages.is_empty() && !run_fmt(root, &manifest, project.root, &packages) {
            passed = false;
        }
        if !run_test(root, &manifest, project.root) {
            passed = false;
        }
        if deep {
            if !run_clippy(root, &manifest, project.root) {
                passed = false;
            }
            if !run_doc(root, &manifest, project.root) {
                passed = false;
            }
        }
    }
    passed
}

pub(crate) fn generated_style_fails_fmt(root: &Path) -> bool {
    let output = cargo(root)
        .args([
            "fmt",
            "--check",
            "--manifest-path",
            "crates/fixtures/generated-style-fmt/Cargo.toml",
        ])
        .output();
    match output {
        Ok(output) => generated_style_fmt_result(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
        ),
        Err(error) => {
            eprintln!("generated-style-fmt: cannot run cargo fmt: {error}");
            false
        }
    }
}

fn generated_style_fmt_result(success: bool, stdout: &str) -> bool {
    !success && stdout.contains("Diff in") && stdout.contains("generated-style-fmt/src/lib.rs")
}

fn fixture_fmt_packages(root: &Path, project: &str) -> Result<Vec<String>, String> {
    let project_root = root.join(project);
    let walked = walk(&project_root).map_err(|error| format!("{project} walk: {error}"))?;
    let output = cargo_metadata_command(&project_root)
        .output()
        .map_err(|error| format!("{project} metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{project} metadata exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let metadata = String::from_utf8(output.stdout)
        .map_err(|_| format!("{project} metadata returned non-UTF-8"))?;
    let manifests = walked
        .manifests()
        .iter()
        .map(|(path, bytes)| {
            if path.as_str() == "boxology.toml" {
                formatting_manifest(bytes).map(|bytes| (path.clone(), bytes))
            } else {
                Ok((path.clone(), bytes.clone()))
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    let files = walked
        .files()
        .iter()
        .filter(|entry| entry.path().as_str() != "Cargo.lock")
        .cloned()
        .collect();
    let inputs = WorkspaceInputs::new(files, manifests, &metadata)
        .map_err(|_| format!("{project} walk returned duplicate paths"))?;
    let workspace = inputs
        .check()
        .map_err(|findings| format!("{project} manifest validation: {findings}"))?;
    Ok(fmt_packages(&workspace))
}

fn formatting_manifest(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "fixture manifest is non-UTF-8")?;
    let mut document = text
        .parse::<DocumentMut>()
        .map_err(|error| format!("cannot parse fixture manifest: {error}"))?;
    document["kind"] = value("platform");
    document.remove("imports");
    document.remove("composition");
    let owned = document["owned"]
        .as_array_mut()
        .ok_or_else(|| "fixture manifest has no owned array".to_owned())?;
    owned.push("Cargo.toml");
    if let Some(crates) = document
        .get_mut("crates")
        .and_then(Item::as_array_of_tables_mut)
    {
        for entry in crates.iter_mut() {
            entry["role"] = value("platform");
        }
    }
    Ok(document.to_string().into_bytes())
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target"));
    command
}

fn run_fmt(root: &Path, manifest: &Path, project: &str, packages: &[String]) -> bool {
    let mut command = cargo(root);
    command
        .args(["fmt", "--check", "--manifest-path"])
        .arg(manifest);
    for package in packages {
        command.args(["-p", package]);
    }
    run_command(&mut command, &format!("{project} fmt"))
}

fn run_clippy(root: &Path, manifest: &Path, project: &str) -> bool {
    let mut command = cargo(root);
    command
        .args(["clippy", "--manifest-path"])
        .arg(manifest)
        .args([
            "--all-targets",
            "--all-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ]);
    run_command(&mut command, &format!("{project} clippy"))
}

fn run_test(root: &Path, manifest: &Path, project: &str) -> bool {
    let mut command = cargo(root);
    command
        .args(["test", "--manifest-path"])
        .arg(manifest)
        .args(["--all-features", "--locked"]);
    run_command(&mut command, &format!("{project} test"))
}

fn run_doc(root: &Path, manifest: &Path, project: &str) -> bool {
    let mut command = cargo(root);
    command
        .args(["doc", "--manifest-path"])
        .arg(manifest)
        .args(["--no-deps"])
        .env("RUSTDOCFLAGS", "-D warnings");
    run_command(&mut command, &format!("{project} doc"))
}

fn run_command(command: &mut Command, label: &str) -> bool {
    match command.status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("fixture-projects: {label} exited with {status}");
            false
        }
        Err(error) => {
            eprintln!("fixture-projects: cannot run {label}: {error}");
            false
        }
    }
}

fn manifest(path: &Path) -> DocumentMut {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .parse()
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()))
}

fn workspace_members(path: &Path) -> Vec<String> {
    manifest(path)
        .get("workspace")
        .and_then(Item::as_table)
        .and_then(|workspace| workspace.get("members"))
        .and_then(Item::as_array)
        .unwrap_or_else(|| panic!("{} has no workspace members", path.display()))
        .iter()
        .map(|member| {
            member
                .as_str()
                .unwrap_or_else(|| panic!("{} has a non-string workspace member", path.display()))
                .to_owned()
        })
        .collect()
}

fn check_workspace_membership(root: &Path) -> bool {
    let root_members: BTreeSet<_> = workspace_members(&root.join("Cargo.toml"))
        .into_iter()
        .collect();
    let root_fixture_members: BTreeSet<_> = root_members
        .iter()
        .filter(|member| member.starts_with("crates/fixtures/"))
        .cloned()
        .collect();
    let staying: BTreeSet<_> = STAYING_ROOT_MEMBERS
        .iter()
        .map(|member| (*member).to_owned())
        .collect();
    if root_fixture_members != staying {
        eprintln!(
            "fixture-projects: root fixture members mismatch: expected {staying:?}, found {root_fixture_members:?}"
        );
        return false;
    }
    let mut project_members = BTreeSet::new();
    for project in FIXTURE_PROJECTS {
        for member in workspace_members(&root.join(project.root).join("Cargo.toml")) {
            project_members.insert(format!("{}/{member}", project.root));
        }
    }
    let migrated: BTreeSet<_> = MIGRATED_ROOT_MEMBERS
        .iter()
        .map(|member| (*member).to_owned())
        .collect();
    if project_members != migrated {
        eprintln!(
            "fixture-projects: migrated fixture members mismatch: expected {migrated:?}, found {project_members:?}"
        );
        return false;
    }

    let still_at_root: BTreeSet<_> = root_members.intersection(&migrated).cloned().collect();
    if !still_at_root.is_empty() {
        eprintln!("fixture-projects: migrated members remain at root: {still_at_root:?}");
        return false;
    }

    true
}

#[cfg(test)]
fn fixture_project_inventory(root: &Path) -> BTreeSet<String> {
    fs::read_dir(root.join("crates/fixtures"))
        .unwrap_or_else(|error| panic!("cannot scan fixture roots: {error}"))
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let manifest_path = path.join("Cargo.toml");
            if !path.is_dir() || !manifest_path.is_file() {
                return None;
            }
            let has_members = manifest(&manifest_path)
                .get("workspace")
                .and_then(Item::as_table)
                .and_then(|workspace| workspace.get("members"))
                .is_some();
            has_members.then(|| format!("crates/fixtures/{}", entry.file_name().to_string_lossy()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_manifest::{CrateRole, Manifest, RelativePath};

    #[test]
    fn fixture_project_inventory_is_exact() {
        let expected: BTreeSet<_> = FIXTURE_PROJECTS
            .iter()
            .map(|project| project.root.to_owned())
            .collect();
        assert_eq!(fixture_project_inventory(&crate::root()), expected);
    }

    #[test]
    fn fmt_selection_covers_exactly_hand_authored_members() {
        for project in FIXTURE_PROJECTS {
            let selected = fixture_fmt_packages(&crate::root(), project.root).unwrap();
            let bytes = fs::read(crate::root().join(project.root).join("boxology.toml")).unwrap();
            let declared =
                Manifest::parse(RelativePath::new("boxology.toml").unwrap(), &bytes).unwrap();
            let expected: BTreeSet<_> = declared
                .crates()
                .iter()
                .filter_map(|entry| {
                    let cargo =
                        RelativePath::new(format!("{}/Cargo.toml", entry.path().as_str())).unwrap();
                    let derived = declared
                        .derived()
                        .iter()
                        .any(|output| output.outputs().iter().any(|glob| glob.matches(&cargo)));
                    assert_eq!(derived, entry.role() == CrateRole::BoxContract);
                    (!derived).then(|| entry.cargo_package().to_owned())
                })
                .collect();
            assert_eq!(selected.into_iter().collect::<BTreeSet<_>>(), expected);
        }
    }

    #[test]
    fn generated_style_requires_an_affirmative_exact_diff() {
        assert!(generated_style_fmt_result(
            false,
            "Diff in generated-style-fmt/src/lib.rs"
        ));
        assert!(!generated_style_fmt_result(
            true,
            "Diff in generated-style-fmt/src/lib.rs"
        ));
        assert!(!generated_style_fmt_result(false, "some other failure"));
    }

    #[test]
    fn workspace_membership_is_exact() {
        assert!(check_workspace_membership(&crate::root()));
    }

    #[test]
    fn standalone_style_fmt_fixture_topology_is_exact() {
        const FIXTURE_ROOT: &str = "crates/fixtures/generated-style-fmt";
        const FIXTURE_GLOB: &str = "crates/fixtures/generated-style-fmt/**";
        const PACKAGE: &str = "generated-style-fmt";

        let root = crate::root();
        let fixture_workspace =
            manifest(&root.join("crates/fixtures/generated-style-fmt/Cargo.toml"))
                .get("workspace")
                .and_then(Item::as_table)
                .expect("generated-style-fmt declares [workspace]")
                .clone();
        assert!(
            fixture_workspace.is_empty(),
            "generated-style-fmt workspace table must be empty, got {fixture_workspace:?}"
        );

        let root_cargo = manifest(&root.join("Cargo.toml"));
        let root_boxology = manifest(&root.join("boxology.toml"));
        assert_eq!(
            string_array_occurrences(&root_cargo, &["workspace", "members"], FIXTURE_ROOT),
            0,
            "generated-style-fmt must not be a root workspace member"
        );
        assert_eq!(
            string_array_occurrences(&root_cargo, &["workspace", "exclude"], FIXTURE_ROOT),
            1,
            "generated-style-fmt must appear exactly once in root workspace.exclude"
        );
        assert_eq!(
            string_array_occurrences(&root_boxology, &["owned"], FIXTURE_GLOB),
            0,
            "generated-style-fmt glob must not appear in root Boxology owned"
        );
        assert_eq!(
            string_array_occurrences(&root_boxology, &["fixtures"], FIXTURE_GLOB),
            1,
            "generated-style-fmt glob must appear exactly once in root Boxology fixtures"
        );
        assert!(
            !root_crate_package_names(&root_boxology).contains(PACKAGE),
            "root [[crates]] must not name cargo package generated-style-fmt"
        );
    }

    fn string_array_occurrences(doc: &DocumentMut, path: &[&str], needle: &str) -> usize {
        let mut item = doc.as_item();
        for key in path {
            item = item
                .get(key)
                .unwrap_or_else(|| panic!("missing key {key} while resolving {path:?}"));
        }
        item.as_array()
            .unwrap_or_else(|| panic!("{path:?} is not an array"))
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|value| *value == needle)
            .count()
    }

    fn root_crate_package_names(doc: &DocumentMut) -> BTreeSet<String> {
        doc.get("crates")
            .and_then(Item::as_array_of_tables)
            .expect("root boxology.toml declares [[crates]]")
            .iter()
            .map(|table| {
                table
                    .get("cargo_package")
                    .and_then(Item::as_str)
                    .expect("[[crates]] entries declare cargo_package")
                    .to_owned()
            })
            .collect()
    }
}
