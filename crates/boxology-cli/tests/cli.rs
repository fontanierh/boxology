#![cfg(unix)]

use boxology_schema::SchemaDocument;
use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::{ffi::OsStringExt, fs::PermissionsExt},
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

const CONTRACT: &[u8] = br#"boxology::contract! {
    #[error]
    pub enum HelloError { EmptyName }
    #[capability(exposure = external)]
    pub async fn ping(nonce: u64) -> Result<u64, HelloError>;
}"#;
const CONTRACT_WITH_GREET: &[u8] = br#"boxology::contract! {
    #[error]
    pub enum HelloError { EmptyName }
    #[capability(exposure = external)]
    pub async fn ping(nonce: u64) -> Result<u64, HelloError>;
    #[capability(exposure = external)]
    pub async fn greet(name: String) -> Result<String, HelloError>;
}"#;
const USAGE: &str = "usage: boxology generate\n       boxology generate --package <id>\n       boxology check\n       boxology check --base <revision>\n";
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
    git_log: PathBuf,
    base_blob: PathBuf,
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
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$BOXOLOGY_ARG_LOG\"\nif [ \"$1\" = \"metadata\" ]; then\n  if [ \"${BOXOLOGY_MODE:-ok}\" = fail ]; then printf '%s\\n' 'synthetic cargo metadata stderr' >&2; exit 17; fi\n  if [ \"${BOXOLOGY_MODE:-ok}\" = nonutf8 ]; then printf '\\377'; exit 0; fi\n  if [ \"${BOXOLOGY_MODE:-ok}\" = fail-lock ]; then\n    for arg in \"$@\"; do\n      if [ \"$arg\" = \"--no-deps\" ]; then\n        /bin/cat \"$BOXOLOGY_METADATA\"\n        exit 0\n      fi\n    done\n    printf '%s\\n' 'representative lock failure'\n    exit 17\n  fi\n  /bin/cat \"$BOXOLOGY_METADATA\"\n  exit 0\nfi\nif [ \"${BOXOLOGY_FAIL:-}\" = \"$1\" ]; then\n  printf '%s\\n' \"representative $1 failure\"\n  exit 17\nfi\nexit 0\n",
        )
        .unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        let metadata = cleanup.join("metadata.json");
        fs::write(&metadata, METADATA).unwrap();
        let log = cleanup.join("argv.log");
        let git_log = cleanup.join("git-argv.log");
        let base_blob = cleanup.join("base-schema.json");
        Self {
            root,
            cleanup,
            cargo,
            metadata,
            log,
            git_log,
            base_blob,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_with(args, "ok", true)
    }

    fn run_with(&self, args: &[&str], mode: &str, fake_cargo: bool) -> Output {
        self.run_tools(args, mode, fake_cargo, None)
    }

    fn run_tools(&self, args: &[&str], mode: &str, fake_cargo: bool, fail: Option<&str>) -> Output {
        let _lock = env_lock();
        let _ = fs::remove_file(&self.log);
        let mut command = Command::new(env!("CARGO_BIN_EXE_boxology"));
        command.args(args).current_dir(&self.root);
        command.env("BOXOLOGY_ARG_LOG", &self.log);
        command.env("BOXOLOGY_METADATA", &self.metadata);
        command.env("BOXOLOGY_MODE", mode);
        command.env("BOXOLOGY_GIT_ARG_LOG", &self.git_log);
        command.env("BOXOLOGY_BASE_BLOB", &self.base_blob);
        command.env("GIT_CEILING_DIRECTORIES", &self.root);
        if let Some(step) = fail {
            command.env("BOXOLOGY_FAIL", step);
        } else {
            command.env_remove("BOXOLOGY_FAIL");
        }
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

    fn argv_log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn run_without_git(&self, args: &[&str]) -> Output {
        let _lock = env_lock();
        Command::new(env!("CARGO_BIN_EXE_boxology"))
            .args(args)
            .current_dir(&self.root)
            .env("BOXOLOGY_ARG_LOG", &self.log)
            .env("BOXOLOGY_METADATA", &self.metadata)
            .env("BOXOLOGY_MODE", "ok")
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env("PATH", self.cargo.parent().unwrap())
            .output()
            .unwrap()
    }

    fn compose(&self, boxes: &[&str], bindings: &[(&str, &str)]) {
        fs::create_dir_all(self.root.join("app")).unwrap();
        let boxes = boxes
            .iter()
            .map(|id| format!("{id:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut manifest = format!(
            "schema = 1\nid = \"app\"\nkind = \"composition\"\nowned = [\"boxology.toml\"]\n\n[composition]\nboxes = [{boxes}]\n"
        );
        for (selector, exposure) in bindings {
            manifest.push_str(&format!(
                "\n[[composition.bindings]]\nbox = \"ping\"\ncapability = {selector:?}\ntransport = \"http\"\nexposure = {exposure:?}\n"
            ));
        }
        fs::write(self.root.join("app/boxology.toml"), manifest).unwrap();
    }

    fn git(&self, args: &[&str]) -> Output {
        let _lock = env_lock();
        Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }

    fn commit(&self, message: &str) {
        if !self.root.join(".git").exists() {
            assert!(self.git(&["init", "-q", "-b", "main"]).status.success());
            assert!(
                self.git(&["config", "user.name", "Boxology Test"])
                    .status
                    .success()
            );
            assert!(
                self.git(&["config", "user.email", "boxology@example.invalid"])
                    .status
                    .success()
            );
        }
        assert!(self.git(&["add", "."]).status.success());
        let output = self.git(&["commit", "-q", "-m", message]);
        assert!(output.status.success(), "{}", text(&output.stderr));
    }

    fn init_repository(&self) {
        assert!(self.git(&["init", "-q", "-b", "main"]).status.success());
        assert!(
            self.git(&["config", "user.name", "Boxology Test"])
                .status
                .success()
        );
        assert!(
            self.git(&["config", "user.email", "boxology@example.invalid"])
                .status
                .success()
        );
    }

    fn install_fake_git(&self, exists_status: u8) {
        let git = self.cargo.parent().unwrap().join("git");
        fs::write(
            &git,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$BOXOLOGY_GIT_ARG_LOG\"\nprintf '%s\\n' -- >> \"$BOXOLOGY_GIT_ARG_LOG\"\ncase \"$1 $2\" in\n  'rev-parse --verify') printf '%040d\\n' 0;;\n  'ls-tree --name-only') printf '%s\\0' \"$6\";;\n  'cat-file -e') exit {exists_status};;\n  'cat-file blob') /bin/cat \"$BOXOLOGY_BASE_BLOB\";;\n  *) exit 19;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn install_fake_git_default(&self, merge_base: &str, exists_status: u8) {
        let git = self.cargo.parent().unwrap().join("git");
        fs::write(
            &git,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$BOXOLOGY_GIT_ARG_LOG\"\nprintf '%s\\n' -- >> \"$BOXOLOGY_GIT_ARG_LOG\"\ncase \"$1 $2\" in\n  'rev-parse --git-dir') printf '%s\\n' .git;;\n  'merge-base HEAD') printf '%s\\n' '{merge_base}';;\n  'rev-parse --verify')\n    case \"$4\" in\n      '{merge_base}^{{commit}}') printf '%s\\n' '{merge_base}';;\n      *) exit 1;;\n    esac;;\n  'ls-tree --name-only') printf '%s\\0' \"$6\";;\n  'cat-file -e') exit {exists_status};;\n  'cat-file blob') /bin/cat \"$BOXOLOGY_BASE_BLOB\";;\n  *) exit 19;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn schema(box_id: &str, capabilities: &[(&str, &str)]) -> Vec<u8> {
    let capabilities = capabilities.iter().map(|(name, exposure)| format!(r#"{{"deprecation":null,"docs":[],"error":"Fault","id":"{box_id}.{name}","idempotency":"none","input":{{"name":"value","type":"String"}},"max_exposure":"{exposure}","name":"{name}","output":{{"type":"String"}},"shape":"unary"}}"#)).collect::<Vec<_>>().join(",");
    format!(r#"{{"box_id":"{box_id}","capabilities":[{capabilities}],"provenance":null,"revision":"sha256:0000000000000000000000000000000000000000000000000000000000000000","schema_format":1,"types":[{{"deprecation":null,"docs":[],"kind":"error","name":"Fault","variants":[{{"deprecation":null,"docs":[],"name":"Failed","payload":"unit"}}]}}]}}"#).into_bytes()
}

#[test]
fn parsing_accepts_only_the_two_generate_forms() {
    for args in [
        vec![],
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
        assert_eq!(text(&output.stderr), USAGE);
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
    assert_eq!(text(&output.stderr), USAGE);
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
fn generate_package_ping_attaches_exact_additive_classification() {
    let fixture = Fixture::new(false);
    let output = fixture.run(&["generate", "--package", "ping"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        text(&output.stdout),
        text(&output.stderr)
    );
    assert!(text(&output.stderr).is_empty());
    assert_eq!(
        text(&output.stdout),
        "\
generate ping written
  written generated/adapter/adapter.rs
  written generated/contract/Cargo.toml
  written generated/contract/src/lib.rs
  written generated/schema.json
classification additive
finding BXC0026 path=\"ping\" additive kind=\"contract introduced\" base=- submitted=\"ping\"
generate result changed
"
    );
}

#[test]
fn generate_incompatible_classification_still_exits_zero() {
    let fixture = Fixture::new(false);
    fs::write(
        fixture.root.join("ping/implementation/src/lib.rs"),
        CONTRACT_WITH_GREET,
    )
    .unwrap();
    assert_eq!(
        fixture
            .run(&["generate", "--package", "ping"])
            .status
            .code(),
        Some(0)
    );
    fs::write(
        fixture.root.join("ping/implementation/src/lib.rs"),
        CONTRACT,
    )
    .unwrap();
    let output = fixture.run(&["generate", "--package", "ping"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={} stderr={}",
        text(&output.stdout),
        text(&output.stderr)
    );
    assert!(text(&output.stderr).is_empty());
    assert_eq!(
        text(&output.stdout),
        "\
generate ping written
  written generated/adapter/adapter.rs
  written generated/contract/src/lib.rs
  written generated/schema.json
classification incompatible
finding BXC0040 path=\"ping.greet\" incompatible kind=\"capability removed\" base=\"greet\" submitted=-
generate result changed
"
    );
}

#[test]
fn generate_unparseable_base_is_bxw0077_without_result_line() {
    let fixture = Fixture::new(false);
    assert_eq!(
        fixture
            .run(&["generate", "--package", "ping"])
            .status
            .code(),
        Some(0)
    );
    fs::write(fixture.root.join("ping/generated/schema.json"), b"{").unwrap();
    let output = fixture.run(&["generate", "--package", "ping"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={} stderr={}",
        text(&output.stdout),
        text(&output.stderr)
    );
    assert!(text(&output.stderr).contains("BXW0077 base:"));
    assert_eq!(
        text(&output.stdout),
        "\
generate ping written
  written generated/schema.json
"
    );
}

#[test]
fn generate_updates_provenance_digest_and_revision() {
    let fixture = Fixture::new(false);
    assert_eq!(
        fixture
            .run(&["generate", "--package", "ping"])
            .status
            .code(),
        Some(0)
    );
    let first =
        SchemaDocument::parse(&fs::read(fixture.root.join("ping/generated/schema.json")).unwrap())
            .unwrap();
    let provenance = first.provenance.value();
    assert_eq!(
        provenance.get("generator").and_then(|value| value.as_str()),
        Some("boxology-generator")
    );
    assert_eq!(
        provenance
            .get("generator_version")
            .and_then(|value| value.as_str()),
        Some("0.0.0")
    );
    assert_eq!(
        provenance
            .get("semantic_digest")
            .and_then(|value| value.as_str()),
        Some("sha256:1ae6fa257308b7e30cb09c8005da2f406171abb36a411538c77b464223ae73e3")
    );
    fs::write(
        fixture.root.join("ping/implementation/src/lib.rs"),
        CONTRACT_WITH_GREET,
    )
    .unwrap();
    assert_eq!(
        fixture
            .run(&["generate", "--package", "ping"])
            .status
            .code(),
        Some(0)
    );
    let second =
        SchemaDocument::parse(&fs::read(fixture.root.join("ping/generated/schema.json")).unwrap())
            .unwrap();
    assert_eq!(
        second
            .provenance
            .value()
            .get("semantic_digest")
            .and_then(|value| value.as_str()),
        Some("sha256:d9e2a6005f4771822df843fa864ba980f4a42d84f533da9fe97184ac7c3415d5")
    );
    assert_ne!(first.revision, second.revision);
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

#[test]
fn check_clean_workspace_reports_all_steps_and_exits_zero() {
    let fixture = Fixture::new(false);
    let generated = fixture.run(&["generate"]);
    assert_eq!(generated.status.code(), Some(0));
    let first = fixture.run(&["check"]);
    let expected = "check discovery passed\n\
                    check regeneration passed\n\
                    check contract-classification skipped\n\
                    \x20 contract classification skipped: no repository is available\n\
                    check diff-ownership skipped\n\
                    \x20 not run: the step is not implemented in this boxology version\n\
                    check cargo-graph passed\n\
                    check fmt passed\n\
                    check clippy passed\n\
                    check tests passed\n\
                    check quality skipped\n\
                    \x20 not run: the step is not implemented in this boxology version\n\
                    check result passed\n";
    assert_eq!(first.status.code(), Some(0));
    assert_eq!(text(&first.stdout), expected);
    assert!(text(&first.stderr).is_empty());
    let log = fixture.argv_log();
    assert_eq!(log.matches("metadata\n").count(), 2, "{log}");
    assert_eq!(log.matches("--no-deps\n").count(), 1, "{log}");
    assert!(
        log.contains("fmt\n--check\n-p\nping-implementation\n"),
        "{log}"
    );
    assert!(
        !log.contains("ping-contract"),
        "fmt selection must exclude derived crate: {log}"
    );
    assert!(
        log.contains("clippy\n--workspace\n--all-targets\n--all-features\n--\n-D\nwarnings\n"),
        "{log}"
    );
    assert!(log.contains("test\n--workspace\n--all-features\n"), "{log}");
    let second = fixture.run(&["check"]);
    assert_eq!(second.status.code(), Some(0));
    assert_eq!(second.stdout, first.stdout);
    assert_eq!(second.stderr, first.stderr);
}

#[test]
fn check_lock_failure_renders_finding_command_output_and_keeps_later_steps() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let output = fixture.run_with(&["check"], "fail-lock", true);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).is_empty());
    assert!(stdout.contains(
        "check cargo-graph failed\n\
         \x20 BXW0097 Cargo.lock package= candidates=[command=\"cargo metadata --format-version 1 --locked\"]\n\
         representative lock failure\n"
    ));
    assert!(stdout.contains("check fmt passed\n"));
    assert!(stdout.contains("check clippy passed\n"));
    assert!(stdout.contains("check tests passed\n"));
    assert!(stdout.contains(
        "check quality skipped\n  not run: the step is not implemented in this boxology version\n"
    ));
    assert!(stdout.ends_with("check result failed\n"));
    let log = fixture.argv_log();
    assert_eq!(log.matches("metadata\n").count(), 2, "{log}");
    assert_eq!(log.matches("--no-deps\n").count(), 1, "{log}");
    assert!(
        log.contains("fmt\n--check\n-p\nping-implementation\n"),
        "{log}"
    );
    assert!(log.contains("test\n--workspace\n--all-features\n"), "{log}");
}

#[test]
fn check_tool_failure_renders_finding_command_output_and_exit_one() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let output = fixture.run_tools(&["check"], "ok", true, Some("clippy"));
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).is_empty());
    assert!(stdout.contains(
        "check fmt passed\n\
         check clippy failed\n\
         \x20 BXW0094 Cargo.toml package= candidates=[command=\"cargo clippy --workspace --all-targets --all-features -- -D warnings\"]\n\
         representative clippy failure\n"
    ));
    assert!(stdout.contains("check tests passed\n"));
    assert!(stdout.contains(
        "check quality skipped\n  not run: the step is not implemented in this boxology version\n"
    ));
    assert!(stdout.ends_with("check result failed\n"));
}

#[test]
fn check_test_failure_renders_finding_command_output_and_exit_one() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let output = fixture.run_tools(&["check"], "ok", true, Some("test"));
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).is_empty());
    assert!(stdout.contains(
        "check fmt passed\n\
         check clippy passed\n\
         check tests failed\n\
         \x20 BXW0095 Cargo.toml package= candidates=[command=\"cargo test --workspace --all-features\"]\n\
         representative test failure\n"
    ));
    assert!(stdout.contains(
        "check quality skipped\n  not run: the step is not implemented in this boxology version\n"
    ));
    assert!(stdout.ends_with("check result failed\n"));
}

#[test]
fn check_validates_exact_and_wildcard_composition_with_the_planned_schema() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.compose(
        &["ping"],
        &[("ping.ping", "external"), ("ping.*", "external")],
    );
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    assert!(
        text(&output.stdout).starts_with("check discovery passed\ncheck regeneration passed\n")
    );
}

#[test]
fn check_reports_schema_selector_and_exposure_findings_in_the_eight_step_report() {
    let missing = Fixture::new(false);
    assert_eq!(missing.run(&["generate"]).status.code(), Some(0));
    missing.compose(&["ping"], &[("ping.*", "external")]);
    fs::remove_file(missing.root.join("ping/generated/schema.json")).unwrap();
    let output = missing.run(&["check"]);
    let report = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).is_empty());
    let missing_schema = "BXW0088 app/boxology.toml package=app candidates=[box=ping schema=ping/generated/schema.json kind=missing]";
    let regeneration = "BXW0083 ping/generated/schema.json package=ping candidates=[kind=missing repair=\"boxology generate --package ping\"]";
    assert!(report.find(missing_schema).unwrap() < report.find(regeneration).unwrap());
    assert!(report.ends_with("check result failed\n"));

    for (bytes, kind) in [
        (b"bad".to_vec(), "unreadable"),
        (schema("other", &[("alpha", "external")]), "mismatched"),
    ] {
        let fixture = Fixture::new(false);
        assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
        fixture.compose(&["ping"], &[("ping.*", "external")]);
        fs::write(fixture.root.join("ping/generated/schema.json"), bytes).unwrap();
        let first = fixture.run(&["check"]);
        let second = fixture.run(&["check"]);
        assert_eq!(first.status.code(), Some(1));
        assert_eq!(first.stdout, second.stdout);
        assert_eq!(first.stderr, second.stderr);
        assert!(text(&first.stderr).is_empty());
        assert!(text(&first.stdout).contains(&format!(
            "BXW0088 app/boxology.toml package=app candidates=[box=ping schema=ping/generated/schema.json kind={kind}]"
        )));
        assert!(text(&first.stdout).contains(
            "BXW0083 ping/generated/schema.json package=ping candidates=[kind=differing repair=\"boxology generate --package ping\"]"
        ));
    }

    let selector = Fixture::new(false);
    assert_eq!(selector.run(&["generate"]).status.code(), Some(0));
    selector.compose(&["ping"], &[("ping.missing", "external")]);
    let output = selector.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).contains(
        "BXW0089 app/boxology.toml package=app candidates=[selector=ping.missing transport=http]"
    ));

    let exposure = Fixture::new(false);
    assert_eq!(exposure.run(&["generate"]).status.code(), Some(0));
    exposure.compose(&["ping"], &[("ping.*", "external")]);
    fs::write(
        exposure.root.join("ping/generated/schema.json"),
        schema("ping", &[("zulu", "code_only"), ("alpha", "internal")]),
    )
    .unwrap();
    let output = exposure.run(&["check"]);
    let report = text(&output.stdout);
    let alpha = "capability=ping.alpha exposure=external max=internal";
    let zulu = "capability=ping.zulu exposure=external max=code_only";
    assert_eq!(output.status.code(), Some(1));
    assert!(report.find(alpha).unwrap() < report.find(zulu).unwrap());
}

