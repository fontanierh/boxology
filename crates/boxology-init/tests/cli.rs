#![cfg(unix)]

use boxology_init::{InitRequest, initialize};
use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

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
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let mut attempt = 0;
        let cleanup = loop {
            let candidate = env::temp_dir().join(format!(
                "boxology-init-cli-{}-{stamp}-{attempt}",
                std::process::id()
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
                Err(error) => panic!("create temporary fixture: {error}"),
            }
        };
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
        self.run(&["--name", name, "--target", self.root.to_str().unwrap()])
    }
}
fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

#[test]
fn help_is_a_stable_successful_installed_binary_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_boxology-init"))
        .arg("--help")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        text(&output.stdout),
        "usage: boxology-init --name <project-name> --target <directory>\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn version_is_a_stable_successful_installed_binary_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_boxology-init"))
        .arg("--version")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        text(&output.stdout),
        concat!("boxology-init ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert!(output.stderr.is_empty());
}

fn regular_files(root: &Path) -> Vec<String> {
    fn visit(root: &Path, directory: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let file_type = entry.file_type().unwrap();
            if file_type.is_dir() {
                visit(root, &path, found);
            } else {
                assert!(
                    file_type.is_file(),
                    "not a regular file: {}",
                    path.display()
                );
                found.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }

    let mut found = Vec::new();
    visit(root, root, &mut found);
    found.sort();
    found
}

fn assert_tree_matches_oracle(root: &Path) {
    let oracle = initialize(&InitRequest::new("example").unwrap()).unwrap();
    assert!(!oracle.files().is_empty());
    let expected_paths: Vec<_> = oracle
        .files()
        .iter()
        .map(|file| file.path().to_owned())
        .collect();
    assert_eq!(regular_files(root), expected_paths);

    let mut expected_entries = Vec::new();
    for file in oracle.files() {
        let entry = file.path().split('/').next().unwrap().to_owned();
        if !expected_entries.contains(&entry) {
            expected_entries.push(entry);
        }
    }
    expected_entries.sort();
    let mut actual_entries: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    actual_entries.sort();
    assert_eq!(actual_entries, expected_entries);

    for file in oracle.files() {
        let path = root.join(file.path());
        assert!(
            path.is_file(),
            "generated path is not a regular file: {path:?}"
        );
        assert_eq!(
            fs::read(&path).unwrap(),
            file.bytes(),
            "bytes differ for {path:?}"
        );
    }
}

#[rustfmt::skip]
fn assert_usage(args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_boxology-init")).args(args).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{args:?}");
    assert!(text(&output.stdout).is_empty());
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("BXI0009 <argv>:"), "{stderr}");
    assert!(stderr.contains("all parameters must be given as explicit flags: `--name`, `--target`"));
    assert!(stderr.contains("usage: boxology-init --name <project-name> --target <directory>\n"));
}

#[test]
#[rustfmt::skip]
fn malformed_invocations_are_usage_failures() {
    for args in [
        &[][..], &["--name"][..], &["--name", "demo"][..],
        &["--version", "extra"][..],
        &["--name", "demo", "--target", "t", "extra"][..],
        &["--unknown", "x"][..], &["demo"][..],
        &["--name=demo", "--target", "t"][..],
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
}

#[test]
fn missing_target_is_coded() {
    let fixture = Fixture::new();
    let missing = fixture.cleanup.join("absent");
    let output = fixture.run(&["--name", "demo", "--target", missing.to_str().unwrap()]);
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
fn success_installs_the_library_tree_byte_exactly() {
    let fixture = Fixture::new();
    let output = fixture.run_init("example");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(text(&output.stdout), "initialized example\n");
    assert!(text(&output.stderr).is_empty());
    assert_tree_matches_oracle(&fixture.root);
}

#[test]
fn success_keeps_a_preexisting_git_directory() {
    let fixture = Fixture::new();
    let head = fixture.root.join(".git").join("HEAD");
    fs::create_dir_all(head.parent().unwrap()).unwrap();
    fs::write(&head, b"known").unwrap();
    let output = fixture.run_init("example");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(fs::read(&head).unwrap(), b"known");
}

#[test]
fn cli_written_tree_is_sentinel_refused_on_rerun() {
    let fixture = Fixture::new();
    let first = fixture.run_init("example");
    assert_eq!(first.status.code(), Some(0));
    let output = fixture.run_init("example");
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.starts_with("BXI0007 "));
    assert!(!stderr.contains("BXI0006"));
}

#[test]
fn generated_tree_rerun_is_sentinel_refusal() {
    let fixture = Fixture::new();
    let tree = initialize(&InitRequest::new("example").unwrap()).unwrap();
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
