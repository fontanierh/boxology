use boxology_cli::{
    CapturedOutput, CommandRunner, CommandSpec, SpawnError, cargo_metadata_command, clippy_spec,
    fmt_packages, fmt_spec, lock_spec, quality_specs, run_clippy_step, run_command, run_fmt_step,
    run_lock_step, run_quality_step, run_test_step, test_spec, walk,
};
use boxology_manifest::RelativePath;
use boxology_workspace::{Completion, FileEntry, Workspace, WorkspaceInputs};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

fn rel(path: &str) -> RelativePath {
    RelativePath::new(path).unwrap()
}

fn fixture_workspace() -> Workspace {
    fixture_workspace_with_quality("", "")
}

fn fixture_workspace_with_quality(root_quality: &str, ping_quality: &str) -> Workspace {
    let root = format!(
        "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n{root_quality}"
    );
    let ping = format!(
        "schema = 1\nid = \"ping\"\nkind = \"box\"\nowned = [\"boxology.toml\", \"implementation/**\"]\n\n[[crates]]\ncargo_package = \"ping-implementation\"\npath = \"implementation\"\nrole = \"box-implementation\"\n\n[[crates]]\ncargo_package = \"ping-contract\"\npath = \"generated/contract\"\nrole = \"box-contract\"\n\n[[derived]]\nid = \"contract\"\ngenerator = \"boxology-contract\"\ninputs = [\"boxology.toml\", \"implementation/src/**\"]\noutputs = [\"generated/**\"]\n{ping_quality}"
    );
    const METADATA: &str = r#"{"workspace_root":"/w","workspace_members":["a","b"],"packages":[{"id":"a","name":"ping-contract","manifest_path":"/w/ping/generated/contract/Cargo.toml","dependencies":[]},{"id":"b","name":"ping-implementation","manifest_path":"/w/ping/implementation/Cargo.toml","dependencies":[]}]}"#;
    let files = [
        "Cargo.toml",
        "Cargo.lock",
        "boxology.toml",
        "ping/boxology.toml",
        "ping/implementation/Cargo.toml",
        "ping/implementation/src/lib.rs",
        "ping/generated/contract/Cargo.toml",
        "ping/generated/contract/src/lib.rs",
    ]
    .into_iter()
    .map(|path| FileEntry::file(rel(path)))
    .collect();
    let manifests = [
        (rel("boxology.toml"), root.into_bytes()),
        (rel("ping/boxology.toml"), ping.into_bytes()),
    ]
    .to_vec();
    WorkspaceInputs::new(files, manifests, METADATA)
        .unwrap()
        .check()
        .unwrap()
}

fn quality_workspace() -> Workspace {
    fixture_workspace_with_quality(
        "\n[quality]\ncommands = [\"tool-a one two\", \"tool-b three\"]\n",
        "\n[quality]\ncommands = [\"ping-cmd --flag value\", \"ping-cmd2 arg\"]\n",
    )
}

#[test]
fn fmt_clippy_and_test_command_construction_is_exact() {
    let workspace = fixture_workspace();
    assert_eq!(fmt_packages(&workspace), ["ping-implementation"]);
    let spec = fmt_spec(&workspace).unwrap();
    assert_eq!(spec.args(), ["fmt", "--check", "-p", "ping-implementation"]);
    assert_eq!(spec.render(), "cargo fmt --check -p ping-implementation");
    assert_eq!(
        clippy_spec().render(),
        "cargo clippy --workspace --all-targets --all-features -- -D warnings"
    );
    assert_eq!(
        test_spec().render(),
        "cargo test --workspace --all-features"
    );
    assert_eq!(
        lock_spec().render(),
        "cargo metadata --format-version 1 --locked"
    );
    assert_eq!(
        lock_spec().args(),
        ["metadata", "--format-version", "1", "--locked"]
    );
}

#[test]
fn real_root_fmt_selection_excludes_standalone_fixture_and_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let walked = walk(&root).unwrap();
    let meta = cargo_metadata_command(&root).output().unwrap();
    assert!(meta.status.success());
    let metadata = String::from_utf8(meta.stdout).unwrap();
    let workspace = WorkspaceInputs::new(
        walked.files().to_vec(),
        walked.manifests().to_vec(),
        &metadata,
    )
    .unwrap()
    .check()
    .unwrap();
    let packages = fmt_packages(&workspace);
    assert!(
        !packages.iter().any(|n| n == "generated-style-fmt"),
        "{packages:?}"
    );
    let out = run_command(&root, &fmt_spec(&workspace).unwrap()).unwrap();
    assert!(
        out.success(),
        "{}",
        String::from_utf8_lossy(&out.combined())
    );
}