#[test]
fn check_reports_an_absent_selected_box_as_bxw0087() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.compose(&["absent"], &[]);
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).is_empty());
    assert!(
        text(&output.stdout)
            .contains("BXW0087 app/boxology.toml package=app candidates=[box=absent]")
    );
}

#[test]
fn check_tampered_and_missing_artifacts_fail_naming_repair() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let contract = fixture.root.join("ping/generated/contract/src/lib.rs");
    let mut bytes = fs::read(&contract).unwrap();
    bytes[0] ^= 1;
    fs::write(contract, bytes).unwrap();
    fs::remove_file(fixture.root.join("ping/generated/adapter/adapter.rs")).unwrap();
    let output = fixture.run(&["check"]);
    let stdout = text(&output.stdout);
    let missing = [
        "BXW0083 ping/generated/adapter/adapter.rs package=ping ",
        "candidates=[kind=missing repair=\"boxology generate --package ping\"]",
    ]
    .concat();
    let differing = [
        "BXW0083 ping/generated/contract/src/lib.rs package=ping ",
        "candidates=[kind=differing repair=\"boxology generate --package ping\"]",
    ]
    .concat();
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).is_empty());
    assert!(stdout.contains("check regeneration failed"));
    assert!(stdout.contains(&missing));
    assert!(stdout.contains(&differing));
    assert!(stdout.find(&missing).unwrap() < stdout.find(&differing).unwrap());
    assert!(stdout.ends_with("check result failed\n"));
}

