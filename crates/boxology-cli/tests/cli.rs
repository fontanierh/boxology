#![cfg(unix)]

use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::{ffi::OsStringExt, fs::PermissionsExt},
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

const CONTRACT: &[u8] = br#"boxology::contract! {
    #[error]
    pub enum HelloError { EmptyName }
    #[capability(exposure = external)]
    pub async fn ping(nonce: u64) -> Result<u64, HelloError>;
}"#;
const ROOT_MANIFEST: &str = "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n";
const PACKAGE_MANIFEST: &str = "schema = 1\nid = \"ping\"\nkind = \"box\"\nowned = [\"boxology.toml\", \"implementation/**\"]\n\n[[crates]]\ncargo_package = \"ping-implementation\"\npath = \"implementation\"\nrole = \"box-implementation\"\n\n[[crates]]\ncargo_package = \"ping-contract\"\npath = \"generated/contract\"\nrole = \"box-contract\"\n\n[[derived]]\nid = \"contract\"\ngenerator = \"boxology-contract\"\ninputs = [\"boxology.toml\", \"implementation/src/**\"]\noutputs = [\"generated/**\"]\n";
const METADATA: &str = r#"{"workspace_root":"/w","workspace_members":["path+file:///w/ping/generated/contract#0.0.0","path+file:///w/ping/implementation#0.0.0"],"packages":[{"id":"path+file:///w/ping/generated/contract#0.0.0","name":"ping-contract","manifest_path":"/w/ping/generated/contract/Cargo.toml","dependencies":[]},{"id":"path+file:///w/ping/implementation#0.0.0","name":"ping-implementation","manifest_path":"/w/ping/implementation/Cargo.toml","dependencies":[]}] }"#;
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    cleanup: PathBuf,
    cargo: PathBuf,
    metadata: PathBuf,
    log: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.cleanup);
    }
}

