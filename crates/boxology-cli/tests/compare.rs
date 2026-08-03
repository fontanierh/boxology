use boxology_cli::{
    CompareDifference, DifferenceKind, GenerationPlan, compare_plans, compare_step, plan,
};
use boxology_contract::BoxId;
use boxology_generator::{OUTPUTS, generate};
use boxology_generator_model::GenerationRequest;
use boxology_manifest::RelativePath;
use boxology_workspace::{FileEntry, Workspace, WorkspaceInputs};
use std::{
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
const METADATA: &str = r#"{"workspace_root":"/w","workspace_members":["path+file:///w/ping/generated/contract#0.0.0","path+file:///w/ping/implementation#0.0.0"],"packages":[{"id":"path+file:///w/ping/generated/contract#0.0.0","name":"ping-contract","manifest_path":"/w/ping/generated/contract/Cargo.toml","dependencies":[]},{"id":"path+file:///w/ping/implementation#0.0.0","name":"ping-implementation","manifest_path":"/w/ping/implementation/Cargo.toml","dependencies":[]}] }"#;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    workspace: Workspace,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn path(value: &str) -> RelativePath {
    RelativePath::new(value).unwrap()
}

fn id(value: &str) -> BoxId {
    BoxId::new(value).unwrap()
}

fn package_manifest(outputs: &str) -> String {
    format!(
        "schema = 1\nid = \"ping\"\nkind = \"box\"\nowned = [\"boxology.toml\", \"implementation/**\"]\n\n[[crates]]\ncargo_package = \"ping-implementation\"\npath = \"implementation\"\nrole = \"box-implementation\"\n\n[[crates]]\ncargo_package = \"ping-contract\"\npath = \"generated/contract\"\nrole = \"box-contract\"\n\n[[derived]]\nid = \"contract\"\ngenerator = \"boxology-contract\"\ninputs = [\"boxology.toml\", \"implementation/src/**\"]\noutputs = {outputs}\n"
    )
}

fn workspace(stale: bool) -> Workspace {
    let mut names: Vec<String> = [
        "Cargo.toml",
        "Cargo.lock",
        "boxology.toml",
        "ping/boxology.toml",
        "ping/implementation/Cargo.toml",
        "ping/implementation/src/lib.rs",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    names.extend(OUTPUTS.iter().map(|output| format!("ping/{output}")));
    if stale {
        names.push("ping/generated/aaa-stale.json".to_owned());
    }
    let files = names
        .iter()
        .map(|name| FileEntry::file(path(name)))
        .collect();
    let manifests = vec![
        (path("boxology.toml"), ROOT_MANIFEST.as_bytes().to_vec()),
        (
            path("ping/boxology.toml"),
            package_manifest("[\"generated/**\"]").into_bytes(),
        ),
    ];
    WorkspaceInputs::new(files, manifests, METADATA)
        .unwrap()
        .check()
        .unwrap()
}

fn root_only_workspace() -> Workspace {
    WorkspaceInputs::new(
        ["Cargo.toml", "Cargo.lock", "boxology.toml"]
            .into_iter()
            .map(|name| FileEntry::file(path(name)))
            .collect(),
        vec![(path("boxology.toml"), ROOT_MANIFEST.as_bytes().to_vec())],
        r#"{"workspace_root":"/w","workspace_members":[],"packages":[]}"#,
    )
    .unwrap()
    .check()
    .unwrap()
}

fn request(root: &Path, plan: &GenerationPlan) -> GenerationRequest {
    let package = root.join("ping");
    let mut paths = plan.inputs().to_vec();
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
        plan.package_id().clone(),
        plan.crate_root().as_str().to_owned(),
        inputs,
        Vec::new(),
        OUTPUTS.iter().map(|output| (*output).to_owned()).collect(),
    )
    .unwrap()
}

fn fixture(stale: bool) -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "boxology-cli-compare-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("ping/implementation/src")).unwrap();
    fs::write(
        root.join("ping/boxology.toml"),
        package_manifest("[\"generated/**\"]"),
    )
    .unwrap();
    fs::write(root.join("ping/implementation/src/lib.rs"), CONTRACT).unwrap();
    let workspace = workspace(stale);
    let [plan] = plan(&workspace, None).unwrap().try_into().unwrap();
    let tree = generate(&request(&root, &plan)).unwrap();
    for file in tree.files() {
        let target = root.join("ping").join(file.path());
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, file.bytes()).unwrap();
    }
    if stale {
        fs::write(root.join("ping/generated/aaa-stale.json"), b"stale").unwrap();
    }
    Fixture { root, workspace }
}