#[test]
fn check_deleted_generation_input_is_a_step_error() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fs::remove_file(fixture.root.join("ping/implementation/src/lib.rs")).unwrap();
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).contains("BXW0070"));
}

#[test]
fn check_guard_rejection_is_fatal_without_a_misleading_report() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.compose(&["ping"], &[("ping.*", "external")]);
    let schema = fixture.root.join("ping/generated/schema.json");
    let target = fixture.root.join("ping/generated/real-schema.json");
    fs::rename(&schema, &target).unwrap();
    symlink("real-schema.json", &schema).unwrap();

    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).starts_with("BXW0076 "));
    assert!(!text(&output.stderr).contains("check discovery"));
}

#[test]
fn check_workspace_findings_fail_before_composition() {
    let fixture = Fixture::new(true);
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).contains("BXW0044"));
}

#[test]
fn check_metadata_failure_is_invocation() {
    let fixture = Fixture::new(false);
    let output = fixture.run_with(&["check"], "fail", true);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).starts_with("BXW0075 Cargo.toml: "));
}

#[test]
fn check_rejects_unwired_flags_with_usage() {
    for args in [
        vec!["check", "--format", "json"],
        vec!["check", "--base"],
        vec!["check", "--base", "HEAD", "extra"],
        vec!["check", "--base=HEAD"],
        vec!["check", "--base", "--help"],
        vec!["check", "--base", ""],
        vec!["check", "extra"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_boxology"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
        assert!(text(&output.stdout).is_empty());
        assert_eq!(text(&output.stderr), USAGE);
    }
}

#[test]
fn check_base_reports_real_addition_and_unchanged_baseline_without_failing() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.commit("baseline");

    let unchanged = fixture.run(&["check", "--base", "HEAD"]);
    assert_eq!(unchanged.status.code(), Some(0));
    assert!(text(&unchanged.stderr).is_empty());
    assert!(text(&unchanged.stdout).contains("check contract-classification passed\n"));

    fs::write(
        fixture.root.join("ping/implementation/src/lib.rs"),
        CONTRACT_WITH_GREET,
    )
    .unwrap();
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let addition = fixture.run(&["check", "--base", "HEAD"]);
    assert_eq!(addition.status.code(), Some(0));
    assert!(text(&addition.stderr).is_empty());
    assert!(text(&addition.stdout).contains("check contract-classification failed\n"));
    assert!(text(&addition.stdout).contains("BXC0039 ping ping.greet additive\n"));
    assert!(text(&addition.stdout).ends_with("check result passed\n"));
}