#[test]
fn real_root_lock_spec_passes_through_run_command() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let out = run_command(&root, &lock_spec()).unwrap();
    assert!(
        out.success(),
        "{}",
        String::from_utf8_lossy(&out.combined())
    );
}

#[test]
fn injected_zero_exit_passes_without_captured_output() {
    let runner: &CommandRunner =
        &|_root, _spec| Ok(CapturedOutput::new(true, b"ok-stdout", b"ok-stderr"));
    let workspace = fixture_workspace();
    let (lock, lock_out) = run_lock_step(runner, Path::new(".")).unwrap().into_parts();
    let (fmt, fmt_out) = run_fmt_step(runner, Path::new("."), &workspace)
        .unwrap()
        .into_parts();
    let (clippy, clippy_out) = run_clippy_step(runner, Path::new("."))
        .unwrap()
        .into_parts();
    let (tests, tests_out) = run_test_step(runner, Path::new(".")).unwrap().into_parts();
    assert_eq!(lock, Completion::Passed);
    assert_eq!(fmt, Completion::Passed);
    assert_eq!(clippy, Completion::Passed);
    assert_eq!(tests, Completion::Passed);
    assert_eq!(lock_out, None);
    assert_eq!(fmt_out, None);
    assert_eq!(clippy_out, None);
    assert_eq!(tests_out, None);
}

#[test]
fn nonzero_exit_is_coded_failure_with_command_and_output() {
    let runner: &CommandRunner = &|_root, spec| {
        Ok(CapturedOutput::new(
            false,
            format!("failed:{}\n", spec.render()),
            b"stderr-line\n".as_slice(),
        ))
    };
    let (completion, output) = run_clippy_step(runner, Path::new("."))
        .unwrap()
        .into_parts();
    let Completion::Failed(findings) = completion else {
        panic!("expected failure");
    };
    assert_eq!(
        findings.to_string(),
        "BXW0094 Cargo.toml package= candidates=[command=\"cargo clippy --workspace --all-targets --all-features -- -D warnings\"]"
    );
    assert_eq!(
        String::from_utf8(output.unwrap()).unwrap(),
        "failed:cargo clippy --workspace --all-targets --all-features -- -D warnings\nstderr-line\n"
    );
}

#[test]
fn test_nonzero_exit_is_bxw0095_with_command_and_output() {
    let runner: &CommandRunner = &|_root, spec| {
        Ok(CapturedOutput::new(
            false,
            format!("failed:{}\n", spec.render()),
            b"stderr-line\n".as_slice(),
        ))
    };
    let (completion, output) = run_test_step(runner, Path::new(".")).unwrap().into_parts();
    let Completion::Failed(findings) = completion else {
        panic!("expected failure");
    };
    assert_eq!(
        findings.to_string(),
        "BXW0095 Cargo.toml package= candidates=[command=\"cargo test --workspace --all-features\"]"
    );
    assert_eq!(
        String::from_utf8(output.unwrap()).unwrap(),
        "failed:cargo test --workspace --all-features\nstderr-line\n"
    );
}

#[test]
fn lock_nonzero_exit_is_bxw0097_with_cargo_lock_path() {
    let runner: &CommandRunner = &|_root, spec| {
        Ok(CapturedOutput::new(
            false,
            format!("failed:{}\n", spec.render()),
            b"stderr-line\n".as_slice(),
        ))
    };
    let (completion, output) = run_lock_step(runner, Path::new(".")).unwrap().into_parts();
    let Completion::Failed(findings) = completion else {
        panic!("expected failure");
    };
    assert_eq!(
        findings.to_string(),
        "BXW0097 Cargo.lock package= candidates=[command=\"cargo metadata --format-version 1 --locked\"]"
    );
    assert_eq!(
        String::from_utf8(output.unwrap()).unwrap(),
        "failed:cargo metadata --format-version 1 --locked\nstderr-line\n"
    );
}

