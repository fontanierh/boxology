use std::{collections::BTreeSet, fs, path::Path, process::Command};

use toml_edit::{DocumentMut, Item};

struct FixtureProject {
    root: &'static str,
    fmt_packages: &'static [&'static str],
}

const FIXTURE_PROJECTS: &[FixtureProject] = &[
    FixtureProject {
        root: "crates/fixtures/greeter",
        fmt_packages: &["greeter-implementation"],
    },
    FixtureProject {
        root: "crates/fixtures/hello",
        fmt_packages: &["hello-implementation"],
    },
    FixtureProject {
        root: "crates/fixtures/ping",
        fmt_packages: &["ping-implementation"],
    },
    FixtureProject {
        root: "crates/fixtures/ping-app",
        fmt_packages: &["ping-app"],
    },
];

const STAYING_ROOT_MEMBERS: &[&str] = &[
    "crates/fixtures/fixture-tests",
    "crates/fixtures/generated-style-fmt",
];
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
pub(crate) fn run(root: &Path) -> bool {
    let mut passed = check_workspace_membership(root);
    for project in FIXTURE_PROJECTS {
        let manifest = root.join(project.root).join("Cargo.toml");
        for package in project.fmt_packages {
            if !run_fmt(root, &manifest, project.root, package) {
                passed = false;
            }
        }
        if !run_clippy(root, &manifest, project.root) {
            passed = false;
        }
        if !run_test(root, &manifest, project.root) {
            passed = false;
        }
        if !run_doc(root, &manifest, project.root) {
            passed = false;
        }
    }
    passed
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target"));
    command
}

fn run_fmt(root: &Path, manifest: &Path, project: &str, package: &str) -> bool {
    let mut command = cargo(root);
    command
        .args(["fmt", "--check", "--manifest-path"])
        .arg(manifest)
        .args(["-p", package]);
    run_command(&mut command, &format!("{project} fmt {package}"))
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

#[cfg(test)]
fn package_name(path: &Path) -> String {
    manifest(path)
        .get("package")
        .and_then(Item::as_table)
        .and_then(|package| package.get("name"))
        .and_then(Item::as_str)
        .unwrap_or_else(|| panic!("{} has no package name", path.display()))
        .to_owned()
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
            let is_workspace = manifest(&manifest_path)
                .get("workspace")
                .and_then(Item::as_table)
                .is_some();
            is_workspace.then(|| format!("crates/fixtures/{}", entry.file_name().to_string_lossy()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let project_root = crate::root().join(project.root);
            let mut hand_authored = BTreeSet::new();
            for member in workspace_members(&project_root.join("Cargo.toml")) {
                let package = package_name(&project_root.join(&member).join("Cargo.toml"));
                if member.starts_with("generated/") {
                    assert!(
                        !project.fmt_packages.contains(&package.as_str()),
                        "generated member {package} is selected for fmt in {}",
                        project.root
                    );
                } else {
                    hand_authored.insert(package);
                }
            }
            let selected: BTreeSet<_> = project
                .fmt_packages
                .iter()
                .map(|package| (*package).to_owned())
                .collect();
            assert_eq!(selected, hand_authored, "fmt selection in {}", project.root);
        }
    }

    #[test]
    fn workspace_membership_is_exact() {
        assert!(check_workspace_membership(&crate::root()));
    }
}
