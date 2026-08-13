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
const USAGE: &str = "usage: boxology generate\n       boxology generate --package <id>\n       boxology check\n       boxology check --base <revision>\n       boxology check --format human|json\n";

#[test]
fn help_is_a_stable_successful_installed_binary_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_boxology"))
        .arg("--help")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stdout), USAGE);
    assert!(output.stderr.is_empty());
}
const METADATA_ARGV: &str = "metadata\n--format-version\n1\n--locked\n--no-deps\n";
const REQUEST_DIAGNOSTICS_JSON: &str = r#"{
  "schema": "boxology.generator-diagnostics@1",
  "diagnostics": [
    {
      "code": "BXG0003",
      "path": "implementation/src/lib.rs",
      "span": {
        "start": {
          "line": 1,
          "column": 1
        },
        "end": {
          "line": 1,
          "column": 1
        }
      },
      "offending": "input[1] bytes",
      "rule": "Rust, TOML, and JSON inputs must be valid UTF-8",
      "rule_source": "specs/s2-contract-generator.md D1"
    }
  ]
}
"#;
const MODEL_DIAGNOSTICS_JSON: &str = r#"{
  "schema": "boxology.generator-diagnostics@1",
  "diagnostics": [
    {
      "code": "BXG0038",
      "path": "implementation/src/lib.rs",
      "span": {
        "start": {
          "line": 1,
          "column": 23
        },
        "end": {
          "line": 1,
          "column": 30
        }
      },
      "offending": "invalid controlled contract syntax",
      "rule": "contract tokens must satisfy the controlled v0 grammar",
      "rule_source": "specs/s2-contract-generator.md D3"
    }
  ]
}
"#;
const CLEAN_CHECK_JSON: &str = "{\n  \"schema\": \"boxology.check-report@1\",\n  \"steps\": [\n    {\n      \"id\": \"discovery\",\n      \"status\": \"passed\",\n      \"findings\": []\n    },\n    {\n      \"id\": \"regeneration\",\n      \"status\": \"passed\",\n      \"findings\": []\n    },\n    {\n      \"id\": \"contract-classification\",\n      \"status\": \"skipped\",\n      \"reason\": \"contract classification skipped: no repository is available\",\n      \"findings\": []\n    },\n    {\n      \"id\": \"diff-ownership\",\n      \"status\": \"skipped\",\n      \"reason\": \"not run: no repository is available\",\n      \"findings\": []\n    },\n    {\n      \"id\": \"cargo-graph\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    },\n    {\n      \"id\": \"fmt\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    },\n    {\n      \"id\": \"clippy\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    },\n    {\n      \"id\": \"tests\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    },\n    {\n      \"id\": \"quality\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null\n    }\n  ],\n  \"result\": \"passed\"\n}\n";
const ROOT_MANIFEST: &str = "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n";
const PACKAGE_MANIFEST: &str = "schema = 1\nid = \"ping\"\nkind = \"box\"\nowned = [\"boxology.toml\", \"implementation/**\"]\n\n[[crates]]\ncargo_package = \"ping-implementation\"\npath = \"implementation\"\nrole = \"box-implementation\"\n\n[[crates]]\ncargo_package = \"ping-contract\"\npath = \"generated/contract\"\nrole = \"box-contract\"\n\n[[derived]]\nid = \"contract\"\ngenerator = \"boxology-contract\"\ninputs = [\"boxology.toml\", \"implementation/src/**\"]\noutputs = [\"generated/**\"]\n";
const METADATA: &str = r#"{"workspace_root":"/w","workspace_members":["path+file:///w/ping/generated/contract#0.0.0","path+file:///w/ping/implementation#0.0.0"],"packages":[{"id":"path+file:///w/ping/generated/contract#0.0.0","name":"ping-contract","manifest_path":"/w/ping/generated/contract/Cargo.toml","dependencies":[]},{"id":"path+file:///w/ping/implementation#0.0.0","name":"ping-implementation","manifest_path":"/w/ping/implementation/Cargo.toml","dependencies":[]}] }"#;
const IMPLEMENTATION_ONLY_METADATA: &str = r#"{"workspace_root":"/w","workspace_members":["path+file:///w/ping/implementation#0.0.0"],"packages":[{"id":"path+file:///w/ping/implementation#0.0.0","name":"ping-implementation","manifest_path":"/w/ping/implementation/Cargo.toml","dependencies":[]}] }"#;
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

    fn run_in_parent_repository(&self, args: &[&str]) -> Output {
        let _lock = env_lock();
        let old_path = env::var_os("PATH").unwrap_or_default();
        let path = format!(
            "{}:{}",
            self.cargo.parent().unwrap().display(),
            old_path.display()
        );
        Command::new(env!("CARGO_BIN_EXE_boxology"))
            .args(args)
            .current_dir(&self.root)
            .env("BOXOLOGY_ARG_LOG", &self.log)
            .env("BOXOLOGY_METADATA", &self.metadata)
            .env("BOXOLOGY_MODE", "ok")
            .env("PATH", path)
            .output()
            .unwrap()
    }

    fn cargo_fmt_all(&self) -> Output {
        Command::new("cargo")
            .args(["fmt", "--all"])
            .current_dir(&self.root)
            .output()
            .unwrap()
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
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$BOXOLOGY_GIT_ARG_LOG\"\nprintf '%s\\n' -- >> \"$BOXOLOGY_GIT_ARG_LOG\"\ncase \"$1 $2\" in\n  'rev-parse --verify') printf '%040d\\n' 0;;\n  'ls-tree --name-only') printf '%s\\0' \"$6\";;\n  'ls-tree -r') ;;\n  'diff --name-only') ;;\n  'cat-file -e') exit {exists_status};;\n  'cat-file blob') /bin/cat \"$BOXOLOGY_BASE_BLOB\";;\n  *) exit 19;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn install_bootstrap_cargo(&self, initial_metadata: Option<&str>) {
        let initial = initial_metadata.map_or_else(
            || "printf '%s\\n' 'missing generated contract manifest' >&2; exit 17".to_owned(),
            |metadata| format!("printf '%s\\n' '{metadata}'; exit 0"),
        );
        fs::write(
            &self.cargo,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$BOXOLOGY_ARG_LOG\"\nif [ ! -f ping/generated/contract/Cargo.toml ]; then {initial}; fi\n/bin/cat \"$BOXOLOGY_METADATA\"\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&self.cargo, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn install_fake_git_default(&self, merge_base: &str, exists_status: u8) {
        let git = self.cargo.parent().unwrap().join("git");
        fs::write(
            &git,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$BOXOLOGY_GIT_ARG_LOG\"\nprintf '%s\\n' -- >> \"$BOXOLOGY_GIT_ARG_LOG\"\ncase \"$1 $2\" in\n  'rev-parse --git-dir') printf '%s\\n' .git;;\n  'merge-base HEAD') printf '%s\\n' '{merge_base}';;\n  'rev-parse --verify')\n    case \"$4\" in\n      '{merge_base}^{{commit}}') printf '%s\\n' '{merge_base}';;\n      *) exit 1;;\n    esac;;\n  'ls-tree --name-only') printf '%s\\0' \"$6\";;\n  'ls-tree -r') ;;\n  'diff --name-only') ;;\n  'cat-file -e') exit {exists_status};;\n  'cat-file blob') /bin/cat \"$BOXOLOGY_BASE_BLOB\";;\n  *) exit 19;;\nesac\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

fn generated_snapshot(fixture: &Fixture) -> Vec<Vec<u8>> {
    [
        "generated/adapter/adapter.rs",
        "generated/contract/Cargo.toml",
        "generated/contract/src/lib.rs",
        "generated/schema.json",
    ]
    .map(|path| fs::read(fixture.root.join("ping").join(path)).unwrap())
    .to_vec()
}

fn assert_check_generator_failure(input: &[u8], json: &str, human_diagnostic: &str) {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let before = generated_snapshot(&fixture);
    fs::write(fixture.root.join("ping/implementation/src/lib.rs"), input).unwrap();

    let structured = fixture.run(&["check", "--format", "json"]);
    assert_eq!(structured.status.code(), Some(1));
    assert!(structured.stdout.is_empty());
    assert_eq!(text(&structured.stderr), json);
    assert_eq!(fixture.argv_log(), METADATA_ARGV);
    assert_eq!(generated_snapshot(&fixture), before);

    let human = fixture.run(&["check"]);
    assert_eq!(human.status.code(), Some(1));
    assert!(human.stdout.is_empty());
    assert_eq!(
        text(&human.stderr),
        format!(
            "BXW0071 \"./ping/implementation/src/lib.rs\": the contract generator returned diagnostics: {human_diagnostic}\n"
        )
    );
    assert_eq!(fixture.argv_log(), METADATA_ARGV);
    assert_eq!(generated_snapshot(&fixture), before);
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
        vec!["generate", "--format", "json"],
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
fn check_json_planning_failure_is_exact_and_human_stays_unchanged() {
    let fixture = Fixture::new(false);
    fs::write(
        fixture.root.join("ping/boxology.toml"),
        PACKAGE_MANIFEST.replace("boxology-contract", "other-tool"),
    )
    .unwrap();
    let json = fixture.run(&["check", "--format", "json"]);
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stdout.is_empty());
    assert_eq!(
        text(&json.stderr),
        "{\n  \"schema\": \"boxology.plan-error@1\",\n  \"code\": \"BXW0064\",\n  \"path\": \"ping/boxology.toml\",\n  \"detail\": \"only the boxology-contract generator is supported by generate\",\n  \"source\": \"specs/s5-manifest-and-validation.md D5\"\n}\n"
    );
    assert_eq!(fixture.argv_log(), METADATA_ARGV);

    let human = fixture.run(&["check"]);
    assert_eq!(human.status.code(), Some(1));
    assert!(human.stdout.is_empty());
    assert_eq!(
        text(&human.stderr),
        "BXW0064 \"ping/boxology.toml\": only the boxology-contract generator is supported by generate\n"
    );
    assert_eq!(fixture.argv_log(), METADATA_ARGV);
}

#[test]
fn check_json_preserves_request_diagnostics_before_any_later_step() {
    assert_check_generator_failure(
        &[0xff],
        REQUEST_DIAGNOSTICS_JSON,
        "BXG0003 implementation/src/lib.rs:1:1-1:1 offending=\"input[1] bytes\" rule=\"Rust, TOML, and JSON inputs must be valid UTF-8\" source=\"specs/s2-contract-generator.md D1\"",
    );
}

#[test]
fn check_json_preserves_controlled_contract_diagnostics_before_any_later_step() {
    assert_check_generator_failure(
        b"boxology::contract! { private }",
        MODEL_DIAGNOSTICS_JSON,
        "BXG0038 implementation/src/lib.rs:1:23-1:30 offending=\"invalid controlled contract syntax\" rule=\"contract tokens must satisfy the controlled v0 grammar\" source=\"specs/s2-contract-generator.md D3\"",
    );
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
fn generate_bootstraps_when_missing_contract_blocks_cargo_metadata() {
    let fixture = Fixture::new(false);
    fs::remove_dir_all(fixture.root.join("ping/generated")).unwrap();
    fixture.install_bootstrap_cargo(None);

    let output = fixture.run(&["generate", "--package", "ping"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains("generate ping written\n"));
    assert!(
        fixture
            .root
            .join("ping/generated/contract/Cargo.toml")
            .is_file()
    );
    assert_eq!(
        fixture.argv_log(),
        format!("{METADATA_ARGV}{METADATA_ARGV}")
    );
}

#[test]
fn generate_bootstraps_when_metadata_has_only_the_existing_implementation() {
    let fixture = Fixture::new(false);
    fs::remove_dir_all(fixture.root.join("ping/generated")).unwrap();
    fixture.install_bootstrap_cargo(Some(IMPLEMENTATION_ONLY_METADATA));

    let output = fixture.run(&["generate", "--package", "ping"]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains("generate result changed\n"));
    assert!(fixture.root.join("ping/generated/schema.json").is_file());
    assert_eq!(
        fixture.argv_log(),
        format!("{METADATA_ARGV}{METADATA_ARGV}")
    );
}

#[test]
fn bootstrap_still_rejects_unowned_source_before_writing() {
    let fixture = Fixture::new(true);
    fs::remove_dir_all(fixture.root.join("ping/generated")).unwrap();
    fixture.install_bootstrap_cargo(None);

    let output = fixture.run(&["generate", "--package", "ping"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stdout).is_empty());
    assert!(text(&output.stderr).contains("BXW0044 stray.txt"));
    assert!(!fixture.root.join("ping/generated").exists());
    assert_eq!(fixture.argv_log(), METADATA_ARGV);
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
                    \x20 not run: no repository is available\n\
                    check cargo-graph passed\n\
                    check fmt passed\n\
                    check clippy passed\n\
                    check tests passed\n\
                    check quality passed\n\
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
fn generated_contract_survives_workspace_fmt_and_check_byte_for_byte() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let contract_dependency =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../boxology-contract");
    fs::write(
        fixture.root.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = [\"ping/implementation\", \"ping/generated/contract\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nboxology-contract = {{ path = {contract_dependency:?} }}\n"
        ),
    )
    .unwrap();
    let generated = fixture.root.join("ping/generated/contract/src/lib.rs");
    let before = fs::read(&generated).unwrap();
    assert!(
        before
            .windows(b"#[rustfmt::skip]".len())
            .any(|window| window == b"#[rustfmt::skip]")
    );

    let unprotected = String::from_utf8(before.clone())
        .unwrap()
        .replace("#[rustfmt::skip]\n", "");
    const FORMATTED_ITEM: &str =
        "pub fn contract_descriptor() -> &'static ::boxology_contract::ContractDescriptor {";
    const UNFORMATTED_ITEM: &str =
        "pub fn contract_descriptor( )->&'static ::boxology_contract::ContractDescriptor{";
    assert_eq!(unprotected.matches(FORMATTED_ITEM).count(), 1);
    let unprotected = unprotected.replacen(FORMATTED_ITEM, UNFORMATTED_ITEM, 1);
    fs::write(&generated, &unprotected).unwrap();
    let mutant_format = fixture.cargo_fmt_all();
    assert!(
        mutant_format.status.success(),
        "stdout={} stderr={}",
        text(&mutant_format.stdout),
        text(&mutant_format.stderr)
    );
    assert_ne!(fs::read(&generated).unwrap(), unprotected.as_bytes());
    fs::write(&generated, &before).unwrap();

    let formatted = fixture.cargo_fmt_all();
    assert!(
        formatted.status.success(),
        "stdout={} stderr={}",
        text(&formatted.stdout),
        text(&formatted.stderr)
    );
    assert_eq!(fs::read(&generated).unwrap(), before);

    let checked = fixture.run(&["check"]);
    assert_eq!(
        checked.status.code(),
        Some(0),
        "stdout={} stderr={}",
        text(&checked.stdout),
        text(&checked.stderr)
    );
    assert!(text(&checked.stdout).contains("check regeneration passed\n"));
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
    assert!(stdout.contains("check quality passed\n"));
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
    assert!(stdout.contains("check quality passed\n"));
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
    assert!(stdout.contains("check quality passed\n"));
    assert!(stdout.ends_with("check result failed\n"));
}

fn install_quality_commands(fixture: &Fixture) {
    let root = format!("{ROOT_MANIFEST}\n[quality]\ncommands = [\"cargo quality-platform\"]\n");
    let ping = format!(
        "{PACKAGE_MANIFEST}\n[quality]\ncommands = [\"cargo quality-ping\", \"cargo quality-ping-b\"]\n"
    );
    fs::write(fixture.root.join("boxology.toml"), root).unwrap();
    fs::write(fixture.root.join("ping/boxology.toml"), ping).unwrap();
}

#[test]
fn check_quality_commands_run_after_tests_in_package_id_order() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    install_quality_commands(&fixture);
    let human = fixture.run(&["check"]);
    assert_eq!(human.status.code(), Some(0), "{}", text(&human.stderr));
    assert!(text(&human.stderr).is_empty());
    assert!(text(&human.stdout).contains("check tests passed\ncheck quality passed\n"));
    assert!(text(&human.stdout).ends_with("check result passed\n"));
    assert!(!text(&human.stdout).contains("not implemented"));
    let log = fixture.argv_log();
    let tests_at = log.find("test\n--workspace\n--all-features\n").unwrap();
    let ping_at = log.find("quality-ping\n").unwrap();
    let ping_b_at = log.find("quality-ping-b\n").unwrap();
    let platform_at = log.find("quality-platform\n").unwrap();
    assert!(tests_at < ping_at, "{log}");
    assert!(ping_at < ping_b_at, "{log}");
    assert!(ping_b_at < platform_at, "{log}");
    let json = fixture.run(&["check", "--format", "json"]);
    assert_eq!(json.status.code(), Some(0));
    assert!(text(&json.stdout).contains(
        "\"id\": \"quality\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null"
    ));
    assert!(text(&json.stdout).contains("\"result\": \"passed\""));
}

#[test]
fn check_quality_failure_is_bxw0107_with_captured_output_and_continues() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    install_quality_commands(&fixture);
    let human = fixture.run_tools(&["check"], "ok", true, Some("quality-ping"));
    let stdout = text(&human.stdout);
    assert_eq!(human.status.code(), Some(1));
    assert!(text(&human.stderr).is_empty());
    assert!(stdout.contains("check tests passed\n"));
    assert!(stdout.contains(
        "check quality failed\n\
         \x20 BXW0107 ping/boxology.toml package=ping candidates=[command=\"cargo quality-ping\"]\n\
command=\"cargo quality-ping\"\n\
representative quality-ping failure\n"
    ));
    assert!(stdout.ends_with("check result failed\n"));
    let log = fixture.argv_log();
    assert!(log.contains("quality-ping\n"), "{log}");
    assert!(log.contains("quality-ping-b\n"), "{log}");
    assert!(log.contains("quality-platform\n"), "{log}");
    let json = fixture.run_tools(
        &["check", "--format", "json"],
        "ok",
        true,
        Some("quality-ping"),
    );
    let json_out = text(&json.stdout);
    assert_eq!(json.status.code(), Some(1));
    assert!(json_out.contains(
        "\"id\": \"tests\",\n      \"status\": \"passed\",\n      \"findings\": [],\n      \"output\": null"
    ));
    assert!(json_out.contains(
        "\"id\": \"quality\",\n      \"status\": \"failed\",\n      \"findings\": [\n        {\n          \"kind\": \"workspace\",\n          \"code\": \"BXW0107\",\n          \"path\": \"ping/boxology.toml\",\n          \"package\": \"ping\",\n          \"payload\": \"command=\\\"cargo quality-ping\\\"\",\n          \"rule\": \"a declared quality command failed\",\n          \"rule_source\": \"boxology-details/08-rust-build-topology.md workspace operations and validation baseline step 8; specs/s5-manifest-and-validation.md D6\"\n        }\n      ],\n      \"output\": \"command=\\\"cargo quality-ping\\\"\\nrepresentative quality-ping failure\\n\""
    ));
    assert!(json_out.ends_with("  \"result\": \"failed\"\n}\n"));
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
        vec!["check", "--format", "yaml"],
        vec!["check", "--format", "json", "--format", "json"],
        vec!["check", "--base", "HEAD", "--base", "HEAD"],
        vec!["check", "--format"],
        vec!["check", "--format", ""],
        vec!["check", "--base"],
        vec!["check", "--base", "HEAD", "extra"],
        vec!["check", "--base=HEAD"],
        vec!["check", "--format=json"],
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
fn check_json_clean_workspace_is_exact_byte_golden() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let output = fixture.run(&["check", "--format", "json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stdout), CLEAN_CHECK_JSON);
    assert!(text(&output.stderr).is_empty());
    let again = fixture.run(&["check", "--format", "json"]);
    assert_eq!(again.stdout, output.stdout);
    assert_eq!(again.stderr, output.stderr);
}

#[test]
fn check_format_human_matches_default_bytes() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let defaulted = fixture.run(&["check"]);
    let explicit = fixture.run(&["check", "--format", "human"]);
    assert_eq!(explicit.status.code(), defaulted.status.code());
    assert_eq!(explicit.stdout, defaulted.stdout);
    assert_eq!(explicit.stderr, defaulted.stderr);
    let reversed = fixture.run(&["check", "--format", "human", "--base", "HEAD"]);
    let ordered = fixture.run(&["check", "--base", "HEAD", "--format", "human"]);
    // Both forms are accepted; without a repository they fail the same way on base resolution.
    assert_eq!(reversed.status.code(), ordered.status.code());
    assert_eq!(reversed.stdout, ordered.stdout);
    assert_eq!(reversed.stderr, ordered.stderr);
}

#[test]
fn check_json_lock_failure_is_structured_failed_with_output() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    let output = fixture.run_with(&["check", "--format", "json"], "fail-lock", true);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).is_empty());
    assert!(stdout.contains("\"schema\": \"boxology.check-report@1\""));
    assert!(stdout.contains(
        "\"id\": \"cargo-graph\",\n      \"status\": \"failed\",\n      \"findings\": [\n        {\n          \"kind\": \"workspace\",\n          \"code\": \"BXW0097\",\n          \"path\": \"Cargo.lock\",\n          \"package\": null,\n          \"payload\": \"command=\\\"cargo metadata --format-version 1 --locked\\\"\",\n          \"rule\": \"cargo graph and lockfile freshness check failed\",\n          \"rule_source\": \"boxology-details/08-rust-build-topology.md workspace operations and validation baseline step 4; specs/s5-manifest-and-validation.md D6\"\n        }\n      ],\n      \"output\": \"representative lock failure\\n\""
    ));
    assert!(stdout.ends_with("  \"result\": \"failed\"\n}\n"));
}

#[test]
fn check_json_accepts_format_and_base_in_either_order() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    fixture.commit("baseline");
    let left = fixture.run(&["check", "--format", "json", "--base", "HEAD"]);
    let right = fixture.run(&["check", "--base", "HEAD", "--format", "json"]);
    assert_eq!(left.status.code(), Some(0));
    assert_eq!(right.status.code(), Some(0));
    assert_eq!(left.stdout, right.stdout);
    assert!(text(&left.stderr).is_empty());
    assert!(text(&left.stdout).contains("\"id\": \"contract-classification\""));
    assert!(text(&left.stdout).contains("\"status\": \"passed\""));
    assert!(text(&left.stdout).contains("\"result\": \"passed\""));

    let no_repo = Fixture::new(false);
    assert_eq!(no_repo.run(&["generate"]).status.code(), Some(0));
    let defaulted = no_repo.run(&["check", "--format", "json"]);
    assert_eq!(defaulted.status.code(), Some(0));
    assert_eq!(text(&defaulted.stdout), CLEAN_CHECK_JSON);
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
fn check_base_resolves_git_objects_from_a_nested_managed_workspace() {
    let fixture = Fixture::new(false);
    assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "Boxology Test"][..],
        &["config", "user.email", "boxology@example.invalid"][..],
        &["add", "."][..],
        &["commit", "-q", "-m", "nested baseline"][..],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(&fixture.cleanup)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output.stderr));
    }

    let output = fixture.run_in_parent_repository(&["check", "--base", "HEAD"]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains("check contract-classification passed\n"));
    assert!(text(&output.stdout).ends_with("check result passed\n"));
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
             cat-file\n-e\n{oid}:./ping/generated/schema.json\n--\n\
             cat-file\nblob\n{oid}:./ping/generated/schema.json\n--\n\
             ls-tree\n-r\n-z\n{oid}\n--\n.\n--\n\
             diff\n--name-only\n--relative\n-z\n--no-renames\n--no-ext-diff\n{oid}\n--\n.\n--\n"
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
    // Generated files alone are derived; keep one non-derived accountable edit for ownership.
    let manifest = fixture.root.join("ping/implementation/Cargo.toml");
    let mut body = fs::read_to_string(&manifest).unwrap();
    body.push('\n');
    fs::write(&manifest, body).unwrap();
    let output = fixture.run(&["check", "--base", "HEAD"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).is_empty());
    assert!(text(&output.stdout).contains("check contract-classification failed\n"));
    assert!(text(&output.stdout).contains("check diff-ownership passed\n"));
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
             cat-file\n-e\n{oid}:./ping/generated/schema.json\n--\n\
             cat-file\nblob\n{oid}:./ping/generated/schema.json\n--\n\
             ls-tree\n-r\n-z\n{oid}\n--\n.\n--\n\
             diff\n--name-only\n--relative\n-z\n--no-renames\n--no-ext-diff\n{oid}\n--\n.\n--\n"
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
const CARGO_MSG: &str = "BXW0106 Cargo.toml: a changed candidate Cargo manifest must be a readable regular file when present";
const CARGO_BASE: &[u8] =
    b"[package]\nname=\"root\"\nversion=\"0.0.0\"\n\n[dependencies]\nserde = \"1\"\n";