#[test]
fn check_base_git_boundary_uses_the_exact_nonmutating_argv() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fs::copy(
        fixture.root.join("ping/generated/schema.json"),
        &fixture.base_blob,
    )
    .unwrap();
    fixture.install_fake_git(0);

    let output = fixture.run(&["check", "--base", "main"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    let oid = "0000000000000000000000000000000000000000";
    assert_eq!(
        fs::read_to_string(&fixture.git_log).unwrap(),
        format!(
            "rev-parse\n--verify\n--end-of-options\nmain^{{commit}}\n--\n\
             ls-tree\n--name-only\n-z\n{oid}\n--\nping/generated/schema.json\n--\n\
             cat-file\n-e\n{oid}:ping/generated/schema.json\n--\n\
             cat-file\nblob\n{oid}:ping/generated/schema.json\n--\n"
        )
    );
}

#[test]
fn check_base_cat_file_failure_after_confirmed_presence_is_always_bxw0092() {
    for status in [1, 17] {
        let fixture = Fixture::new(false);
        assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
        fixture.install_fake_git(status);

        let output = fixture.run(&["check", "--base", "main"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(text(&output.stdout).is_empty());
        assert_eq!(
            text(&output.stderr),
            "BXW0092 ping/generated/schema.json: a base-revision schema object must be readable as a Git blob\n"
        );
    }
}

#[test]
fn check_base_git_spawn_failure_is_invocation_exit_two() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));

    let output = fixture.run_without_git(&["check", "--base", "HEAD"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stdout).is_empty());
    assert_eq!(text(&output.stderr), "git could not be executed\n");
}

