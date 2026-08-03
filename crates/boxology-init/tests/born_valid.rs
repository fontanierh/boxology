#![cfg(unix)]

use boxology_init::{InitRequest, initialize};
use boxology_manifest::{Kind, Manifest, RelativePath};
use boxology_workspace::{FileEntry, WorkspaceInputs};
use std::{
    collections::BTreeMap,
    env, fs,
    fs::File,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const TOTAL_TIMEOUT: Duration = Duration::from_secs(420);
static NEXT_LOG: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    cleanup: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock follows Unix epoch")
            .as_nanos();
        let cleanup = env::temp_dir().join(format!(
            "boxology-born-valid-{}-{stamp}",
            std::process::id()
        ));
        let root = cleanup.join("project");
        fs::create_dir_all(&root).expect("create generated-project root");
        Self { root, cleanup }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.cleanup);
    }
}

fn run(deadline: Instant, root: &Path, program: &Path, args: &[&str]) -> String {
    let id = NEXT_LOG.fetch_add(1, Ordering::Relaxed);
    let capture = root.parent().expect("fixture root has a cleanup parent");
    let stdout_path = capture.join(format!("born-valid-{id}.stdout"));
    let stderr_path = capture.join(format!("born-valid-{id}.stderr"));
    let stdout = File::create(&stdout_path).expect("create command stdout capture");
    let stderr = File::create(&stderr_path).expect("create command stderr capture");
    let cargo_target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let mut child = Command::new(program);
    child
        .args(args)
        .current_dir(root)
        .process_group(0)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if program.file_name().is_some_and(|name| name == "cargo") {
        child.env("CARGO_TARGET_DIR", cargo_target);
    }
    let mut child = child
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {} {args:?}: {error}", program.display()));

    let status = loop {
        match child.try_wait().expect("poll child") {
            Some(status) => break status,
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            None => {
                let group = format!("-{}", child.id());
                let _ = Command::new("/bin/kill").args(["-KILL", &group]).status();
                let _ = child.kill();
                let _ = child.wait();
                panic!("timed out running {} {args:?}", program.display());
            }
        }
    };
    let stdout = fs::read_to_string(&stdout_path).expect("read command stdout");
    let stderr = fs::read_to_string(&stderr_path).expect("read command stderr");
    fs::remove_file(stdout_path).expect("remove stdout capture");
    fs::remove_file(stderr_path).expect("remove stderr capture");
    assert!(
        status.success(),
        "{} {args:?} failed with {status}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        program.display()
    );
    stdout
}

fn quality_commands(root: &Path, logical: &str) -> Vec<String> {
    let path = root.join(logical);
    let bytes = fs::read(&path).expect("read generated manifest");
    Manifest::parse(
        RelativePath::new(logical).expect("manifest logical path is valid"),
        &bytes,
    )
    .expect("generated manifest parses")
    .quality_commands()
    .to_vec()
}

fn run_quality(deadline: Instant, root: &Path, command: &str) {
    let words: Vec<_> = command.split_ascii_whitespace().collect();
    assert_eq!(words.first(), Some(&"cargo"), "quality command is Cargo");
    run(deadline, root, Path::new("cargo"), &words[1..]);
}