const TWIN: &str = "schema = 1\nid = \"twin\"\nkind = \"box\"\nowned = [\"boxology.toml\"]\n";
const PLATFORM_OWN: &str = "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\nfixtures = [\"corpus/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n";
const HDR: &str =
    "BXW0105 .git: the base revision's package declarations must form a discoverable workspace";
const GIT_SH: &str = "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$BOXOLOGY_GIT_ARG_LOG\"\nprintf '%s\\n' -- >> \"$BOXOLOGY_GIT_ARG_LOG\"\nprintf '%s\\n' \"$BOXOLOGY_GIT_SECRET\" >&2\nst(){ read s < \"$BOXOLOGY_GIT_DATA/$1\"; [ \"$s\" -eq 0 ]||exit \"$s\"; }\ncase \"$1 $2\" in\n 'ls-tree -r') st ts;cat \"$BOXOLOGY_GIT_DATA/t\";;\n 'diff --name-only') st ds;cat \"$BOXOLOGY_GIT_DATA/d\";;\n 'cat-file blob') st bs;cat \"$BOXOLOGY_GIT_DATA/b/$3\";;\n 'rev-parse --verify') printf '%s\\n' aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa;;\n *) exit 19;;\nesac\n";

mod ingest {
    use super::*;
    use boxology_cli::{BaseError, BaseInputsError, ResolvedBase, base_diff_inputs};
    use boxology_manifest::RelativePath;
    use boxology_workspace::diff_ownership;
    use std::os::unix::fs::symlink;

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
    fn rp(path: &str) -> RelativePath {
        RelativePath::new(path).unwrap()
    }
    fn cargo_err(
        result: Result<Vec<boxology_workspace::CargoManifestChange>, BaseInputsError>,
    ) -> String {
        match result {
            Err(error) => data(error).to_string(),
            Ok(_) => panic!("expected cargo error"),
        }
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
                "ls-tree\n-r\n-z\n{OID40}\n--\n.\n--\ncat-file\nblob\n{m}\n--\ncat-file\nblob\n{l}\n--\ndiff\n--name-only\n--relative\n-z\n--no-renames\n--no-ext-diff\n{OID40}\n--\n.\n--\n"
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

        let (held, link, cargo) = (oid('4'), oid('5'), oid('6'));
        let mut t = tre("100644", "blob", &m, "boxology.toml");
        t.extend(tre("100644", "blob", &held, "corpus/sample/boxology.toml"));
        t.extend(tre("120000", "blob", &link, "linked/boxology.toml"));
        t.extend(tre("100644", "blob", &cargo, "Cargo.toml"));
        seed(
            &f,
            &t,
            b"Cargo.toml\0Cargo.lock\0orphan.rs\0",
            &[
                (&m, PLATFORM_OWN.as_bytes()),
                (&held, PACKAGE_MANIFEST.as_bytes()),
                (&link, b"../boxology.toml"),
                (&cargo, CARGO_BASE),
            ],
            [0, 0, 0],
        );
        with_git(&f, || {
            let got = base_diff_inputs(&f.root, &base()).unwrap();
            assert_eq!(
                got.packages()
                    .iter()
                    .map(|p| p.id().as_str())
                    .collect::<Vec<_>>(),
                ["platform"]
            );
            // Candidate boxology.toml cannot authorize paths unowned under base declarations.
            fs::write(
                f.root.join("boxology.toml"),
                "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"orphan.rs\"]\n",
            )
            .unwrap();
            assert_eq!(
                diff_ownership(got.packages(), &[rp("orphan.rs")])
                    .findings()
                    .unwrap()
                    .to_string(),
                "BXW0098 orphan.rs package= candidates=[]\nBXW0100 orphan.rs package= candidates=[]"
            );
            let own = diff_ownership(got.packages(), &[rp("Cargo.toml"), rp("Cargo.lock")]);
            fs::write(f.root.join("Cargo.toml"), CARGO_BASE).unwrap();
            let present = got.manifest_changes(&f.root, &own).unwrap();
            assert_eq!(present.len(), 1);
            assert_eq!(present[0].path().as_str(), "Cargo.toml");
            assert_eq!(
                own.lockfile_scope(&present).unwrap().unwrap().to_string(),
                "BXW0102 Cargo.lock package=platform candidates=[Cargo.toml=unchanged]"
            );
            fs::remove_file(f.root.join("Cargo.toml")).unwrap();
            let deleted = got.manifest_changes(&f.root, &own).unwrap();
            assert_eq!(deleted.len(), 1);
            assert_eq!(deleted[0].path().as_str(), "Cargo.toml");
            // Deletion yields candidate None; a missing side is a dependency change.
            assert!(own.lockfile_scope(&deleted).unwrap().is_none());
            symlink("x", f.root.join("Cargo.toml")).unwrap();
            assert_eq!(cargo_err(got.manifest_changes(&f.root, &own)), CARGO_MSG);
            fs::remove_file(f.root.join("Cargo.toml")).unwrap();
            fs::create_dir(f.root.join("Cargo.toml")).unwrap();
            assert_eq!(cargo_err(got.manifest_changes(&f.root, &own)), CARGO_MSG);
            fs::remove_dir(f.root.join("Cargo.toml")).unwrap();
            fs::write(f.root.join("Cargo.toml"), CARGO_BASE).unwrap();
            fs::set_permissions(f.root.join("Cargo.toml"), fs::Permissions::from_mode(0o000))
                .unwrap();
            let err = cargo_err(got.manifest_changes(&f.root, &own));
            let _ =
                fs::set_permissions(f.root.join("Cargo.toml"), fs::Permissions::from_mode(0o644));
            assert_eq!(err, CARGO_MSG);
        });
    }
}