#[test]
fn check_base_absent_schema_is_a_valid_none_base() {
    let fixture = Fixture::new(false);
    fixture.commit("no generated schema");
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let output = fixture.run(&["check", "--base", "HEAD"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains("check contract-classification failed\n"));
    assert!(text(&output.stdout).ends_with("check result passed\n"));
}

#[test]
fn check_base_reports_malformed_schema_as_bxw0080() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let good = fs::read(fixture.root.join("ping/generated/schema.json")).unwrap();
    fs::write(fixture.root.join("ping/generated/schema.json"), b"bad").unwrap();
    fixture.commit("malformed base schema");
    fs::write(fixture.root.join("ping/generated/schema.json"), good).unwrap();

    let output = fixture.run(&["check", "--base", "HEAD"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).starts_with("BXW0080 ping base: "));
}

#[test]
fn check_base_rejects_invalid_and_noncommit_revisions_without_echoing_them() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.commit("baseline");
    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"]);
    let tree = text(&tree.stdout).trim().to_owned();
    for revision in ["does-not-exist", tree.as_str()] {
        let output = fixture.run(&["check", "--base", revision]);
        assert_eq!(output.status.code(), Some(1), "revision={revision}");
        assert!(text(&output.stdout).is_empty());
        assert_eq!(
            text(&output.stderr),
            "BXW0091 .git: the explicit base revision must resolve to a Git commit\n"
        );
        assert!(!text(&output.stderr).contains(revision));
    }
}

