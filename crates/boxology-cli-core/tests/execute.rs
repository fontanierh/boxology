use boxology_cli_core::{GenerationPlan, execute, execute_plans, plan};
use boxology_generator::{GeneratedTree, OUTPUTS, generate};
use boxology_generator_model::GenerationRequest;
use boxology_manifest::RelativePath;
use boxology_workspace::{FileEntry, Workspace, WorkspaceInputs};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const ROOT_MANIFEST: &str = "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n";
const CONTRACT: &[u8] = br#"boxology::contract! {
    #[error]
    pub enum HelloError { EmptyName }
    #[capability(exposure = external)]
    pub async fn ping(nonce: u64) -> Result<u64, HelloError>;
}"#;
const OUTLINE: &[u8] = b"mod contract;\npub use contract::*;\n";
const METADATA: &str = r#"{"workspace_root":"/w","workspace_members":["path+file:///w/ping/generated/contract#0.0.0","path+file:///w/ping/implementation#0.0.0"],"packages":[{"id":"path+file:///w/ping/generated/contract#0.0.0","name":"ping-contract","manifest_path":"/w/ping/generated/contract/Cargo.toml","dependencies":[]},{"id":"path+file:///w/ping/implementation#0.0.0","name":"ping-implementation","manifest_path":"/w/ping/implementation/Cargo.toml","dependencies":[]}] }"#;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    plan: GenerationPlan,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn path(value: &str) -> RelativePath {
    RelativePath::new(value).unwrap()
}

fn package_manifest(outputs: &str) -> String {
    format!(
        "schema = 1\nid = \"ping\"\nkind = \"box\"\nowned = [\"boxology.toml\", \"implementation/**\"]\n\n[[crates]]\ncargo_package = \"ping-implementation\"\npath = \"implementation\"\nrole = \"box-implementation\"\n\n[[crates]]\ncargo_package = \"ping-contract\"\npath = \"generated/contract\"\nrole = \"box-contract\"\n\n[[derived]]\nid = \"contract\"\ngenerator = \"boxology-contract\"\ninputs = [\"boxology.toml\", \"implementation/src/**\"]\noutputs = {outputs}\n"
    )
}

fn workspace(outputs: &str) -> Workspace {
    let files = [
        "Cargo.toml",
        "Cargo.lock",
        "boxology.toml",
        "ping/boxology.toml",
        "ping/implementation/Cargo.toml",
        "ping/implementation/src/lib.rs",
        "ping/implementation/src/contract.rs",
        "ping/generated/contract/Cargo.toml",
    ]
    .into_iter()
    .map(|name| FileEntry::file(path(name)))
    .collect();
    let manifests = vec![
        (path("boxology.toml"), ROOT_MANIFEST.as_bytes().to_vec()),
        (
            path("ping/boxology.toml"),
            package_manifest(outputs).into_bytes(),
        ),
    ];
    WorkspaceInputs::new(files, manifests, METADATA)
        .unwrap()
        .check()
        .unwrap()
}

fn fixture(outputs: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "boxology-cli-execute-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("ping/implementation/src")).unwrap();
    fs::write(root.join("ping/boxology.toml"), package_manifest(outputs)).unwrap();
    fs::write(root.join("ping/implementation/src/lib.rs"), OUTLINE).unwrap();
    fs::write(root.join("ping/implementation/src/contract.rs"), CONTRACT).unwrap();
    let [plan] = plan(&workspace(outputs), None).unwrap().try_into().unwrap();
    Fixture { root, plan }
}

const PROJECT_FILES: [&str; 8] = [
    "boxology.toml",
    "implementation/Cargo.toml",
    "implementation/src/lib.rs",
    "implementation/src/contract.rs",
    "generated/contract/Cargo.toml",
    "generated/contract/src/lib.rs",
    "generated/adapter/adapter.rs",
    "generated/schema.json",
];

