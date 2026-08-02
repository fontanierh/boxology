use boxology_cli::{GenerationPlan, PlanError, plan};
use boxology_contract::BoxId;
use boxology_manifest::RelativePath;
use boxology_workspace::{FileEntry, Workspace, WorkspaceInputs};

#[rustfmt::skip]
mod tests {
use super::*;

const ROOT_MANIFEST: &str = "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n";

fn implementation_crate(id: &str, suffix: &str, path: &str) -> String { format!("[[crates]]\ncargo_package = \"{id}-implementation{suffix}\"\npath = \"{path}\"\nrole = \"box-implementation\"\n") }

fn candidate_manifest_with(id: &str, generator: &str, implementation: bool, implementation_path: &str, extra_implementation: bool, import_ids: &[&str], duplicate: bool) -> String {
    let owned = match (implementation, extra_implementation) {
        (false, _) => "\"boxology.toml\", \"fixtures/**\"".to_owned(),
        (true, false) => format!("\"boxology.toml\", \"{implementation_path}/**\", \"fixtures/**\""),
        (true, true) => format!("\"boxology.toml\", \"{implementation_path}/**\", \"alternate/**\", \"fixtures/**\""),
    };
    let implementations = if implementation { let mut value = implementation_crate(id, "", implementation_path); if extra_implementation { value.push_str(&implementation_crate(id, "-alternate", "alternate")); } value } else { String::new() };
    let imports = import_ids.iter().map(|id| format!("[[imports]]\npackage = \"{id}\"\ncontract = \"{id}\"\n")).collect::<String>();
    let (outputs, second) = if duplicate { ("[\"generated/contract/**\"]", "[[derived]]\nid = \"second\"\ngenerator = \"boxology-contract\"\ninputs = [\"**\"]\noutputs = [\"generated/other/**\"]\n") } else { ("[\"generated/**\"]", "") };
    format!("schema = 1\nid = {id:?}\nkind = \"box\"\nowned = [{owned}]\n\n{implementations}{imports}[[crates]]\ncargo_package = \"{id}-contract\"\npath = \"generated/contract\"\nrole = \"box-contract\"\n\n[[derived]]\nid = \"contract\"\ngenerator = {generator:?}\ninputs = [\"**\"]\noutputs = {outputs}\n{second}")
}

fn candidate_manifest(id: &str, generator: &str, implementation: bool, implementation_path: &str, extra_implementation: bool, imports: bool, duplicate: bool) -> String {
    let import_ids = if imports { vec!["foreign"] } else { Vec::new() };
    candidate_manifest_with(id, generator, implementation, implementation_path, extra_implementation, &import_ids, duplicate)
}

const FOREIGN_MANIFEST: &str = "schema = 1\nid = \"foreign\"\nkind = \"box\"\nowned = [\"boxology.toml\", \"implementation/**\"]\n\n[[crates]]\ncargo_package = \"foreign-implementation\"\npath = \"implementation\"\nrole = \"box-implementation\"\n";

fn metadata_package(directory: &str, name: &str) -> String { let id = format!("path+file:///w/{directory}#0.0.0"); format!(r#"{{"id":{id:?},"name":{name:?},"manifest_path":"/w/{directory}/Cargo.toml","dependencies":[]}}"#) }

fn metadata(foreign: bool, implementation: bool, implementation_path: &str, extra_implementation: bool, second_candidate: bool) -> String {
    let mut members = vec![(String::from("ping/generated/contract"), String::from("ping-contract"))];
    if implementation { members.push((format!("ping/{implementation_path}"), String::from("ping-implementation"))); if extra_implementation { members.push((String::from("ping/alternate"), String::from("ping-implementation-alternate"))); } }
    if second_candidate { members.extend([(String::from("pong/generated/contract"), String::from("alpha-contract")), (String::from("pong/implementation"), String::from("alpha-implementation"))]); }
    if foreign { members.push((String::from("ping/nested/implementation"), String::from("foreign-implementation"))); }
    let ids = members.iter().map(|(directory, _)| format!("{:?}", format!("path+file:///w/{directory}#0.0.0"))).collect::<Vec<_>>().join(",");
    let packages = members.iter().map(|(directory, name)| metadata_package(directory, name)).collect::<Vec<_>>().join(",");
    format!(r#"{{"workspace_root":"/w","workspace_members":[{ids}],"packages":[{packages}]}}"#)
}

const SECOND_ID: &str = "alpha";
const SECOND_ROOT: &str = "pong";

struct WorkspaceConfig<'a> { generator: &'a str, implementation: bool, imports: bool, duplicate: bool, foreign: bool, second_candidate: bool, implementation_path: &'a str, extra_implementation: bool }

fn workspace_config<'a>(generator: &'a str, implementation: bool, imports: bool, duplicate: bool, foreign: bool) -> WorkspaceConfig<'a> { WorkspaceConfig { generator, implementation, imports, duplicate, foreign, second_candidate: false, implementation_path: "implementation", extra_implementation: false } }

fn workspace(generator: &str, implementation: bool, imports: bool, duplicate: bool, foreign: bool) -> Workspace { workspace_with(workspace_config(generator, implementation, imports, duplicate, foreign)) }

fn workspace_with(config: WorkspaceConfig<'_>) -> Workspace {
    let WorkspaceConfig { generator, implementation, imports, duplicate, foreign, second_candidate, implementation_path, extra_implementation } = config;
    let mut files: Vec<String> = ["Cargo.toml", "Cargo.lock", "boxology.toml", "ping/boxology.toml", "ping/fixtures/data.txt", "ping/generated/contract/Cargo.toml"].into_iter().map(String::from).collect();
    if implementation { files.extend([format!("ping/{implementation_path}/Cargo.toml"), format!("ping/{implementation_path}/src/lib.rs")]); if extra_implementation { files.extend([String::from("ping/alternate/Cargo.toml"), String::from("ping/alternate/src/lib.rs")]); } }
    let mut manifests = vec![(String::from("boxology.toml"), ROOT_MANIFEST.as_bytes().to_vec()), (String::from("ping/boxology.toml"), candidate_manifest("ping", generator, implementation, implementation_path, extra_implementation, imports, duplicate).into_bytes())];
    if duplicate { files.push(String::from("ping/generated/other/old.json")); }
    if second_candidate { files.extend([format!("{SECOND_ROOT}/boxology.toml"), format!("{SECOND_ROOT}/implementation/Cargo.toml"), format!("{SECOND_ROOT}/implementation/src/lib.rs"), format!("{SECOND_ROOT}/generated/contract/Cargo.toml")]); manifests.push((format!("{SECOND_ROOT}/boxology.toml"), candidate_manifest(SECOND_ID, generator, true, "implementation", false, false, false).into_bytes())); }
    if foreign { files.extend([String::from("ping/nested/boxology.toml"), String::from("ping/nested/implementation/Cargo.toml"), String::from("ping/nested/implementation/src/lib.rs")]); manifests.push((String::from("ping/nested/boxology.toml"), FOREIGN_MANIFEST.as_bytes().to_vec())); }
    let files = files.into_iter().map(|name| FileEntry::file(path(&name))).collect();
    let manifests = manifests.into_iter().map(|(name, bytes)| (path(&name), bytes)).collect();
    WorkspaceInputs::new(files, manifests, &metadata(foreign, implementation, implementation_path, extra_implementation, second_candidate)).unwrap().check().unwrap()
}

fn path(value: &str) -> RelativePath { RelativePath::new(value).unwrap() }
fn id(value: &str) -> BoxId { BoxId::new(value).unwrap() }

fn metadata_for(packages: &[&str]) -> String {
    let members = packages.iter().flat_map(|id| [(format!("{id}/generated/contract"), format!("{id}-contract")), (format!("{id}/implementation"), format!("{id}-implementation"))]).collect::<Vec<_>>();
    let ids = members.iter().map(|(directory, _)| format!("{:?}", format!("path+file:///w/{directory}#0.0.0"))).collect::<Vec<_>>().join(",");
    let packages = members.iter().map(|(directory, name)| metadata_package(directory, name)).collect::<Vec<_>>().join(",");
    format!(r#"{{"workspace_root":"/w","workspace_members":[{ids}],"packages":[{packages}]}}"#)
}

fn imported_workspace(imports: &[&str], targets: &[(&str, &str)]) -> Workspace {
    let mut files: Vec<String> = ["Cargo.toml", "Cargo.lock", "boxology.toml", "greeter/boxology.toml", "greeter/implementation/Cargo.toml", "greeter/implementation/src/lib.rs", "greeter/generated/contract/Cargo.toml"].into_iter().map(String::from).collect();
    let mut manifests = vec![(String::from("boxology.toml"), ROOT_MANIFEST.as_bytes().to_vec()), (String::from("greeter/boxology.toml"), candidate_manifest_with("greeter", "boxology-contract", true, "implementation", false, imports, false).into_bytes())];
    for (id, generator) in targets {
        files.extend([format!("{id}/boxology.toml"), format!("{id}/implementation/Cargo.toml"), format!("{id}/implementation/src/lib.rs"), format!("{id}/generated/contract/Cargo.toml")]);
        manifests.push((format!("{id}/boxology.toml"), candidate_manifest(id, generator, true, "implementation", false, false, false).into_bytes()));
    }
    let mut packages = vec!["greeter"];
    packages.extend(targets.iter().map(|(id, _)| *id));
    let files = files.into_iter().map(|name| FileEntry::file(path(&name))).collect();
    let manifests = manifests.into_iter().map(|(name, bytes)| (path(&name), bytes)).collect();
    WorkspaceInputs::new(files, manifests, &metadata_for(&packages)).unwrap().check().unwrap()
}

fn error_is(result: Result<Vec<GenerationPlan>, PlanError>, code: &str, at: &str, detail: &str) {
    let error = result.expect_err("planning must reject the unsatisfied fixture");
    assert_eq!(error.code(), code);
    assert_eq!(error.path().as_str(), at);
    assert_eq!(error.detail(), detail);
    assert_eq!(error.to_string(), format!("{code} {at:?}: {detail}"));
}

fn input_names(plan: &GenerationPlan) -> Vec<&str> { plan.inputs().iter().map(RelativePath::as_str).collect() }
fn plan_ids(plans: &[GenerationPlan]) -> Vec<&str> { plans.iter().map(|plan| plan.package_id().as_str()).collect() }

#[test]
fn greeter_fixture_manifest_parses_clean() {
    let manifest = boxology_manifest::Manifest::parse(path("boxology.toml"), include_str!("../../fixtures/greeter/boxology.toml").as_bytes()).unwrap();
    assert_eq!(manifest.imports().iter().map(|import| import.package().as_str()).collect::<Vec<_>>(), ["hello"]);
}

#[test]
fn plan_resolves_import_by_identity_to_target_schema() {
    let workspace = imported_workspace(&["hello"], &[("hello", "boxology-contract")]);
    let plans = plan(&workspace, None).unwrap();
    let greeter = plans.iter().find(|plan| plan.package_id().as_str() == "greeter").unwrap();
    let [import] = greeter.imports() else { panic!("one resolved import is required") };
    assert_eq!(import.package().as_str(), "hello");
    assert_eq!(import.schema().as_str(), "hello/generated/schema.json");
}

#[test]
fn imports_must_resolve_to_generation_candidates() {
    error_is(plan(&imported_workspace(&["missing"], &[]), None), "BXW0084", "greeter/boxology.toml", "a declared import must name a discovered workspace package");
    error_is(plan(&imported_workspace(&["hello"], &[("hello", "cargo")]), None), "BXW0085", "greeter/boxology.toml", "an imported package must declare a contract-generation output");
}

#[test]
fn plan_preserves_import_declaration_order() {
    let workspace = imported_workspace(&["zulu", "alpha"], &[("alpha", "boxology-contract"), ("zulu", "boxology-contract")]);
    let plans = plan(&workspace, None).unwrap();
    let greeter = plans.iter().find(|plan| plan.package_id().as_str() == "greeter").unwrap();
    assert_eq!(greeter.imports().iter().map(|import| import.package().as_str()).collect::<Vec<_>>(), ["zulu", "alpha"]);
}

#[test]
fn plan_is_complete_sorted_and_excludes_cargo_and_foreign_or_derived_files() {
    let plans = plan(&workspace("boxology-contract", true, false, false, true), None).unwrap();
    assert_eq!(plans.len(), 1, "the platform cargo output is not a plan");
    let [plan] = plans.as_slice() else { panic!("one ping plan is required") };
    assert_eq!(plan.package_id().as_str(), "ping");
    assert_eq!(plan.manifest_path().as_str(), "ping/boxology.toml");
    assert_eq!(plan.package_root().map(RelativePath::as_str), Some("ping"));
    assert_eq!(plan.derived_output_id().as_str(), "contract");
    assert_eq!(plan.crate_root().as_str(), "implementation/src/lib.rs");
    assert_eq!(input_names(plan), ["boxology.toml", "fixtures/data.txt", "implementation/Cargo.toml", "implementation/src/lib.rs"]);
    assert_eq!(plan.outputs().iter().map(|pattern| pattern.as_str()).collect::<Vec<_>>(), ["generated/**"]);
}

#[test]
fn package_selection_is_live_and_deterministic() {
    let workspace = workspace_with(WorkspaceConfig { second_candidate: true, ..workspace_config("boxology-contract", true, false, false, false) });
    assert_eq!(plan_ids(&plan(&workspace, None).unwrap()), [SECOND_ID, "ping"]);
    assert_eq!(plan_ids(&plan(&workspace, Some(&id("ping"))).unwrap()), ["ping"]);
}

#[test]
fn planning_rejections_are_stable() {
    for (workspace, selection, code, at, detail) in [
        (workspace("other-tool", true, false, false, false), None, "BXW0064", "ping/boxology.toml", "only the boxology-contract generator is supported by generate"),
        (workspace("boxology-contract", true, false, false, false), Some(id("absent")), "BXW0065", "<request>", "the requested package must be a discovered workspace package"),
        (workspace("cargo", true, false, false, false), Some(id("ping")), "BXW0066", "ping/boxology.toml", "the selected package must declare a contract-generation output"),
        (workspace("boxology-contract", true, false, true, false), None, "BXW0069", "ping/boxology.toml", "a package must declare at most one contract-generation output"),
    ] { error_is(plan(&workspace, selection.as_ref()), code, at, detail); }
}

#[test]
fn implementation_crate_root_must_be_exactly_derivable() {
    for (implementation, path, extra) in [(false, "implementation", false), (true, "implementation", true)] {
        let config = WorkspaceConfig { implementation_path: path, extra_implementation: extra, ..workspace_config("boxology-contract", implementation, false, false, false) };
        error_is(plan(&workspace_with(config), None), "BXW0067", "ping/boxology.toml", "a generation candidate must declare exactly one box-implementation crate");
    }
    let config = WorkspaceConfig { implementation_path: "engine", ..workspace_config("boxology-contract", true, false, false, false) };
    let plans = plan(&workspace_with(config), None).unwrap();
    assert_eq!(plans[0].crate_root().as_str(), "engine/src/lib.rs");
}
}
