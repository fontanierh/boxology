#[rustfmt::skip]
use std::{collections::{BTreeMap, BTreeSet}, env, fs, path::Path, process::{Command, Output}};
use toml_edit::{DocumentMut, Item, TableLike};

const VERSION: &str = "0.1.0";
const REQUIRED_FILES: [&str; 3] = ["README.md", "LICENSE-MIT", "LICENSE-APACHE"];
// The checker proves this is a topological ordering of the exact CLI/init normal closure.
#[rustfmt::skip]
const RELEASE: &[(&str, &str)] = &[
    ("boxology-contract", "crates/boxology-contract"), ("boxology-contract-syntax", "crates/boxology-contract-syntax"), ("boxology-macros", "crates/boxology-macros"), ("boxology", "crates/boxology"),
    ("boxology-schema", "crates/boxology-schema"), ("boxology-classifier", "crates/boxology-classifier"), ("boxology-runtime", "crates/boxology-runtime"), ("boxology-manifest", "crates/boxology-manifest"),
    ("boxology-generator-model", "crates/boxology-generator-model"), ("boxology-generator", "crates/boxology-generator"), ("boxology-generator-writer", "crates/boxology-generator-writer"), ("boxology-workspace", "crates/boxology-workspace"),
    ("boxology-cli-core", "crates/boxology-cli-core"), ("classifier-contract", "crates/boxology-classifier/generated/contract"), ("classifier-implementation", "crates/boxology-classifier/implementation"), ("check-contract", "crates/boxology-check/generated/contract"),
    ("check-implementation", "crates/boxology-check/implementation"), ("boxology-init", "crates/boxology-init"), ("boxology-cli", "crates/boxology-cli"),
];
pub(crate) fn run(root: &Path, args: &[String]) -> u8 {
    if let Err(errors) = check(root, RELEASE) {
        for error in errors {
            eprintln!("release: {error}");
        }
        return 1;
    }
    match args {
        [] => preflight(root),
        [arg] if arg == "preflight" => preflight(root),
        [action, name] if action == "publish" => publish_one(root, name),
        _ => {
            eprintln!("release: use `preflight` or `publish <crate-name>`");
            2
        }
    }
}
fn cargo(root: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("cargo")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|error| format!("run cargo {}: {error}", args.join(" ")))
}
fn inventory(output: &Output, name: &str) -> Result<(), String> {
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let files: BTreeSet<_> = text.lines().collect();
    for required in REQUIRED_FILES {
        if !files
            .iter()
            .any(|path| *path == required || path.ends_with(&format!("/{required}")))
        {
            return Err(format!("{name}: package inventory lacks {required}"));
        }
    }
    Ok(())
}
fn preflight(root: &Path) -> u8 {
    for (name, _) in RELEASE {
        let output = match cargo(
            root,
            &["package", "--locked", "--allow-dirty", "--list", "-p", name],
        ) {
            Ok(output) => output,
            Err(error) => {
                eprintln!("release: {error}");
                return 1;
            }
        };
        if let Err(error) = inventory(&output, name) {
            eprintln!("release: {error}");
            return 1;
        }
        println!("release: {name}: inventory verified");
    }
    // Only dependency-free roots can produce and verify a real package before staged publication.
    let root_name = RELEASE[0].0;
    let output = cargo(
        root,
        &["package", "--locked", "--allow-dirty", "-p", root_name],
    )
    .unwrap();
    if !output.status.success() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        return 1;
    }
    let archive = root.join(format!("target/package/{root_name}-{VERSION}.crate"));
    let tar = Command::new("tar")
        .args(["-tzf"])
        .arg(&archive)
        .output()
        .unwrap();
    if let Err(error) = inventory(&tar, root_name) {
        eprintln!("release: archive {error}");
        return 1;
    }
    println!("release: {root_name}: real package and archive verified");
    0
}
fn visible(root: &Path, name: &str) -> bool {
    cargo(
        root,
        &[
            "info",
            &format!("{name}@{VERSION}"),
            "--registry",
            "crates-io",
        ],
    )
    .is_ok_and(|output| output.status.success())
}
fn publish_one(root: &Path, requested: &str) -> u8 {
    if env::var_os("BOXOLOGY_RELEASE_PUBLISH").as_deref() != Some("1".as_ref()) {
        eprintln!(
            "release: set BOXOLOGY_RELEASE_PUBLISH=1 after configuring Cargo credentials securely"
        );
        return 2;
    }
    let states: Vec<_> = RELEASE
        .iter()
        .map(|(name, _)| visible(root, name))
        .collect();
    let Some(next) = states.iter().position(|published| !published) else {
        eprintln!("release: all {VERSION} crates are already visible");
        return 2;
    };
    if states[next + 1..].iter().any(|published| *published) {
        eprintln!("release: crates.io has a non-prefix partial release; refusing");
        return 1;
    }
    if requested != RELEASE[next].0 {
        eprintln!(
            "release: next unpublished crate is {}, not {requested}",
            RELEASE[next].0
        );
        return 2;
    }
    for args in [
        vec![
            "publish",
            "--dry-run",
            "--locked",
            "--allow-dirty",
            "--registry",
            "crates-io",
            "-p",
            requested,
        ],
        vec![
            "publish",
            "--locked",
            "--registry",
            "crates-io",
            "-p",
            requested,
        ],
    ] {
        let output = cargo(root, &args).unwrap();
        if !output.status.success() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            return 1;
        }
    }
    println!(
        "release: published {requested}; wait until crates.io reports {requested}@{VERSION} before continuing"
    );
    0
}
fn load(root: &Path, relative: &str) -> Result<DocumentMut, String> {
    fs::read_to_string(root.join(relative))
        .map_err(|e| format!("read {relative}: {e}"))?
        .parse()
        .map_err(|e| format!("parse {relative}: {e}"))
}
fn inherited(table: &dyn TableLike, key: &str) -> bool {
    table
        .get(key)
        .and_then(Item::as_table_like)
        .and_then(|v| v.get("workspace"))
        .and_then(Item::as_bool)
        == Some(true)
}
#[rustfmt::skip]
fn check(root: &Path, order: &[(&str, &str)]) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let root_doc = load(root, "Cargo.toml").map_err(|e| vec![e])?;
    let workspace = root_doc["workspace"].as_table_like().ok_or_else(|| vec!["missing workspace".into()])?;
    let package = workspace.get("package").and_then(Item::as_table_like).ok_or_else(|| vec!["missing workspace.package".into()])?;
    if package.get("version").and_then(Item::as_str) != Some(VERSION) { errors.push(format!("workspace version must be {VERSION}")); }
    let members: Vec<_> = workspace.get("members").and_then(Item::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    let names: BTreeSet<_> = order.iter().map(|(name, _)| *name).collect();
    let positions: BTreeMap<_, _> = order.iter().enumerate().map(|(i, (name, _))| (*name, i)).collect();
    let paths: BTreeMap<_, _> = order.iter().map(|(name, path)| (*path, *name)).collect();
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for member in members {
        let relative = format!("{member}/Cargo.toml");
        let Ok(doc) = load(root, &relative) else { errors.push(format!("cannot load {relative}")); continue; };
        let Some(pkg) = doc.get("package").and_then(Item::as_table_like) else { errors.push(format!("{relative}: missing package")); continue; };
        let Some(name) = pkg.get("name").and_then(Item::as_str).map(str::to_owned) else { continue; };
        let released = names.contains(name.as_str());
        if pkg.get("publish").and_then(Item::as_bool) != Some(released) { errors.push(format!("{name}: publish must be {released}")); }
        if released {
            if paths.get(member.as_str()).copied() != Some(name.as_str()) { errors.push(format!("{name}: unexpected release path")); }
            for key in ["version", "rust-version", "license", "repository", "homepage", "readme"] {
                if !inherited(pkg, key) { errors.push(format!("{name}: {key} must inherit")); }
            }
            for file in ["LICENSE-MIT", "LICENSE-APACHE"] {
                if fs::read(root.join(&member).join(file)).ok() != fs::read(root.join(file)).ok() { errors.push(format!("{name}: {file} must equal root license")); }
            }
        }
        let mut local = Vec::new();
        if let Some(deps) = doc.get("dependencies").and_then(Item::as_table_like) {
            for (alias, item) in deps.iter() {
                let Some(dep) = item.as_table_like() else { continue; };
                if dep.get("path").is_none() { continue; }
                let dependency = dep.get("package").and_then(Item::as_str).unwrap_or(alias);
                if !names.contains(dependency) { if released { errors.push(format!("{name}: {dependency} is unpublished")); } continue; }
                if dep.get("version").and_then(Item::as_str) != Some("=0.1.0") { errors.push(format!("{name}: {dependency} must use =0.1.0 plus path")); }
                local.push(dependency.to_owned());
                if released && positions[dependency] >= positions[name.as_str()] { errors.push(format!("order places {dependency} after {name}")); }
            }
        }
        if released { edges.insert(name, local); }
    }
    let mut closure = BTreeSet::new();
    let mut pending = vec!["boxology-cli".to_owned(), "boxology-init".to_owned()];
    while let Some(name) = pending.pop() { if closure.insert(name.clone()) { pending.extend(edges.get(&name).into_iter().flatten().cloned()); } }
    if closure != names.iter().map(|name| (*name).to_owned()).collect() { errors.push("release set is not exact CLI/init closure".into()); }
    for (name, expected) in [("boxology-cli", "boxology"), ("boxology-init", "boxology-init")] {
        let path = order.iter().find(|(n, _)| *n == name).unwrap().1;
        let doc = load(root, &format!("{path}/Cargo.toml")).unwrap();
        let bins: Vec<_> = doc.get("bin").and_then(Item::as_array_of_tables).map(|a| a.iter().filter_map(|t| t.get("name")?.as_str()).collect()).unwrap_or_default();
        if bins != [expected] { errors.push(format!("{name}: binary must be {expected}")); }
    }
    errors.is_empty().then_some(()).ok_or(errors)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_and_swapped_order_mutation() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert_eq!(check(&root, RELEASE), Ok(()));
        let mut swapped = RELEASE.to_vec();
        swapped.swap(0, 1);
        assert!(
            check(&root, &swapped)
                .unwrap_err()
                .iter()
                .any(|e| e.contains("order places"))
        );
    }
}