fn fixture_bytes(package: &str, file: &str) -> &'static [u8] {
    match (package, file) {
        ("hello", "boxology.toml") => include_bytes!("../../fixtures/hello/boxology.toml"),
        ("hello", "implementation/Cargo.toml") => {
            include_bytes!("../../fixtures/hello/implementation/Cargo.toml")
        }
        ("hello", "implementation/src/lib.rs") => {
            include_bytes!("../../fixtures/hello/implementation/src/lib.rs")
        }
        ("hello", "implementation/src/contract.rs") => {
            include_bytes!("../../fixtures/hello/implementation/src/contract.rs")
        }
        ("hello", "generated/contract/Cargo.toml") => {
            include_bytes!("../../fixtures/hello/generated/contract/Cargo.toml")
        }
        ("hello", "generated/contract/src/lib.rs") => {
            include_bytes!("../../fixtures/hello/generated/contract/src/lib.rs")
        }
        ("hello", "generated/adapter/adapter.rs") => {
            include_bytes!("../../fixtures/hello/generated/adapter/adapter.rs")
        }
        ("hello", "generated/schema.json") => {
            include_bytes!("../../fixtures/hello/generated/schema.json")
        }
        ("greeter", "boxology.toml") => include_bytes!("../../fixtures/greeter/boxology.toml"),
        ("greeter", "implementation/Cargo.toml") => {
            include_bytes!("../../fixtures/greeter/implementation/Cargo.toml")
        }
        ("greeter", "implementation/src/lib.rs") => {
            include_bytes!("../../fixtures/greeter/implementation/src/lib.rs")
        }
        ("greeter", "implementation/src/contract.rs") => {
            include_bytes!("../../fixtures/greeter/implementation/src/contract.rs")
        }
        ("greeter", "generated/contract/Cargo.toml") => {
            include_bytes!("../../fixtures/greeter/generated/contract/Cargo.toml")
        }
        ("greeter", "generated/contract/src/lib.rs") => {
            include_bytes!("../../fixtures/greeter/generated/contract/src/lib.rs")
        }
        ("greeter", "generated/adapter/adapter.rs") => {
            include_bytes!("../../fixtures/greeter/generated/adapter/adapter.rs")
        }
        ("greeter", "generated/schema.json") => {
            include_bytes!("../../fixtures/greeter/generated/schema.json")
        }
        _ => panic!("unknown fixture file {package}/{file}"),
    }
}