#[test]
fn spawn_and_missing_program_map_to_invocation_error() {
    let runner: &CommandRunner = &|_root, _spec| Err(SpawnError);
    assert_eq!(
        run_lock_step(runner, Path::new("."))
            .unwrap_err()
            .to_string(),
        "BXW0096 Cargo.toml: a trusted check command could not be executed"
    );
    assert_eq!(
        run_clippy_step(runner, Path::new("."))
            .unwrap_err()
            .to_string(),
        "BXW0096 Cargo.toml: a trusted check command could not be executed"
    );
    assert_eq!(
        run_test_step(runner, Path::new("."))
            .unwrap_err()
            .to_string(),
        "BXW0096 Cargo.toml: a trusted check command could not be executed"
    );
    assert!(
        run_command(
            Path::new("."),
            &CommandSpec::new("true", Vec::<String>::new())
        )
        .unwrap()
        .success()
    );
    assert!(
        !run_command(
            Path::new("."),
            &CommandSpec::new("false", Vec::<String>::new())
        )
        .unwrap()
        .success()
    );
    assert_eq!(
        run_command(
            Path::new("."),
            &CommandSpec::new("boxology-missing-tool-9f3a2c", Vec::<String>::new()),
        )
        .unwrap_err()
        .to_string(),
        "BXW0096 Cargo.toml: a trusted check command could not be executed"
    );
}

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch_workspace() -> Scratch {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("boxology-cli-lock-{}-{}", std::process::id(), id));
    fs::create_dir_all(root.join("crates/a/src")).unwrap();
    fs::create_dir_all(root.join("crates/b/src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/a\", \"crates/b\"]\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/a/Cargo.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nb = { path = \"../b\" }\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/a/src/lib.rs"),
        "pub fn a() { let _ = b::b(); }\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/b/Cargo.toml"),
        "[package]\nname = \"b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(
        root.join("crates/b/src/lib.rs"),
        "pub fn b() -> u32 { 1 }\n",
    )
    .unwrap();
    // Scratch-only unlocked resolution to create Cargo.lock; never against the repository root.
    let generated = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    assert!(root.join("Cargo.lock").is_file());
    Scratch(root)
}