impl Fixture {
    fn new(unowned: bool) -> Self {
        let cleanup = env::temp_dir().join(format!(
            "boxology-cli-generate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let root = cleanup.join("workspace");
        fs::create_dir_all(root.join("ping/implementation/src")).unwrap();
        fs::create_dir_all(root.join("ping/generated/contract")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"ping/implementation\", \"ping/generated/contract\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        fs::write(root.join("Cargo.lock"), b"").unwrap();
        fs::write(root.join("boxology.toml"), ROOT_MANIFEST).unwrap();
        fs::write(root.join("ping/boxology.toml"), PACKAGE_MANIFEST).unwrap();
        fs::write(
            root.join("ping/implementation/Cargo.toml"),
            "[package]\nname = \"ping-implementation\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(root.join("ping/implementation/src/lib.rs"), CONTRACT).unwrap();
        fs::write(
            root.join("ping/generated/contract/Cargo.toml"),
            "[package]\nname = \"ping-contract\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        if unowned {
            fs::write(root.join("stray.txt"), b"not owned").unwrap();
        }
        let cargo_dir = cleanup.join("cargo-bin");
        fs::create_dir(&cargo_dir).unwrap();
        let cargo = cargo_dir.join("cargo");
        fs::write(
            &cargo,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$BOXOLOGY_ARG_LOG\"\nif [ \"${BOXOLOGY_MODE:-ok}\" = fail ]; then printf '%s\\n' 'synthetic cargo metadata stderr' >&2; exit 17; fi\nif [ \"${BOXOLOGY_MODE:-ok}\" = nonutf8 ]; then printf '\\377'; exit 0; fi\ncat \"$BOXOLOGY_METADATA\"\n",
        )
        .unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        let metadata = cleanup.join("metadata.json");
        fs::write(&metadata, METADATA).unwrap();
        let log = cleanup.join("argv.log");
        Self {
            root,
            cleanup,
            cargo,
            metadata,
            log,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, "ok", true)
    }

    fn run_with(&self, args: &[&str], mode: &str, fake_cargo: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_boxology"));
        command.args(args).current_dir(&self.root);
        command.env("BOXOLOGY_ARG_LOG", &self.log);
        command.env("BOXOLOGY_METADATA", &self.metadata);
        command.env("BOXOLOGY_MODE", mode);
        if fake_cargo {
            let old_path = env::var_os("PATH").unwrap_or_default();
            let path = format!(
                "{}:{}",
                self.cargo.parent().unwrap().display(),
                old_path.display()
            );
            command.env("PATH", path);
        }
        command.output().unwrap()
    }
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

#[test]
fn parsing_accepts_only_the_two_generate_forms() {
    for args in [
        vec![],
        vec!["check"],
        vec!["generate", "--package"],
        vec!["generate", "--package", "ping", "extra"],
        vec!["generate", "--package=ping"],
        vec!["generate", "other"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_boxology"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
        assert!(text(&output.stdout).is_empty());
        assert_eq!(
            text(&output.stderr),
            "usage: boxology generate\n       boxology generate --package <id>\n"
        );
    }
}

#[test]
fn non_unicode_argument_is_usage_failure_without_panic() {
    let output = Command::new(env!("CARGO_BIN_EXE_boxology"))
        .arg(OsString::from_vec(vec![0xff]))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stdout).is_empty());
    assert_eq!(
        text(&output.stderr),
        "usage: boxology generate\n       boxology generate --package <id>\n"
    );
}

#[test]
fn first_write_then_byte_identical_unchanged_run_uses_exact_argv() {
    let fixture = Fixture::new(false);
    let first = fixture.run(&["generate"]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "stdout={} stderr={}",
        text(&first.stdout),
        text(&first.stderr)
    );
    assert!(text(&first.stderr).is_empty());
    assert!(text(&first.stdout).contains("generate ping written\n"));
    assert!(text(&first.stdout).ends_with("generate result changed\n"));
    let paths = [
        "generated/adapter/adapter.rs",
        "generated/contract/Cargo.toml",
        "generated/contract/src/lib.rs",
        "generated/schema.json",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|path| fs::read(fixture.root.join("ping").join(path)).unwrap())
        .collect();
    let second = fixture.run(&["generate"]);
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(
        text(&second.stdout),
        "generate ping unchanged\ngenerate result unchanged\n"
    );
    assert!(text(&second.stderr).is_empty());
    let after: Vec<_> = paths
        .iter()
        .map(|path| fs::read(fixture.root.join("ping").join(path)).unwrap())
        .collect();
    assert_eq!(before, after);
    assert_eq!(
        fs::read_to_string(&fixture.log).unwrap(),
        "metadata\n--format-version\n1\n--locked\n--no-deps\n"
    );
}

#[test]
fn unknown_package_is_invocation_failure() {
    let fixture = Fixture::new(false);
    let output = fixture.run(&["generate", "--package", "absent"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout={} stderr={}",
        text(&output.stdout),
        text(&output.stderr)
    );
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).contains("BXW0065"));
    assert!(
        text(&output.stderr)
            .contains("the requested package must be a discovered workspace package")
    );
}

#[test]
fn unowned_tracked_file_is_a_workspace_failure() {
    let fixture = Fixture::new(true);
    let output = fixture.run(&["generate"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).contains("BXW0044"));
}

#[test]
fn metadata_failure_reports_code_and_captured_stderr() {
    let fixture = Fixture::new(false);
    let output = fixture.run_with(&["generate"], "fail", true);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).starts_with("BXW0075 Cargo.toml: "));
    assert!(text(&output.stderr).contains("synthetic cargo metadata stderr"));
}

#[test]
fn non_utf8_metadata_is_a_coded_failure() {
    let fixture = Fixture::new(false);
    let output = fixture.run_with(&["generate"], "nonutf8", true);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).starts_with("BXW0075 Cargo.toml: "));
}

#[test]
fn corrupted_cargo_toml_is_reported_with_captured_cargo_output() {
    let fixture = Fixture::new(false);
    fs::write(fixture.root.join("Cargo.toml"), "[workspace\n").unwrap();
    let output = fixture.run_with(&["generate"], "ok", false);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).contains("BXW0075 Cargo.toml: "));
    assert!(text(&output.stderr).contains("Cargo.toml"));
}