fn fixture_metadata() -> String {
    let members = ["hello", "greeter"]
        .into_iter()
        .flat_map(|id| {
            [
                (format!("{id}/generated/contract"), format!("{id}-contract")),
                (
                    format!("{id}/implementation"),
                    format!("{id}-implementation"),
                ),
            ]
        })
        .collect::<Vec<_>>();
    let ids = members
        .iter()
        .map(|(directory, _)| format!("{:?}", format!("path+file:///w/{directory}#0.0.0")))
        .collect::<Vec<_>>()
        .join(",");
    let packages = members
        .iter()
        .map(|(directory, name)| {
            format!(
                r#"{{"id":{:?},"name":{name:?},"manifest_path":"/w/{directory}/Cargo.toml","dependencies":[]}}"#,
                format!("path+file:///w/{directory}#0.0.0")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"workspace_root":"/w","workspace_members":[{ids}],"packages":[{packages}]}}"#)
}

fn fixture_workspace() -> Workspace {
    let mut files: Vec<String> = ["Cargo.toml", "Cargo.lock", "boxology.toml"]
        .into_iter()
        .map(String::from)
        .collect();
    let mut manifests = vec![(path("boxology.toml"), ROOT_MANIFEST.as_bytes().to_vec())];
    for package in ["hello", "greeter"] {
        files.extend(PROJECT_FILES.iter().map(|file| format!("{package}/{file}")));
        manifests.push((
            path(&format!("{package}/boxology.toml")),
            fixture_bytes(package, "boxology.toml").to_vec(),
        ));
    }
    let files = files
        .into_iter()
        .map(|name| FileEntry::file(path(&name)))
        .collect();
    WorkspaceInputs::new(files, manifests, &fixture_metadata())
        .unwrap()
        .check()
        .unwrap()
}

struct FixtureProjects {
    root: PathBuf,
    plan: GenerationPlan,
}

impl Drop for FixtureProjects {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn materialize_fixture_projects(root: &Path) {
    for package in ["hello", "greeter"] {
        for file in PROJECT_FILES {
            let path = root.join(package).join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, fixture_bytes(package, file)).unwrap();
        }
    }
}

fn fixture_projects() -> FixtureProjects {
    let root = std::env::temp_dir().join(format!(
        "boxology-cli-imports-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    materialize_fixture_projects(&root);
    let plan = plan(&fixture_workspace(), None)
        .unwrap()
        .into_iter()
        .find(|plan| plan.package_id().as_str() == "greeter")
        .unwrap();
    FixtureProjects { root, plan }
}

fn one_pass_root() -> TempRoot {
    TempRoot(std::env::temp_dir().join(format!(
        "boxology-cli-one-pass-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
}

// Keep hello.greet so greeter still generates, but poison the imported revision so a greeter
// tree produced from these bytes cannot match the checked-in fixture adapter/schema.
const STALE_HELLO_SCHEMA: &[u8] = br#"{
  "box_id": "hello",
  "capabilities": [
    {
      "deprecation": null,
      "docs": [],
      "error": "GreetError",
      "id": "hello.greet",
      "idempotency": "none",
      "input": {
        "name": "name",
        "type": "String"
      },
      "max_exposure": "external",
      "name": "greet",
      "output": {
        "type": "String"
      },
      "shape": "unary"
    }
  ],
  "provenance": "@PROVENANCE@",
  "revision": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
  "schema_format": 1,
  "types": [
    {
      "deprecation": null,
      "docs": [],
      "kind": "error",
      "name": "GreetError",
      "variants": [
        {
          "deprecation": null,
          "docs": [],
          "name": "EmptyName",
          "payload": "unit"
        }
      ]
    }
  ]
}
"#;

fn plant_stale_hello_schema(root: &Path) {
    let hello_schema = root.join("hello/generated/schema.json");
    let checked_in_hello = fixture_bytes("hello", "generated/schema.json");
    assert_ne!(STALE_HELLO_SCHEMA, checked_in_hello);
    fs::write(&hello_schema, STALE_HELLO_SCHEMA).unwrap();
    assert_eq!(fs::read(&hello_schema).unwrap(), STALE_HELLO_SCHEMA);
}

fn package_dir_for(root: &Path, plan: &GenerationPlan) -> PathBuf {
    match plan.package_root() {
        Some(package_root) => root.join(package_root.as_str()),
        None => root.to_path_buf(),
    }
}

fn live_generation_request(
    root: &Path,
    plan: &GenerationPlan,
    import_bytes: &BTreeMap<String, Vec<u8>>,
) -> GenerationRequest {
    let package_dir = package_dir_for(root, plan);
    let mut input_paths = plan.inputs().to_vec();
    input_paths.sort_unstable();
    let mut inputs = input_paths
        .into_iter()
        .map(|input| {
            (
                input.as_str().to_owned(),
                fs::read(package_dir.join(input.as_str())).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let raw_imports = plan
        .imports()
        .iter()
        .map(|import| {
            (
                import.package().clone(),
                import.schema().as_str().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    for import in plan.imports() {
        let key = import.schema().as_str().to_owned();
        let bytes = import_bytes
            .get(&key)
            .cloned()
            .unwrap_or_else(|| fs::read(root.join(&key)).unwrap());
        inputs.push((key, bytes));
    }
    GenerationRequest::new(
        plan.package_id().clone(),
        plan.crate_root().as_str().to_owned(),
        inputs,
        raw_imports,
        OUTPUTS.iter().map(|path| (*path).to_owned()).collect(),
    )
    .unwrap()
}

fn write_generated_tree(root: &Path, plan: &GenerationPlan, tree: &GeneratedTree) {
    boxology_generator_writer::write(&package_dir_for(root, plan), tree, plan.outputs()).unwrap();
}

fn package_dir(fixture: &Fixture) -> PathBuf {
    fixture.root.join("ping")
}

fn normalize_adapter(bytes: &[u8]) -> Vec<u8> {
    let text = std::str::from_utf8(bytes).expect("generated Rust is UTF-8");
    assert_eq!(
        text.matches("// Generated by boxology-generator ").count(),
        1,
        "Rust output must have exactly one generator header"
    );
    let (header, body) = text
        .split_once('\n')
        .expect("generated Rust has a header line");
    assert!(header.starts_with("// Generated by boxology-generator "));
    format!("// Generated by boxology-generator @PROVENANCE@\n{body}").into_bytes()
}

const PROVENANCE_ANCHOR: &[u8] = b"  \"provenance\": ";
const PROVENANCE_TOKEN: &[u8] = b"\"@PROVENANCE@\"";

fn occurrence_count(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn normalize_live_schema(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(occurrence_count(bytes, PROVENANCE_ANCHOR), 1);
    let anchor = bytes
        .windows(PROVENANCE_ANCHOR.len())
        .position(|window| window == PROVENANCE_ANCHOR)
        .expect("schema has one top-level provenance anchor");
    let value_start = anchor + PROVENANCE_ANCHOR.len();
    assert_eq!(bytes[value_start], b'{', "live provenance is an object");
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut value_end = None;
    for (offset, byte) in bytes[value_start..].iter().copied().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    value_end = Some(value_start + offset + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let value_end = value_end.expect("live provenance object is complete");
    let mut normalized =
        Vec::with_capacity(bytes.len() - (value_end - value_start) + PROVENANCE_TOKEN.len());
    normalized.extend_from_slice(&bytes[..value_start]);
    normalized.extend_from_slice(PROVENANCE_TOKEN);
    normalized.extend_from_slice(&bytes[value_end..]);
    normalized
}

fn generated_tree_matches_checked_in(root: &Path, package: &str) -> bool {
    [
        "generated/contract/Cargo.toml",
        "generated/contract/src/lib.rs",
        "generated/adapter/adapter.rs",
        "generated/schema.json",
    ]
    .into_iter()
    .all(|file| {
        let Ok(actual) = fs::read(root.join(package).join(file)) else {
            return false;
        };
        let expected = fixture_bytes(package, file);
        match file.rsplit_once('.').map(|(_, extension)| extension) {
            Some("rs") => normalize_adapter(&actual) == normalize_adapter(expected),
            Some("json") => normalize_live_schema(&actual) == expected,
            _ => actual == expected,
        }
    })
}

fn assert_generated_tree_matches_checked_in(root: &Path, package: &str) {
    assert!(
        generated_tree_matches_checked_in(root, package),
        "{package} generated tree must match checked-in fixtures"
    );
}

const CHECKED_IN_HELLO_REVISION: &str =
    "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";
const PLANTED_ZERO_HELLO_REVISION: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// Value-positive oracle for stale-import mutants: Hello converges, Greeter keeps contract/
/// schema bytes, and its adapter embeds the planted zero Hello revision exactly once.
fn assert_stale_import_mutant_outputs(root: &Path) {
    assert_generated_tree_matches_checked_in(root, "hello");

    assert_eq!(
        fs::read(root.join("greeter/generated/contract/Cargo.toml")).unwrap(),
        fixture_bytes("greeter", "generated/contract/Cargo.toml")
    );
    assert_eq!(
        normalize_adapter(&fs::read(root.join("greeter/generated/contract/src/lib.rs")).unwrap(),),
        normalize_adapter(fixture_bytes("greeter", "generated/contract/src/lib.rs"))
    );
    assert_eq!(
        normalize_live_schema(&fs::read(root.join("greeter/generated/schema.json")).unwrap()),
        fixture_bytes("greeter", "generated/schema.json")
    );

    let actual_adapter =
        normalize_adapter(&fs::read(root.join("greeter/generated/adapter/adapter.rs")).unwrap());
    let checked_in_adapter =
        normalize_adapter(fixture_bytes("greeter", "generated/adapter/adapter.rs"));
    assert_eq!(
        occurrence_count(
            checked_in_adapter.as_slice(),
            CHECKED_IN_HELLO_REVISION.as_bytes()
        ),
        1
    );
    let expected_adapter = {
        let text = String::from_utf8(checked_in_adapter).unwrap();
        assert_eq!(text.matches(CHECKED_IN_HELLO_REVISION).count(), 1);
        text.replacen(CHECKED_IN_HELLO_REVISION, PLANTED_ZERO_HELLO_REVISION, 1)
            .into_bytes()
    };
    assert_eq!(
        occurrence_count(
            expected_adapter.as_slice(),
            PLANTED_ZERO_HELLO_REVISION.as_bytes()
        ),
        1
    );
    assert_eq!(
        occurrence_count(
            actual_adapter.as_slice(),
            PLANTED_ZERO_HELLO_REVISION.as_bytes()
        ),
        1
    );
    assert_eq!(actual_adapter, expected_adapter);
}

#[test]
fn imported_fixture_schema_is_hydrated_into_typed_adapter() {
    let fixture = fixture_projects();
    execute(&fixture.root, &fixture.plan).unwrap();
    let adapter = fs::read(fixture.root.join("greeter/generated/adapter/adapter.rs")).unwrap();
    let adapter_text = String::from_utf8(adapter.clone()).unwrap();
    assert_eq!(adapter_text.matches("pub hello: HelloImport").count(), 1);
    let checked_in = include_bytes!("../../fixtures/greeter/generated/adapter/adapter.rs");
    assert_eq!(normalize_adapter(&adapter), normalize_adapter(checked_in));
}

#[test]
fn one_pass_stale_import_converges_to_checked_in_trees() {
    let root = one_pass_root();
    materialize_fixture_projects(&root.0);
    plant_stale_hello_schema(&root.0);
    let plans = plan(&fixture_workspace(), None).unwrap();
    assert_eq!(
        plans
            .iter()
            .map(|plan| plan.package_id().as_str())
            .collect::<Vec<_>>(),
        ["hello", "greeter"]
    );

    for step in execute_plans(&root.0, &plans) {
        step.expect("canonical sequential generate must accept the fixture workspace");
    }

    assert_generated_tree_matches_checked_in(&root.0, "hello");
    assert_generated_tree_matches_checked_in(&root.0, "greeter");
}

#[test]
fn plan_time_import_byte_snapshot_mutant_fails_to_converge() {
    let root = one_pass_root();
    materialize_fixture_projects(&root.0);
    plant_stale_hello_schema(&root.0);
    let plans = plan(&fixture_workspace(), None).unwrap();
    let mut snapshot = BTreeMap::new();
    for plan in &plans {
        for import in plan.imports() {
            let key = import.schema().as_str().to_owned();
            snapshot.insert(key.clone(), fs::read(root.0.join(&key)).unwrap());
        }
    }
    assert_eq!(
        snapshot
            .get("hello/generated/schema.json")
            .map(Vec::as_slice),
        Some(STALE_HELLO_SCHEMA)
    );

    for plan in &plans {
        let tree = generate(live_generation_request(&root.0, plan, &snapshot)).unwrap();
        write_generated_tree(&root.0, plan, &tree);
    }

    assert_stale_import_mutant_outputs(&root.0);
}

#[test]
fn generate_all_before_write_mutant_fails_to_converge() {
    let root = one_pass_root();
    materialize_fixture_projects(&root.0);
    plant_stale_hello_schema(&root.0);
    let plans = plan(&fixture_workspace(), None).unwrap();
    let live = BTreeMap::new();
    let trees = plans
        .iter()
        .map(|plan| generate(live_generation_request(&root.0, plan, &live)).unwrap())
        .collect::<Vec<_>>();
    for (plan, tree) in plans.iter().zip(&trees) {
        write_generated_tree(&root.0, plan, tree);
    }

    assert_stale_import_mutant_outputs(&root.0);
}

#[test]
fn execute_plans_is_terminal_after_first_error() {
    let root = one_pass_root();
    materialize_fixture_projects(&root.0);
    let plans = plan(&fixture_workspace(), None).unwrap();
    assert_eq!(
        plans
            .iter()
            .map(|plan| plan.package_id().as_str())
            .collect::<Vec<_>>(),
        ["hello", "greeter"]
    );

    // Fail Hello before any write by removing a declared package input after planning.
    fs::remove_file(root.0.join("hello/implementation/src/lib.rs")).unwrap();

    const GREETER_SENTINEL: &[u8] = b"greeter-sentinel-must-remain-unchanged\n";
    let greeter_output = root.0.join("greeter/generated/schema.json");
    fs::write(&greeter_output, GREETER_SENTINEL).unwrap();
    assert_eq!(fs::read(&greeter_output).unwrap(), GREETER_SENTINEL);

    let mut steps = execute_plans(&root.0, &plans);
    let first = steps
        .next()
        .expect("first plan must yield a result")
        .expect_err("hello must fail before writing");
    assert_eq!(first.code(), "BXW0070");
    assert!(steps.next().is_none());
    assert!(steps.next().is_none());
    assert_eq!(fs::read(&greeter_output).unwrap(), GREETER_SENTINEL);
}

fn request(fixture: &Fixture) -> GenerationRequest {
    let package = package_dir(fixture);
    let mut paths = fixture.plan.inputs().to_vec();
    paths.sort_unstable();
    let inputs = paths
        .into_iter()
        .map(|path| {
            (
                path.as_str().to_owned(),
                fs::read(package.join(path.as_str())).unwrap(),
            )
        })
        .collect();
    GenerationRequest::new(
        fixture.plan.package_id().clone(),
        fixture.plan.crate_root().as_str().to_owned(),
        inputs,
        Vec::new(),
        OUTPUTS.iter().map(|path| (*path).to_owned()).collect(),
    )
    .unwrap()
}

fn generated_paths() -> Vec<String> {
    let mut paths = OUTPUTS
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    paths.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    paths
}

#[test]
fn first_run_writes_exact_outputs_from_exact_input_bytes() {
    let fixture = fixture("[\"generated/**\"]");
    let expected = generate(request(&fixture)).unwrap();
    let outcome = execute(&fixture.root, &fixture.plan).unwrap();
    assert_eq!(outcome.written(), generated_paths().as_slice());
    assert!(outcome.removed().is_empty());
    assert_eq!(
        expected
            .files()
            .iter()
            .map(|file| file.path())
            .collect::<Vec<_>>(),
        generated_paths()
    );
    for file in expected.files() {
        assert_eq!(
            fs::read(package_dir(&fixture).join(file.path())).unwrap(),
            file.bytes()
        );
    }
}

#[test]
fn second_run_is_unchanged() {
    let fixture = fixture("[\"generated/**\"]");
    execute(&fixture.root, &fixture.plan).unwrap();
    let outcome = execute(&fixture.root, &fixture.plan).unwrap();
    assert!(outcome.is_unchanged());
    assert!(outcome.written().is_empty());
    assert!(outcome.removed().is_empty());
}

#[test]
fn tamper_is_repaired_exactly() {
    let fixture = fixture("[\"generated/**\"]");
    let expected = generate(request(&fixture)).unwrap();
    execute(&fixture.root, &fixture.plan).unwrap();
    let target = &expected.files()[0];
    fs::write(package_dir(&fixture).join(target.path()), b"tampered").unwrap();
    let outcome = execute(&fixture.root, &fixture.plan).unwrap();
    assert_eq!(outcome.written(), &[target.path().to_owned()]);
    assert!(outcome.removed().is_empty());
    assert_eq!(
        fs::read(package_dir(&fixture).join(target.path())).unwrap(),
        target.bytes()
    );
}

#[test]
fn stale_declared_output_is_pruned_and_neighbor_survives() {
    let fixture = fixture("[\"generated/**\"]");
    execute(&fixture.root, &fixture.plan).unwrap();
    fs::write(package_dir(&fixture).join("generated/stale.json"), b"stale").unwrap();
    fs::write(package_dir(&fixture).join("neighbor.txt"), b"keep").unwrap();
    let outcome = execute(&fixture.root, &fixture.plan).unwrap();
    assert_eq!(outcome.removed(), &["generated/stale.json".to_owned()]);
    assert_eq!(
        fs::read(package_dir(&fixture).join("neighbor.txt")).unwrap(),
        b"keep"
    );
}

#[test]
fn generator_diagnostics_are_rendered_verbatim() {
    let fixture = fixture("[\"generated/**\"]");
    fs::write(
        package_dir(&fixture).join("implementation/src/contract.rs"),
        b"this is not a contract",
    )
    .unwrap();
    let expected = generate(request(&fixture)).unwrap_err();
    let error = execute(&fixture.root, &fixture.plan).unwrap_err();
    assert_eq!(error.code(), "BXW0071");
    assert_eq!(
        error.diagnostics().unwrap().to_string(),
        expected.to_string()
    );
    assert!(!package_dir(&fixture).join("generated").exists());
}

#[test]
fn underdeclared_output_is_rejected_before_any_write() {
    let fixture = fixture("[\"generated/contract/**\"]");
    let error = execute(&fixture.root, &fixture.plan).unwrap_err();
    let location = package_dir(&fixture).join("generated/adapter/adapter.rs");
    assert_eq!(error.code(), "BXW0073");
    assert_eq!(error.location(), location.as_path());
    assert_eq!(error.path(), location.as_path());
    assert_eq!(
        error.to_string(),
        format!("BXW0073 {location:?}: {}", error.detail())
    );
    assert!(!package_dir(&fixture).join("generated").exists());
}

#[cfg(unix)]
#[test]
fn symlink_input_is_rejected_without_writing() {
    use std::os::unix::fs::symlink;
    let fixture = fixture("[\"generated/**\"]");
    let input = package_dir(&fixture).join("implementation/src/lib.rs");
    let outside = fixture.root.join("outside.rs");
    fs::write(&outside, CONTRACT).unwrap();
    fs::remove_file(&input).unwrap();
    symlink(&outside, &input).unwrap();
    let error = execute(&fixture.root, &fixture.plan).unwrap_err();
    assert_eq!(error.code(), "BXW0070");
    assert_eq!(error.location(), input.as_path());
    assert_eq!(
        error.detail(),
        "a generation input must be a readable regular file"
    );
    assert_eq!(
        error.to_string(),
        format!("BXW0070 {input:?}: {}", error.detail())
    );
    assert!(!package_dir(&fixture).join("generated").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_input_parent_is_rejected_without_writing() {
    use std::os::unix::fs::symlink;
    let fixture = fixture("[\"generated/**\"]");
    let parent = package_dir(&fixture).join("implementation/src");
    let outside = fixture.root.join("outside-src");
    fs::rename(&parent, &outside).unwrap();
    symlink(&outside, &parent).unwrap();
    let error = execute(&fixture.root, &fixture.plan).unwrap_err();
    assert_eq!(error.code(), "BXW0070");
    assert_eq!(error.location(), parent.as_path());
    assert!(!package_dir(&fixture).join("generated").exists());
}

#[test]
fn first_run_has_no_base_and_tree_submitted() {
    let fixture = fixture("[\"generated/**\"]");
    let expected = generate(request(&fixture)).unwrap();
    let submitted = expected
        .files()
        .iter()
        .find(|file| file.path() == "generated/schema.json")
        .unwrap()
        .bytes();
    let outcome = execute(&fixture.root, &fixture.plan).unwrap();
    assert!(outcome.base_schema().is_none());
    assert_eq!(outcome.submitted_schema(), submitted);
}

#[test]
fn base_schema_is_the_pre_write_bytes() {
    let fixture = fixture("[\"generated/**\"]");
    execute(&fixture.root, &fixture.plan).unwrap();
    let schema = package_dir(&fixture).join("generated/schema.json");
    fs::write(&schema, b"tampered base").unwrap();
    let expected = generate(request(&fixture)).unwrap();
    let submitted = expected
        .files()
        .iter()
        .find(|file| file.path() == "generated/schema.json")
        .unwrap()
        .bytes();
    let outcome = execute(&fixture.root, &fixture.plan).unwrap();
    assert_eq!(outcome.base_schema(), Some(b"tampered base".as_slice()));
    assert_eq!(outcome.submitted_schema(), submitted);
}

#[test]
fn unchanged_run_still_captures_base() {
    let fixture = fixture("[\"generated/**\"]");
    execute(&fixture.root, &fixture.plan).unwrap();
    let outcome = execute(&fixture.root, &fixture.plan).unwrap();
    assert!(outcome.is_unchanged());
    assert_eq!(outcome.base_schema(), Some(outcome.submitted_schema()));
}

#[test]
fn schema_path_is_a_generator_output_exactly_once() {
    assert_eq!(
        OUTPUTS
            .iter()
            .filter(|path| **path == "generated/schema.json")
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn symlinked_checked_in_schema_is_refused() {
    use std::os::unix::fs::symlink;
    let fixture = fixture("[\"generated/**\"]");
    execute(&fixture.root, &fixture.plan).unwrap();
    let package = package_dir(&fixture);
    let prior: Vec<_> = OUTPUTS
        .iter()
        .filter(|path| **path != "generated/schema.json")
        .map(|path| ((*path).to_owned(), fs::read(package.join(path)).unwrap()))
        .collect();
    let schema = package.join("generated/schema.json");
    let outside = fixture.root.join("outside-schema.json");
    fs::rename(&schema, &outside).unwrap();
    symlink(&outside, &schema).unwrap();
    let error = execute(&fixture.root, &fixture.plan).unwrap_err();
    assert_eq!(error.code(), "BXW0076");
    assert_eq!(error.location(), schema.as_path());
    assert_eq!(
        error.detail(),
        "the checked-in schema document must be a readable regular file"
    );
    for (path, bytes) in prior {
        assert_eq!(fs::read(package.join(path)).unwrap(), bytes);
    }
}