#[test]
fn check_base_reports_a_present_non_blob_schema_as_bxw0092() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let schema_path = fixture.root.join("ping/generated/schema.json");
    let valid = fs::read(&schema_path).unwrap();
    fs::remove_file(&schema_path).unwrap();
    fs::create_dir(&schema_path).unwrap();
    fs::write(schema_path.join("child"), b"tree at schema path").unwrap();
    fixture.commit("non-blob schema object");
    fs::remove_dir_all(&schema_path).unwrap();
    fs::write(&schema_path, valid).unwrap();

    let output = fixture.run(&["check", "--base", "HEAD"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).is_empty());
    assert_eq!(
        text(&output.stderr),
        "BXW0092 ping/generated/schema.json: a base-revision schema object must be readable as a Git blob\n"
    );
}

#[test]
fn check_default_base_classifies_against_merge_base_with_main() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.commit("baseline");
    assert!(
        fixture
            .git(&["checkout", "-q", "-b", "work"])
            .status
            .success()
    );

    let unchanged = fixture.run(&["check"]);
    assert_eq!(unchanged.status.code(), Some(0));
    assert!(text(&unchanged.stderr).is_empty());
    assert!(text(&unchanged.stdout).contains("check contract-classification passed\n"));
    assert!(text(&unchanged.stdout).ends_with("check result passed\n"));

    fs::write(
        fixture.root.join("ping/implementation/src/lib.rs"),
        CONTRACT_WITH_GREET,
    )
    .unwrap();
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let addition = fixture.run(&["check"]);
    assert_eq!(addition.status.code(), Some(0));
    assert!(text(&addition.stderr).is_empty());
    assert!(text(&addition.stdout).contains("check contract-classification failed\n"));
    assert!(text(&addition.stdout).contains("BXC0039 ping ping.greet additive\n"));
    assert!(text(&addition.stdout).ends_with("check result passed\n"));
}

#[test]
fn check_default_base_skips_when_main_is_missing_after_committed_trunk() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.commit("trunk");
    assert!(
        fixture
            .git(&["branch", "-m", "main", "trunk"])
            .status
            .success()
    );
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains(
        "check contract-classification skipped\n  contract classification skipped: no merge base with main is available\n"
    ));
    assert!(text(&output.stdout).ends_with("check result passed\n"));
}

#[test]
fn check_default_base_skips_when_main_is_unborn() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.init_repository();
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains(
        "check contract-classification skipped\n  contract classification skipped: no merge base with main is available\n"
    ));
    assert!(text(&output.stdout).ends_with("check result passed\n"));
}

#[test]
fn check_default_base_skips_when_histories_are_disjoint() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.commit("main lineage");
    assert!(
        fixture
            .git(&["checkout", "--orphan", "orphan"])
            .status
            .success()
    );
    assert!(fixture.git(&["add", "."]).status.success());
    let orphan = fixture.git(&["commit", "-q", "-m", "orphan lineage"]);
    assert!(orphan.status.success(), "{}", text(&orphan.stderr));
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains(
        "check contract-classification skipped\n  contract classification skipped: no merge base with main is available\n"
    ));
    assert!(text(&output.stdout).ends_with("check result passed\n"));
}

#[test]
fn check_default_base_skips_when_no_repository_is_available() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains(
        "check contract-classification skipped\n  contract classification skipped: no repository is available\n"
    ));
    assert!(text(&output.stdout).ends_with("check result passed\n"));
}

#[test]
fn check_default_base_git_spawn_failure_is_invocation_exit_two() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let output = fixture.run_without_git(&["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stdout).is_empty());
    assert_eq!(text(&output.stderr), "git could not be executed\n");
}

#[test]
fn check_default_base_git_boundary_uses_the_exact_nonmutating_argv() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fs::copy(
        fixture.root.join("ping/generated/schema.json"),
        &fixture.base_blob,
    )
    .unwrap();
    let oid = "0000000000000000000000000000000000000000";
    fixture.install_fake_git_default(oid, 0);

    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains("check contract-classification passed\n"));
    assert_eq!(
        fs::read_to_string(&fixture.git_log).unwrap(),
        format!(
            "rev-parse\n--git-dir\n--\n\
             merge-base\nHEAD\nmain\n--\n\
             ls-tree\n--name-only\n-z\n{oid}\n--\nping/generated/schema.json\n--\n\
             cat-file\n-e\n{oid}:ping/generated/schema.json\n--\n\
             cat-file\nblob\n{oid}:ping/generated/schema.json\n--\n"
        )
    );
}