fn discovery_locked_no_deps(root: &Path) -> std::process::Output {
    Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--locked", "--no-deps"])
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn path_only_scratch_lock_step_proves_fresh_stale_and_missing() {
    let scratch = scratch_workspace();
    let root = &scratch.0;
    let lock_path = root.join("Cargo.lock");
    let before = fs::read(&lock_path).unwrap();

    let (passed, output) = run_lock_step(&run_command, root).unwrap().into_parts();
    assert_eq!(passed, Completion::Passed);
    assert_eq!(output, None);
    assert_eq!(fs::read(&lock_path).unwrap(), before);

    fs::write(
        root.join("crates/b/Cargo.toml"),
        "[package]\nname = \"b\"\nversion = \"0.2.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let discovery = discovery_locked_no_deps(root);
    assert!(
        discovery.status.success(),
        "discovery must still succeed on a stale lock: {}",
        String::from_utf8_lossy(&discovery.stderr)
    );
    let (stale, stale_out) = run_lock_step(&run_command, root).unwrap().into_parts();
    let Completion::Failed(findings) = stale else {
        panic!("expected stale lock failure");
    };
    assert_eq!(
        findings.to_string(),
        "BXW0097 Cargo.lock package= candidates=[command=\"cargo metadata --format-version 1 --locked\"]"
    );
    let captured = String::from_utf8(stale_out.unwrap()).unwrap();
    assert!(!captured.is_empty());
    assert_eq!(fs::read(&lock_path).unwrap(), before);

    fs::remove_file(&lock_path).unwrap();
    let (missing, missing_out) = run_lock_step(&run_command, root).unwrap().into_parts();
    let Completion::Failed(findings) = missing else {
        panic!("expected missing lock failure");
    };
    assert_eq!(
        findings.to_string(),
        "BXW0097 Cargo.lock package= candidates=[command=\"cargo metadata --format-version 1 --locked\"]"
    );
    assert!(!String::from_utf8(missing_out.unwrap()).unwrap().is_empty());
    assert!(!lock_path.exists());
}

#[test]
fn quality_specs_follow_package_id_and_declaration_order_with_exact_argv_split() {
    let workspace = quality_workspace();
    let specs = quality_specs(&workspace);
    let rendered: Vec<_> = specs
        .iter()
        .map(|command| {
            (
                command.package().as_str(),
                command.manifest_path().as_str(),
                command.spec().render(),
                command.spec().args().to_vec(),
            )
        })
        .collect();
    assert_eq!(
        rendered,
        [
            (
                "ping",
                "ping/boxology.toml",
                "ping-cmd --flag value".to_owned(),
                vec!["--flag".to_owned(), "value".to_owned()],
            ),
            (
                "ping",
                "ping/boxology.toml",
                "ping-cmd2 arg".to_owned(),
                vec!["arg".to_owned()],
            ),
            (
                "platform",
                "boxology.toml",
                "tool-a one two".to_owned(),
                vec!["one".to_owned(), "two".to_owned()],
            ),
            (
                "platform",
                "boxology.toml",
                "tool-b three".to_owned(),
                vec!["three".to_owned()],
            ),
        ]
    );
}

#[test]
fn quality_all_success_passes_without_output_in_exact_order() {
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    SEEN.lock().unwrap().clear();
    let workspace = quality_workspace();
    let runner: &CommandRunner = &|_root, spec| {
        SEEN.lock().unwrap().push(spec.render());
        Ok(CapturedOutput::new(true, b"ok-stdout", b"ok-stderr"))
    };
    let (completion, output) = run_quality_step(runner, Path::new("."), &workspace)
        .unwrap()
        .into_parts();
    assert_eq!(completion, Completion::Passed);
    assert_eq!(output, None);
    assert_eq!(
        SEEN.lock().unwrap().as_slice(),
        [
            "ping-cmd --flag value",
            "ping-cmd2 arg",
            "tool-a one two",
            "tool-b three",
        ]
    );
}

#[test]
fn quality_middle_failure_continues_with_bxw0107_and_header_output() {
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    SEEN.lock().unwrap().clear();
    let workspace = quality_workspace();
    let runner: &CommandRunner = &|_root, spec| {
        SEEN.lock().unwrap().push(spec.render());
        if spec.render() == "ping-cmd2 arg" {
            Ok(CapturedOutput::new(
                false,
                b"failed-stdout\n",
                b"failed-stderr\n",
            ))
        } else {
            Ok(CapturedOutput::new(true, b"", b""))
        }
    };
    let (completion, output) = run_quality_step(runner, Path::new("."), &workspace)
        .unwrap()
        .into_parts();
    let Completion::Failed(findings) = completion else {
        panic!("expected quality failure");
    };
    assert_eq!(
        findings.to_string(),
        "BXW0107 ping/boxology.toml package=ping candidates=[command=\"ping-cmd2 arg\"]"
    );
    assert_eq!(
        String::from_utf8(output.unwrap()).unwrap(),
        "command=\"ping-cmd2 arg\"\nfailed-stdout\nfailed-stderr\n"
    );
    assert_eq!(
        SEEN.lock().unwrap().as_slice(),
        [
            "ping-cmd --flag value",
            "ping-cmd2 arg",
            "tool-a one two",
            "tool-b three",
        ]
    );
}

#[test]
fn quality_failures_in_two_packages_sort_findings_keep_execution_output_order() {
    let workspace = quality_workspace();
    let runner: &CommandRunner = &|_root, spec| {
        if spec.render() == "ping-cmd --flag value" {
            // First failure deliberately omits a terminal LF so the next header needs framing.
            Ok(CapturedOutput::new(
                false,
                b"out:ping-cmd --flag value",
                b"err:ping-cmd --flag value",
            ))
        } else if spec.render() == "tool-b three" {
            Ok(CapturedOutput::new(
                false,
                b"out:tool-b three\n",
                b"err:tool-b three\n",
            ))
        } else {
            Ok(CapturedOutput::new(true, b"", b""))
        }
    };
    let (completion, output) = run_quality_step(runner, Path::new("."), &workspace)
        .unwrap()
        .into_parts();
    let Completion::Failed(findings) = completion else {
        panic!("expected quality failure");
    };
    assert_eq!(
        findings.to_string(),
        "BXW0107 ping/boxology.toml package=ping candidates=[command=\"ping-cmd --flag value\"]\n\
BXW0107 boxology.toml package=platform candidates=[command=\"tool-b three\"]"
    );
    assert_eq!(
        String::from_utf8(output.unwrap()).unwrap(),
        "command=\"ping-cmd --flag value\"\nout:ping-cmd --flag valueerr:ping-cmd --flag value\
\ncommand=\"tool-b three\"\nout:tool-b three\nerr:tool-b three\n"
    );
}

#[test]
fn quality_spawn_failure_returns_spawn_error() {
    let workspace = quality_workspace();
    let runner: &CommandRunner = &|_root, _spec| Err(SpawnError);
    assert_eq!(
        run_quality_step(runner, Path::new("."), &workspace)
            .unwrap_err()
            .to_string(),
        "BXW0096 Cargo.toml: a trusted check command could not be executed"
    );
}

#[test]
fn quality_no_declarations_passes_with_zero_invocations() {
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    CALLS.store(0, Ordering::Relaxed);
    let workspace = fixture_workspace();
    let runner: &CommandRunner = &|_root, _spec| {
        CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(CapturedOutput::new(true, b"", b""))
    };
    let (completion, output) = run_quality_step(runner, Path::new("."), &workspace)
        .unwrap()
        .into_parts();
    assert_eq!(completion, Completion::Passed);
    assert_eq!(output, None);
    assert_eq!(CALLS.load(Ordering::Relaxed), 0);
}