mod ownership {
    use super::*;
    use boxology_cli::{ResolvedBase, base_diff_inputs};

    const SKIP_REPO: &str = "check diff-ownership skipped\n  not run: no repository is available\n";
    const SKIP_MERGE: &str =
        "check diff-ownership skipped\n  not run: no merge base with main is available\n";
    const QUALITY: &str = "check quality passed\n";

    fn ready() -> Fixture {
        let fixture = Fixture::new(false);
        assert_eq!(fixture.run(&["generate"]).status.code(), Some(0));
        fixture
    }

    fn section<'a>(stdout: &'a str, name: &str) -> &'a str {
        let start = stdout.find(name).unwrap_or_else(|| panic!("{stdout}"));
        let body = &stdout[start..];
        let end = body.find("\ncheck ").map(|i| i + 1).unwrap_or(body.len());
        &body[..end]
    }

    fn ownership_failed(stdout: &str) -> &str {
        section(stdout, "check diff-ownership failed\n")
    }

    fn stage(fixture: &Fixture) {
        assert!(fixture.git(&["add", "-A"]).status.success());
    }

    #[test]
    fn report_wiring_real_git_cases() {
        // Accountable source edit passes after regeneration.
        let pass = ready();
        pass.commit("base");
        fs::write(
            pass.root.join("ping/implementation/src/lib.rs"),
            CONTRACT_WITH_GREET,
        )
        .unwrap();
        assert_eq!(pass.run(&["generate"]).status.code(), Some(0));
        let output = pass.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(0));
        assert!(text(&output.stderr).is_empty());
        assert!(text(&output.stdout).contains("check diff-ownership passed\n"));
        assert!(text(&output.stdout).contains(QUALITY));
        assert!(!text(&output.stdout).contains("{\""));

        // Unowned under base declarations even when candidate authorizes the path.
        let unowned = ready();
        unowned.commit("base");
        fs::write(
            unowned.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"orphan.rs\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        )
        .unwrap();
        fs::write(unowned.root.join("orphan.rs"), b"x").unwrap();
        stage(&unowned);
        let output = unowned.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(text(&output.stderr).is_empty());
        assert_eq!(
            ownership_failed(text(&output.stdout)),
            "check diff-ownership failed\n  BXW0098 orphan.rs package= candidates=[]\n"
        );

        // Ambiguous base claims on a path the candidate uniquely owns.
        let ambiguous = ready();
        fs::write(
            ambiguous.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"ping/shared/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        )
        .unwrap();
        fs::write(
            ambiguous.root.join("ping/boxology.toml"),
            "schema = 1\nid = \"ping\"\nkind = \"box\"\nowned = [\"boxology.toml\", \"implementation/**\", \"shared/**\"]\n\n[[crates]]\ncargo_package = \"ping-implementation\"\npath = \"implementation\"\nrole = \"box-implementation\"\n\n[[crates]]\ncargo_package = \"ping-contract\"\npath = \"generated/contract\"\nrole = \"box-contract\"\n\n[[derived]]\nid = \"contract\"\ngenerator = \"boxology-contract\"\ninputs = [\"boxology.toml\", \"implementation/src/**\"]\noutputs = [\"generated/**\"]\n",
        )
        .unwrap();
        ambiguous.commit("overlap patterns");
        fs::write(ambiguous.root.join("ping/boxology.toml"), PACKAGE_MANIFEST).unwrap();
        fs::write(
            ambiguous.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"ping/shared/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        )
        .unwrap();
        fs::create_dir_all(ambiguous.root.join("ping/shared")).unwrap();
        fs::write(ambiguous.root.join("ping/shared/x.rs"), b"x").unwrap();
        stage(&ambiguous);
        let output = ambiguous.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(ownership_failed(text(&output.stdout)).contains(
            "BXW0099 ping/shared/x.rs package= candidates=[platform boxology.toml ping/shared/**,ping ping/boxology.toml shared/**]"
        ));

        // Zero non-derived owners (lock-only) and two non-derived owners.
        let zero = ready();
        zero.commit("base");
        fs::write(zero.root.join("Cargo.lock"), b"# changed\n").unwrap();
        let output = zero.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            ownership_failed(text(&output.stdout)),
            "check diff-ownership failed\n  BXW0100 Cargo.lock package= candidates=[]\n"
        );

        let two = ready();
        two.commit("base");
        fs::write(two.root.join("Cargo.toml"), "[workspace]\nmembers = [\"ping/implementation\", \"ping/generated/contract\"]\nresolver = \"3\"\n# touch\n").unwrap();
        fs::write(
            two.root.join("ping/implementation/src/lib.rs"),
            CONTRACT_WITH_GREET,
        )
        .unwrap();
        assert_eq!(two.run(&["generate"]).status.code(), Some(0));
        let output = two.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(1));
        assert!(
            ownership_failed(text(&output.stdout))
                .contains("BXW0100 Cargo.toml package= candidates=[ping,platform]")
        );

        // Foreign-derived output under another package.
        let foreign = ready();
        foreign.commit("base");
        fs::write(
            foreign.root.join("boxology.toml"),
            format!("{ROOT_MANIFEST}\n"),
        )
        .unwrap();
        let contract = foreign.root.join("ping/generated/contract/src/lib.rs");
        let mut bytes = fs::read(&contract).unwrap();
        bytes.push(b'\n');
        fs::write(&contract, bytes).unwrap();
        let output = foreign.run(&["check", "--base", "HEAD"]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}{}",
            text(&output.stdout),
            text(&output.stderr)
        );
        assert!(ownership_failed(text(&output.stdout)).contains(
            "BXW0101 ping/generated/contract/src/lib.rs package=ping candidates=[contract]",
        ));

        // Accountable derived Cargo.toml + lock pass; drive-by BXW0102 payloads.
        let cargo = ready();
        let cargo_manifest = "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n\n[[derived]]\nid = \"extra-manifest\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"extra/Cargo.toml\"]\n";
        fs::write(cargo.root.join("boxology.toml"), cargo_manifest).unwrap();
        fs::create_dir_all(cargo.root.join("extra")).unwrap();
        fs::write(
            cargo.root.join("extra/Cargo.toml"),
            "[package]\nname = \"extra\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        cargo.commit("with extra");
        // Non-derived platform change establishes accountability; derived Cargo.toml proves scope.
        fs::write(
            cargo.root.join("boxology.toml"),
            format!("{cargo_manifest}# touch\n"),
        )
        .unwrap();
        fs::write(
            cargo.root.join("extra/Cargo.toml"),
            "[package]\nname = \"extra\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        fs::write(cargo.root.join("Cargo.lock"), b"# lock\n").unwrap();
        let output = cargo.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stdout));
        assert!(text(&output.stdout).contains("check diff-ownership passed\n"));

        let drive = ready();
        drive.commit("base");
        fs::write(
            drive.root.join("boxology.toml"),
            format!("{ROOT_MANIFEST}\n"),
        )
        .unwrap();
        fs::write(drive.root.join("Cargo.lock"), b"# drive-by\n").unwrap();
        let empty = drive.run(&["check", "--base", "HEAD"]);
        assert_eq!(empty.status.code(), Some(1));
        assert_eq!(
            ownership_failed(text(&empty.stdout)),
            "check diff-ownership failed\n  BXW0102 Cargo.lock package=platform candidates=[]\n"
        );

        let unchanged = ready();
        unchanged.commit("base");
        let root_cargo = fs::read_to_string(unchanged.root.join("Cargo.toml")).unwrap();
        fs::write(
            unchanged.root.join("Cargo.toml"),
            format!("{root_cargo}\n# comment\n"),
        )
        .unwrap();
        fs::write(unchanged.root.join("Cargo.lock"), b"# lock\n").unwrap();
        let output = unchanged.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            ownership_failed(text(&output.stdout)),
            "check diff-ownership failed\n  BXW0102 Cargo.lock package=platform candidates=[Cargo.toml=unchanged]\n"
        );

        let unread = ready();
        fs::write(
            unread.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"tool/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        )
        .unwrap();
        fs::create_dir_all(unread.root.join("tool")).unwrap();
        fs::write(unread.root.join("tool/Cargo.toml"), [0xff, 0xfe]).unwrap();
        unread.commit("unreadable tool manifest");
        fs::write(unread.root.join("tool/Cargo.toml"), [0xff, 0xfe, 0x00]).unwrap();
        fs::write(unread.root.join("Cargo.lock"), b"# lock\n").unwrap();
        fs::write(
            unread.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"tool/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n#\n",
        )
        .unwrap();
        let output = unread.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            ownership_failed(text(&output.stdout)),
            "check diff-ownership failed\n  BXW0102 Cargo.lock package=platform candidates=[tool/Cargo.toml=unreadable]\n"
        );

        // Cargo.toml add and delete supply None sides; rename is add+delete.
        let add = ready();
        fs::write(
            add.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"extra/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        )
        .unwrap();
        fs::create_dir_all(add.root.join("extra")).unwrap();
        add.commit("extra dir");
        fs::write(
            add.root.join("extra/Cargo.toml"),
            "[package]\nname = \"extra\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        fs::write(add.root.join("Cargo.lock"), b"# lock\n").unwrap();
        stage(&add);
        let output = add.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stdout));
        assert!(text(&output.stdout).contains("check diff-ownership passed\n"));

        let delete = ready();
        fs::write(
            delete.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"extra/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        )
        .unwrap();
        fs::create_dir_all(delete.root.join("extra")).unwrap();
        fs::write(
            delete.root.join("extra/Cargo.toml"),
            "[package]\nname = \"extra\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = \"1\"\n",
        )
        .unwrap();
        delete.commit("extra present");
        fs::remove_file(delete.root.join("extra/Cargo.toml")).unwrap();
        fs::write(delete.root.join("Cargo.lock"), b"# lock\n").unwrap();
        let output = delete.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stdout));
        assert!(text(&output.stdout).contains("check diff-ownership passed\n"));

        let rename = ready();
        fs::write(
            rename.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"notes.txt\", \"renamed.txt\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        )
        .unwrap();
        fs::write(rename.root.join("notes.txt"), b"hi").unwrap();
        rename.commit("notes");
        assert!(
            rename
                .git(&["mv", "notes.txt", "renamed.txt"])
                .status
                .success()
        );
        let oid = text(&rename.git(&["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_owned();
        let inputs = base_diff_inputs(&rename.root, &ResolvedBase::from_oid(oid).unwrap()).unwrap();
        assert_eq!(
            inputs
                .changed()
                .iter()
                .map(|path| path.as_str())
                .collect::<Vec<_>>(),
            ["notes.txt", "renamed.txt"],
            "--no-renames must surface rename as delete plus add"
        );
        let output = rename.run(&["check", "--base", "HEAD"]);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stdout));
        assert!(text(&output.stdout).contains("check diff-ownership passed\n"));

        // Cascade BXW0101 + BXW0102 through one Findings ordering.
        let cascade = ready();
        cascade.commit("base");
        fs::write(
            cascade.root.join("boxology.toml"),
            format!("{ROOT_MANIFEST}\n"),
        )
        .unwrap();
        fs::write(cascade.root.join("Cargo.lock"), b"# lock\n").unwrap();
        let contract = cascade.root.join("ping/generated/contract/src/lib.rs");
        let mut bytes = fs::read(&contract).unwrap();
        bytes.push(b'\n');
        fs::write(&contract, bytes).unwrap();
        let first = cascade.run(&["check", "--base", "HEAD"]);
        let second = cascade.run(&["check", "--base", "HEAD"]);
        assert_eq!(first.status.code(), Some(1));
        assert_eq!(first.stdout, second.stdout);
        assert_eq!(
            ownership_failed(text(&first.stdout)),
            "check diff-ownership failed\n  BXW0101 ping/generated/contract/src/lib.rs package=ping candidates=[contract]\n  BXW0102 Cargo.lock package=platform candidates=[]\n"
        );

        // Explicit base and default merge-base equivalence.
        let eq = ready();
        eq.commit("main base");
        assert!(eq.git(&["checkout", "-q", "-b", "work"]).status.success());
        fs::write(eq.root.join("Cargo.lock"), b"# eq\n").unwrap();
        let merge = text(&eq.git(&["merge-base", "HEAD", "main"]).stdout)
            .trim()
            .to_owned();
        let explicit = eq.run(&["check", "--base", &merge]);
        let default = eq.run(&["check"]);
        assert_eq!(explicit.status.code(), Some(1));
        assert_eq!(explicit.status.code(), default.status.code());
        assert_eq!(
            ownership_failed(text(&explicit.stdout)),
            ownership_failed(text(&default.stdout))
        );
        assert_eq!(
            ownership_failed(text(&explicit.stdout)),
            "check diff-ownership failed\n  BXW0100 Cargo.lock package= candidates=[]\n"
        );

        // Typed skips for both classification and ownership.
        let no_repo = ready();
        let output = no_repo.run(&["check"]);
        assert_eq!(output.status.code(), Some(0));
        assert!(text(&output.stdout).contains(SKIP_REPO));
        assert!(
            text(&output.stdout)
                .contains("contract classification skipped: no repository is available")
        );

        let no_merge = ready();
        no_merge.commit("trunk");
        assert!(
            no_merge
                .git(&["branch", "-m", "main", "trunk"])
                .status
                .success()
        );
        let output = no_merge.run(&["check"]);
        assert_eq!(output.status.code(), Some(0));
        assert!(text(&output.stdout).contains(SKIP_MERGE));
        assert!(
            text(&output.stdout)
                .contains("contract classification skipped: no merge base with main is available")
        );

        // Ownership failure continues into later tools; final exit 1.
        let cont = ready();
        cont.commit("base");
        fs::write(cont.root.join("orphan-not"), b"x").unwrap();
        fs::write(
            cont.root.join("boxology.toml"),
            "schema = 1\nid = \"platform\"\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"boxology.toml\", \"orphan-not\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        )
        .unwrap();
        stage(&cont);
        let output = cont.run_tools(&["check", "--base", "HEAD"], "ok", true, Some("clippy"));
        let stdout = text(&output.stdout);
        assert_eq!(output.status.code(), Some(1));
        assert!(stdout.contains("check diff-ownership failed\n"));
        assert!(stdout.contains("BXW0098 orphan-not package= candidates=[]"));
        assert!(stdout.contains("check clippy failed\n"));
        assert!(stdout.contains(QUALITY));
        assert!(stdout.ends_with("check result failed\n"));

        // Deterministic composite human bytes for one ownership failure.
        let det = ready();
        det.commit("base");
        fs::write(det.root.join("Cargo.lock"), b"# x\n").unwrap();
        let first = det.run(&["check", "--base", "HEAD"]);
        let second = det.run(&["check", "--base", "HEAD"]);
        assert_eq!(first.stdout, second.stdout);
        assert_eq!(first.stderr, second.stderr);
        let stdout = text(&first.stdout);
        assert!(stdout.starts_with("check discovery passed\ncheck regeneration passed\n"));
        assert!(stdout.contains(
            "check diff-ownership failed\n  BXW0100 Cargo.lock package= candidates=[]\n"
        ));
        assert!(stdout.contains(QUALITY));
        assert!(stdout.ends_with("check result failed\n"));
        assert!(!stdout.contains("{\"diff"));
    }
}