#[test]
fn check_default_base_merge_base_garbage_is_bxw0091() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.install_fake_git_default("not-a-commit", 0);
    let output = fixture.run(&["check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).is_empty());
    assert_eq!(
        text(&output.stderr),
        "BXW0091 .git: the explicit base revision must resolve to a Git commit\n"
    );
    assert!(!text(&output.stderr).contains("not-a-commit"));
}

const OID40: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SECRET: &str = "BOXOLOGY_SECRET_GIT_ERR";
const LISTING: &str =
    "BXW0103 .git: the base revision's Git listings must parse as expected NUL-delimited output";
const BLOB: &str =
    "BXW0104 boxology.toml: a base-revision workspace object must be readable as a Git blob";
const TWIN: &str = "schema = 1\nid = \"twin\"\nkind = \"box\"\nowned = [\"boxology.toml\"]\n";
const PLATFORM_OWN: &str = "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\nfixtures = [\"corpus/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n";
const HDR: &str =
    "BXW0105 .git: the base revision's package declarations must form a discoverable workspace";
const GIT_SH: &str = "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$BOXOLOGY_GIT_ARG_LOG\"\nprintf '%s\\n' -- >> \"$BOXOLOGY_GIT_ARG_LOG\"\nprintf '%s\\n' \"$BOXOLOGY_GIT_SECRET\" >&2\nst(){ read s < \"$BOXOLOGY_GIT_DATA/$1\"; [ \"$s\" -eq 0 ]||exit \"$s\"; }\ncase \"$1 $2\" in\n 'ls-tree -r') st ts;cat \"$BOXOLOGY_GIT_DATA/t\";;\n 'diff --name-only') st ds;cat \"$BOXOLOGY_GIT_DATA/d\";;\n 'cat-file blob') st bs;cat \"$BOXOLOGY_GIT_DATA/b/$3\";;\n 'rev-parse --verify') printf '%s\\n' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa;;\n *) exit 19;;\nesac\n";

mod ingest {
    use super::*;
    use boxology_cli::{BaseError, BaseInputsError, ResolvedBase, base_diff_inputs};
    use boxology_manifest::RelativePath;

    fn oid(c: char) -> String {
        c.to_string().repeat(40)
    }
    fn tre(mode: &str, kind: &str, o: &str, p: &str) -> Vec<u8> {
        let mut v = format!("{mode} {kind} {o}\t{p}").into_bytes();
        v.push(0);
        v
    }
    fn data(e: BaseInputsError) -> BaseError {
        match e {
            BaseInputsError::Data(e) => e,
            o => panic!("{o}"),
        }
    }
    fn base() -> ResolvedBase {
        ResolvedBase::from_oid(OID40.into()).unwrap()
    }
    fn setup(f: &Fixture) {
        let d = f.cleanup.join("g");
        fs::create_dir_all(d.join("b")).unwrap();
        fs::write(f.cleanup.join("gp"), d.to_str().unwrap()).unwrap();
        let git = f.cargo.parent().unwrap().join("git");
        fs::write(&git, GIT_SH).unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fn seed(f: &Fixture, tree: &[u8], diff: &[u8], blobs: &[(&str, &[u8])], st: [i32; 3]) {
        let d = f.cleanup.join("g");
        let _ = fs::remove_dir_all(d.join("b"));
        fs::create_dir_all(d.join("b")).unwrap();
        fs::write(d.join("t"), tree).unwrap();
        fs::write(d.join("d"), diff).unwrap();
        for (n, c) in [("ts", st[0]), ("ds", st[1]), ("bs", st[2])] {
            fs::write(d.join(n), c.to_string()).unwrap();
        }
        for (id, body) in blobs {
            fs::write(d.join("b").join(id), body).unwrap();
        }
    }
    fn with_git<T>(f: &Fixture, body: impl FnOnce() -> T) -> T {
        let _g = env_lock();
        let data = fs::read_to_string(f.cleanup.join("gp")).unwrap();
        let path = format!(
            "{}:{}",
            f.cargo.parent().unwrap().display(),
            env::var("PATH").unwrap_or_default()
        );
        let old = env::var_os("PATH");
        unsafe {
            env::set_var("PATH", &path);
            env::set_var("BOXOLOGY_GIT_ARG_LOG", &f.git_log);
            env::set_var("BOXOLOGY_GIT_DATA", &data);
            env::set_var("BOXOLOGY_GIT_SECRET", SECRET);
        }
        let _ = fs::remove_file(&f.git_log);
        let out = body();
        unsafe {
            match old {
                Some(v) => env::set_var("PATH", v),
                None => env::remove_var("PATH"),
            }
            env::remove_var("BOXOLOGY_GIT_ARG_LOG");
            env::remove_var("BOXOLOGY_GIT_DATA");
            env::remove_var("BOXOLOGY_GIT_SECRET");
        }
        out
    }
    fn inputs(f: &Fixture) -> Result<boxology_cli::BaseDiffInputs, BaseInputsError> {
        with_git(f, || base_diff_inputs(&f.root, &base()))
    }

    #[test]
    fn git_ingestion_boundary_discovery_and_cargo() {
        assert_eq!(
            ResolvedBase::from_oid(format!("  {OID40}  "))
                .unwrap()
                .as_str(),
            OID40
        );
        assert!(ResolvedBase::from_oid("b".repeat(64)).is_ok());
        assert_eq!(
            ResolvedBase::from_oid("nope".into())
                .unwrap_err()
                .to_string(),
            "BXW0091 .git: the explicit base revision must resolve to a Git commit"
        );

        let f = Fixture::new(false);
        setup(&f);
        let (m, x, l, gl) = (oid('e'), oid('c'), oid('d'), oid('f'));
        let mut tree = tre("100644", "blob", &m, "boxology.toml");
        tree.extend(tre("100755", "blob", &x, "bin/run"));
        tree.extend(tre("120000", "blob", &l, "alias"));
        tree.extend(tre("160000", "commit", &gl, "vendor/lib"));
        // Duplicate validated paths are BXW0103, including gitlink collisions.
        let mut file_git = tre("100644", "blob", &m, "p");
        file_git.extend(tre("160000", "commit", &gl, "p"));
        seed(&f, &file_git, b"", &[], [0, 0, 0]);
        assert_eq!(data(inputs(&f).unwrap_err()).to_string(), LISTING);
        let mut git_git = tre("160000", "commit", &gl, "vendor/lib");
        git_git.extend(tre("160000", "commit", &x, "vendor/lib"));
        seed(&f, &git_git, b"", &[], [0, 0, 0]);
        assert_eq!(data(inputs(&f).unwrap_err()).to_string(), LISTING);

        seed(
            &f,
            &tree,
            b"alias\0bin/run\0",
            &[(&m, ROOT_MANIFEST.as_bytes()), (&x, b"x"), (&l, b"bin/run")],
            [0, 0, 0],
        );
        let got = inputs(&f).unwrap();
        assert_eq!(
            got.changed()
                .iter()
                .map(RelativePath::as_str)
                .collect::<Vec<_>>(),
            ["alias", "bin/run"]
        );
        assert_eq!(
            got.packages()
                .iter()
                .map(|p| p.id().as_str())
                .collect::<Vec<_>>(),
            ["platform"]
        );
        assert_eq!(
            fs::read_to_string(&f.git_log).unwrap(),
            format!(
                "ls-tree\n-r\n-z\n--full-tree\n{OID40}\n--\ncat-file\nblob\n{m}\n--\ncat-file\nblob\n{l}\n--\ndiff\n--name-only\n-z\n--no-renames\n--no-ext-diff\n{OID40}\n--\n--\n"
            )
        );

        let good = tre("100644", "blob", &m, "boxology.toml");
        let mut no_tab = format!("100644 blob {m} path").into_bytes();
        no_tab.push(0);
        let mut bad8 = format!("100644 blob {m}\t").into_bytes();
        bad8.extend([0x80, 0]);
        for (tree, diff) in [
            (tre("100644", "tree", &m, "p"), &b""[..]),
            (tre("100755", "commit", &m, "p"), &b""[..]),
            (no_tab, &b""[..]),
            (good[..good.len() - 1].to_vec(), &b""[..]),
            (tre("100644", "blob", &"z".repeat(40), "p"), &b""[..]),
            (tre("100644", "blob", &m, "a\nb"), &b""[..]),
            (bad8, &b""[..]),
            (vec![], &b"a\0b"[..]),
            (vec![], &b"../x\0"[..]),
            (vec![], &b"a\nb\0"[..]),
        ] {
            seed(&f, &tree, diff, &[], [0, 0, 0]);
            assert_eq!(data(inputs(&f).unwrap_err()).to_string(), LISTING);
        }
        seed(&f, &good, b"", &[], [17, 0, 0]);
        assert_eq!(data(inputs(&f).unwrap_err()).to_string(), LISTING);
        seed(
            &f,
            &good,
            b"",
            &[(&m, ROOT_MANIFEST.as_bytes())],
            [0, 17, 0],
        );
        assert_eq!(data(inputs(&f).unwrap_err()).to_string(), LISTING);
        seed(&f, &good, b"", &[], [0, 0, 17]);
        assert_eq!(data(inputs(&f).unwrap_err()).to_string(), BLOB);
        let miss = {
            let empty = f.cleanup.join("no-git");
            fs::create_dir_all(&empty).unwrap();
            let _g = env_lock();
            let old = env::var_os("PATH");
            unsafe { env::set_var("PATH", &empty) };
            let result = base_diff_inputs(&f.root, &base());
            unsafe {
                match old {
                    Some(value) => env::set_var("PATH", value),
                    None => env::remove_var("PATH"),
                }
            }
            result
        };
        assert!(matches!(miss, Err(BaseInputsError::Tool(_))));

        let bad = oid('1');
        let mut t = tre("100644", "blob", &m, "boxology.toml");
        t.extend(tre("100644", "blob", &bad, "ping/boxology.toml"));
        seed(
            &f,
            &t,
            b"",
            &[(&m, ROOT_MANIFEST.as_bytes()), (&bad, b"x")],
            [0, 0, 0],
        );
        match inputs(&f).unwrap_err() {
            BaseInputsError::Declarations { header, findings } => assert_eq!(
                format!("{header}\n{findings}"),
                format!(
                    "{HDR}\nBXW0002 ping/boxology.toml:1:2-1:2 offending=\"manifest document\" rule=\"boxology.toml must be well-formed TOML\" source=\"specs/s5-manifest-and-validation.md D2\""
                )
            ),
            o => panic!("{o}"),
        }
        let (a, b) = (oid('2'), oid('3'));
        let mut t = tre("100644", "blob", &m, "boxology.toml");
        t.extend(tre("100644", "blob", &a, "a/boxology.toml"));
        t.extend(tre("100644", "blob", &b, "b/boxology.toml"));
        seed(
            &f,
            &t,
            b"",
            &[
                (&m, ROOT_MANIFEST.as_bytes()),
                (&a, TWIN.as_bytes()),
                (&b, TWIN.as_bytes()),
            ],
            [0, 0, 0],
        );
        match inputs(&f).unwrap_err() {
            BaseInputsError::Declarations { header, findings } => assert_eq!(
                format!("{header}\n{findings}"),
                format!(
                    "{HDR}\nBXW0042 a/boxology.toml package=twin candidates=[]\nBXW0042 b/boxology.toml package=twin candidates=[]"
                )
            ),
            o => panic!("{o}"),
        }

        let (held, link) = (oid('4'), oid('5'));
        let mut t = tre("100644", "blob", &m, "boxology.toml");
        t.extend(tre("100644", "blob", &held, "corpus/sample/boxology.toml"));
        t.extend(tre("120000", "blob", &link, "linked/boxology.toml"));
        seed(
            &f,
            &t,
            b"",
            &[
                (&m, PLATFORM_OWN.as_bytes()),
                (&held, PACKAGE_MANIFEST.as_bytes()),
                (&link, b"../boxology.toml"),
            ],
            [0, 0, 0],
        );
        let got = inputs(&f).unwrap();
        assert_eq!(
            got.packages()
                .iter()
                .map(|p| p.id().as_str())
                .collect::<Vec<_>>(),
            ["platform"]
        );
    }
}
