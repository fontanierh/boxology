#![cfg(unix)]

use boxology_init::{InitRequest, initialize};
use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    cleanup: PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.cleanup);
    }
}
impl Fixture {
    fn new() -> Self {
        let cleanup = env::temp_dir().join(format!(
            "boxology-init-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&cleanup).unwrap();
        let root = cleanup.join("target");
        fs::create_dir(&root).unwrap();
        Self { root, cleanup }
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_boxology-init"))
            .args(args)
            .output()
            .unwrap()
    }
    fn run_init(&self, name: &str) -> Output {
        self.run(&[
            "--name",
            name,
            "--dependency-source",
            "../boxology",
            "--target",
            self.root.to_str().unwrap(),
        ])
    }
}
fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

#[rustfmt::skip]
fn assert_usage(args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_boxology-init")).args(args).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{args:?}");
    assert!(text(&output.stdout).is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("BXI0009 <argv>:"), "{stderr}");
    assert!(stderr.contains("all parameters must be given as explicit flags: `--name`, `--dependency-source`, `--target`"));
    assert!(stderr.contains("usage: boxology-init --name <project-name> --dependency-source <path> --target <directory>\n"));
}

#[test]
#[rustfmt::skip]
fn malformed_invocations_are_usage_failures() {
    for args in [
        &[][..], &["--name"][..], &["--name", "demo", "--dependency-source", "s"][..],
        &["--name", "demo", "--dependency-source", "s", "--target", "t", "extra"][..],
        &["--unknown", "x"][..], &["demo"][..],
        &["--name=demo", "--dependency-source", "s", "--target", "t"][..],
    ] {
        assert_usage(args);
    }
    let output = Command::new(env!("CARGO_BIN_EXE_boxology-init"))
        .arg(OsString::from_vec(vec![0xff])).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).starts_with("BXI0009 "));
}

#[test]
fn invalid_request_surfaces_before_target_checks() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("stray"), b"x").unwrap();
    let bad_name = fixture.run_init("Bad.Name");
    assert_eq!(bad_name.status.code(), Some(1));
    assert!(text(&bad_name.stdout).is_empty());
    assert!(text(&bad_name.stderr).starts_with("BXI0001 "));
    assert!(!text(&bad_name.stderr).contains("BXI0006"));
    let empty_source = fixture.run(&[
        "--name",
        "demo",
        "--dependency-source",
        "",
        "--target",
        fixture.root.to_str().unwrap(),
    ]);
    assert_eq!(empty_source.status.code(), Some(1));
    assert!(text(&empty_source.stderr).starts_with("BXI0002 "));
}

#[test]
fn missing_target_is_coded() {
    let fixture = Fixture::new();
    let missing = fixture.cleanup.join("absent");
    let output = fixture.run(&[
        "--name",
        "demo",
        "--dependency-source",
        "../boxology",
        "--target",
        missing.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("BXI0005 "));
    assert!(stderr.contains(missing.to_str().unwrap()));
}

#[test]
fn non_empty_target_names_offending_entries() {
    let alone = Fixture::new();
    fs::write(alone.root.join(".DS_Store"), b"x").unwrap();
    let alone_out = alone.run_init("demo");
    let stderr = text(&alone_out.stderr);
    assert!(stderr.starts_with("BXI0006 "));
    assert!(stderr.contains("entries=[\".DS_Store\"]"));
    let both = Fixture::new();
    fs::create_dir(both.root.join(".git")).unwrap();
    fs::write(both.root.join(".DS_Store"), b"x").unwrap();
    let both_out = both.run_init("demo");
    let stderr = text(&both_out.stderr);
    assert!(stderr.starts_with("BXI0006 "));
    assert!(stderr.contains("entries=[\".DS_Store\"]"));
    assert!(!stderr.contains("entries=[\".DS_Store\", \".git\"]"));
}

#[test]
fn generated_tree_rerun_is_sentinel_refusal() {
    let fixture = Fixture::new();
    let tree = initialize(&InitRequest::new("example", "../boxology").unwrap()).unwrap();
    for file in tree.files() {
        let path = fixture.root.join(file.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, file.bytes()).unwrap();
    }
    let output = fixture.run_init("example");
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("BXI0007 "));
    assert!(!stderr.contains("BXI0006 "));
}