#[test]
fn initialized_project_is_born_valid_and_regeneration_is_a_no_op() {
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    let fixture = Fixture::new();
    let root = &fixture.root;
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("boxology-init is under the workspace crates directory")
        .canonicalize()
        .expect("canonicalize current source checkout");
    let source_text = source.to_str().expect("source checkout path is UTF-8");

    run(deadline, root, Path::new("git"), &["init", "--quiet"]);
    let init = run(
        deadline,
        root,
        Path::new(env!("CARGO_BIN_EXE_boxology-init")),
        &[
            "--name",
            "example",
            "--dependency-source",
            source_text,
            "--target",
            root.to_str().expect("temporary path is UTF-8"),
        ],
    );
    assert_eq!(init, "initialized example\n");

    let request = InitRequest::new("example", source_text).expect("absolute request is valid");
    let tree = initialize(&request).expect("absolute-source tree initializes");
    let initialized: BTreeMap<_, _> = tree
        .files()
        .iter()
        .map(|file| {
            let bytes = fs::read(root.join(file.path())).expect("read initialized file");
            assert_eq!(bytes, file.bytes(), "initializer bytes for {}", file.path());
            (file.path().to_owned(), bytes)
        })
        .collect();

    run(deadline, root, Path::new("git"), &["add", "."]);
    run(
        deadline,
        root,
        Path::new("git"),
        &[
            "-c",
            "user.name=Boxology Test",
            "-c",
            "user.email=boxology@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "Initialize Boxology project",
        ],
    );

    assert!(!root.join("Cargo.lock").exists());
    run(deadline, root, Path::new("cargo"), &["build"]);
    assert!(root.join("Cargo.lock").is_file());

    let metadata = run(
        deadline,
        root,
        Path::new("cargo"),
        &["metadata", "--format-version", "1", "--no-deps"],
    );
    let mut files: Vec<_> = initialized
        .keys()
        .map(|logical| {
            FileEntry::file(RelativePath::new(logical).expect("initialized path is valid"))
        })
        .collect();
    files.push(FileEntry::file(
        RelativePath::new("Cargo.lock").expect("lockfile path is valid"),
    ));
    let manifests = initialized
        .iter()
        .filter(|(logical, _)| {
            logical.as_str() == "boxology.toml" || logical.ends_with("/boxology.toml")
        })
        .map(|(logical, bytes)| {
            (
                RelativePath::new(logical).expect("manifest path is valid"),
                bytes.clone(),
            )
        })
        .collect();
    let workspace = WorkspaceInputs::new(files, manifests, &metadata)
        .expect("generated listing has unique paths")
        .check()
        .expect("generated workspace classifies");
    let lockfile = workspace
        .classifications()
        .iter()
        .find(|classification| classification.path().as_str() == "Cargo.lock")
        .expect("materialized lockfile is classified");
    assert_eq!(lockfile.package().as_str(), "example");
    assert_eq!(
        lockfile.derived_output().map(|output| output.as_str()),
        Some("lockfile")
    );
    let platform = workspace
        .packages()
        .iter()
        .find(|package| package.id() == lockfile.package())
        .expect("lockfile owner is a discovered package");
    assert_eq!(platform.manifest().kind(), Kind::Platform);

    let mut commands = quality_commands(root, "ping/boxology.toml");
    commands.extend(quality_commands(root, "app/boxology.toml"));
    assert_eq!(
        commands,
        [
            "cargo check -p ping-contract --all-features",
            "cargo test -p ping-implementation",
            "cargo test -p ping-app tests::assembled_ping_answers_in_process_and_over_real_http -- --exact",
        ]
    );
    for command in &commands {
        run_quality(deadline, root, command);
    }

    let source_manifest = source.join("Cargo.toml");
    let source_manifest = source_manifest
        .to_str()
        .expect("source manifest path is UTF-8");
    let check = run(
        deadline,
        root,
        Path::new("cargo"),
        &[
            "run",
            "--quiet",
            "--manifest-path",
            source_manifest,
            "-p",
            "boxology-cli",
            "--",
            "check",
        ],
    );
    assert!(check.contains("check regeneration passed"), "{check}");
    let generate = run(
        deadline,
        root,
        Path::new("cargo"),
        &[
            "run",
            "--quiet",
            "--manifest-path",
            source_manifest,
            "-p",
            "boxology-cli",
            "--",
            "generate",
        ],
    );
    assert_eq!(
        generate,
        "generate ping unchanged\ngenerate result unchanged\n"
    );
    for (logical, before) in initialized {
        assert_eq!(
            fs::read(root.join(&logical)).expect("read regenerated file"),
            before,
            "regeneration changed initialized bytes for {logical}"
        );
    }
}