fn package_dir(fixture: &Fixture) -> PathBuf {
    fixture.root.join("ping")
}

fn assert_difference(difference: &CompareDifference, path: &str, kind: DifferenceKind) {
    assert_eq!(difference.package(), &id("ping"));
    assert_eq!(difference.path().as_str(), path);
    assert_eq!(difference.kind(), kind);
    assert_eq!(difference.code(), "BXW0083");
    assert_eq!(
        difference.detail(),
        "a checked-in derived artifact must be byte-identical to regeneration; regenerate the accountable package with boxology generate --package <id>"
    );
    assert_eq!(
        difference.rule_source(),
        "specs/s5-manifest-and-validation.md D6; boxology-details/08-rust-build-topology.md workspace operations and validation baseline step 2"
    );
}

#[test]
fn difference_kind_names_are_frozen() {
    assert_eq!(DifferenceKind::Missing.as_str(), "missing");
    assert_eq!(DifferenceKind::Differing.as_str(), "differing");
    assert_eq!(DifferenceKind::Stale.as_str(), "stale");
}

#[test]
fn clean_workspace_compares_empty() {
    let fixture = fixture(false);
    let differences = compare_step(&fixture.root, &fixture.workspace).unwrap();
    assert!(differences.is_empty());
    let plans = plan(&fixture.workspace, None).unwrap();
    assert!(
        compare_plans(&fixture.root, &fixture.workspace, &plans)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn compare_plans_accepts_a_different_checked_workspace_without_panicking() {
    let fixture = fixture(false);
    let plans = plan(&fixture.workspace, None).unwrap();
    assert!(
        compare_plans(&fixture.root, &root_only_workspace(), &plans)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn tampered_output_bytes_are_differing() {
    let fixture = fixture(false);
    let target = package_dir(&fixture).join("generated/contract/src/lib.rs");
    let mut bytes = fs::read(&target).unwrap();
    bytes[0] ^= 1;
    fs::write(&target, bytes).unwrap();

    let differences = compare_step(&fixture.root, &fixture.workspace).unwrap();
    assert_eq!(differences.len(), 1);
    assert_difference(
        &differences[0],
        "generated/contract/src/lib.rs",
        DifferenceKind::Differing,
    );
    assert_eq!(
        differences[0].repair_command(),
        "boxology generate --package ping"
    );
}

#[test]
fn deleted_output_is_missing() {
    let fixture = fixture(false);
    fs::remove_file(package_dir(&fixture).join("generated/adapter/adapter.rs")).unwrap();

    let differences = compare_step(&fixture.root, &fixture.workspace).unwrap();
    assert_eq!(differences.len(), 1);
    assert_difference(
        &differences[0],
        "generated/adapter/adapter.rs",
        DifferenceKind::Missing,
    );
}

#[test]
fn stale_declared_output_is_stale() {
    let fixture = fixture(true);
    let differences = compare_step(&fixture.root, &fixture.workspace).unwrap();
    assert_eq!(differences.len(), 1);
    assert_difference(
        &differences[0],
        "generated/aaa-stale.json",
        DifferenceKind::Stale,
    );
}

#[test]
fn differences_are_sorted() {
    let fixture = fixture(true);
    let target = package_dir(&fixture).join("generated/contract/src/lib.rs");
    let mut bytes = fs::read(&target).unwrap();
    bytes[0] ^= 1;
    fs::write(&target, bytes).unwrap();

    let differences = compare_step(&fixture.root, &fixture.workspace).unwrap();
    assert_eq!(differences.len(), 2);
    assert_difference(
        &differences[0],
        "generated/aaa-stale.json",
        DifferenceKind::Stale,
    );
    assert_difference(
        &differences[1],
        "generated/contract/src/lib.rs",
        DifferenceKind::Differing,
    );
}

#[cfg(unix)]
#[test]
fn unreadable_artifact_is_differing() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture(false);
    let target = package_dir(&fixture).join("generated/contract/src/lib.rs");
    let original = fs::metadata(&target).unwrap().permissions().mode();
    let mut unreadable = fs::metadata(&target).unwrap().permissions();
    unreadable.set_mode(original & !0o777);
    fs::set_permissions(&target, unreadable).unwrap();
    let result = compare_step(&fixture.root, &fixture.workspace);
    let mut restored = fs::metadata(&target).unwrap().permissions();
    restored.set_mode(original);
    fs::set_permissions(&target, restored).unwrap();

    let differences = result.unwrap();
    assert_eq!(differences.len(), 1);
    assert_difference(
        &differences[0],
        "generated/contract/src/lib.rs",
        DifferenceKind::Differing,
    );
}
