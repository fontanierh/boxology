use boxology_cli::{GenerationPlan, execute, plan};
use boxology_generator::{OUTPUTS, generate};
use boxology_generator_model::GenerationRequest;
use boxology_manifest::RelativePath;
use boxology_workspace::{FileEntry, Workspace, WorkspaceInputs};
use std::{
    fs,
    path::PathBuf,
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
    fs::write(root.join("ping/implementation/src/lib.rs"), CONTRACT).unwrap();
    let [plan] = plan(&workspace(outputs), None).unwrap().try_into().unwrap();
    Fixture { root, plan }
}

fn package_dir(fixture: &Fixture) -> PathBuf {
    fixture.root.join("ping")
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
    let expected = generate(&request(&fixture)).unwrap();
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
    let expected = generate(&request(&fixture)).unwrap();
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
        package_dir(&fixture).join("implementation/src/lib.rs"),
        b"this is not a contract",
    )
    .unwrap();
    let expected = generate(&request(&fixture)).unwrap_err();
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
    let expected = generate(&request(&fixture)).unwrap();
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
    let expected = generate(&request(&fixture)).unwrap();
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
