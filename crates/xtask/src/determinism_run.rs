use crate::determinism::{Manifest, scan_subject_trees};
use crate::determinism_publish::publish;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

const MARKER: &str = ".boxology-determinism-run";
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
static NEXT_RUN: AtomicU64 = AtomicU64::new(0);
type Prepare = fn(&Path) -> std::result::Result<(), String>;

pub(crate) struct Subject {
    pub name: &'static str,
    pub prepare: Option<Prepare>,
    pub argv: fn(&Path) -> (PathBuf, Vec<OsString>),
}

struct Capture {
    bytes: Vec<u8>,
    truncated: bool,
}

struct Outcome {
    stdout: Capture,
    stderr: Capture,
}

#[derive(Debug)]
enum Failure {
    Finding(String),
    Infra(String),
}

type Result<T> = std::result::Result<T, Failure>;

pub fn local(workspace: &Path) -> u8 {
    let subjects = match registry() {
        Ok(subjects) => subjects,
        Err(error) => return report(Failure::Infra(error), None),
    };
    let root = match create_run_root(workspace) {
        Ok(root) => root,
        Err(error) => return report(error, None),
    };
    match baseline(workspace, &root, &subjects) {
        Ok(_) => match remove_run_root(&root) {
            Ok(()) => {
                println!("determinism: PASS (baseline observation)");
                0
            }
            Err(error) => report(error, Some(&root)),
        },
        Err(error) => report(error, Some(&root)),
    }
}
pub fn manifest(workspace: &Path, out: &Path) -> u8 {
    let subjects = match registry() {
        Ok(subjects) => subjects,
        Err(error) => return report(Failure::Infra(error), None),
    };
    manifest_with(workspace, out, &subjects)
}
fn manifest_with(workspace: &Path, out: &Path, subjects: &[Subject]) -> u8 {
    let root = match create_run_root(workspace) {
        Ok(root) => root,
        Err(error) => return report(error, None),
    };
    let result = baseline(workspace, &root, subjects).and_then(|manifest| {
        let environment = controlled_env(&root.join("home"), &root.join("tmp"));
        let names: Vec<_> = subjects.iter().map(|subject| subject.name).collect();
        let run_id = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Failure::Infra("run root has no UTF-8 identifier".into()))?;
        publish(
            out,
            &root.join("out/base"),
            &root.join("capture"),
            &manifest,
            &names,
            &environment,
            run_id,
        )
        .map_err(Failure::Infra)
    });
    match result {
        Ok(()) => match remove_run_root(&root) {
            Ok(()) => {
                println!("determinism-manifest: PASS {}", out.display());
                0
            }
            Err(error) => report(error, Some(&root)),
        },
        Err(error) => report(error, Some(&root)),
    }
}
pub fn child(name: &str, out: &Path) -> u8 {
    if name != "trivial-tree" {
        return report(
            Failure::Infra(format!("unknown determinism subject: {name}")),
            None,
        );
    }
    if let Err(error) = ensure_empty_directory(out) {
        return report(Failure::Infra(error), None);
    }
    match run_trivial(out) {
        Ok(()) => 0,
        Err(error) => report(Failure::Finding(error), None),
    }
}
fn registry() -> std::result::Result<Vec<Subject>, String> {
    let subjects = vec![Subject {
        name: "trivial-tree",
        prepare: None,
        argv: trivial_argv,
    }];
    let mut previous: Option<&str> = None;
    for subject in &subjects {
        let mut bytes = subject.name.bytes();
        if !bytes
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            || !bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || previous.is_some_and(|name| name.as_bytes() >= subject.name.as_bytes())
        {
            return Err("subject registry names must be unique, sorted, and canonical".into());
        }
        previous = Some(subject.name);
    }
    Ok(subjects)
}
fn trivial_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    (
        env::current_exe().unwrap_or_default(),
        vec![
            "subject-run".into(),
            "trivial-tree".into(),
            "--out".into(),
            out.as_os_str().into(),
        ],
    )
}

fn run_trivial(out: &Path) -> std::result::Result<(), String> {
    fs::write(
        out.join("README.txt"),
        b"boxology determinism trivial subject v1\n",
    )
    .map_err(|error| format!("write README.txt: {error}"))?;
    fs::write(out.join("empty.txt"), []).map_err(|error| format!("write empty.txt: {error}"))?;
    fs::write(out.join("data.bin"), (0..=255).collect::<Vec<u8>>())
        .map_err(|error| format!("write data.bin: {error}"))?;
    fs::create_dir(out.join("sub")).map_err(|error| format!("create sub: {error}"))?;
    fs::write(out.join("sub/nested.txt"), b"nested\n")
        .map_err(|error| format!("write nested.txt: {error}"))
}

fn create_run_root(workspace: &Path) -> Result<PathBuf> {
    let parent = workspace.join("target/xtask-determinism");
    fs::create_dir_all(&parent).map_err(|error| infra("create run parent", error))?;
    let root = loop {
        let candidate = parent.join(format!(
            "{}-{}",
            std::process::id(),
            NEXT_RUN.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(infra("create run root", error)),
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .map_err(|error| infra("secure run root", error))?;
    }
    let root = root
        .canonicalize()
        .map_err(|error| infra("canonicalize run root", error))?;
    fs::write(root.join(MARKER), b"v1\n").map_err(|error| infra("write run marker", error))?;
    Ok(root)
}

fn remove_run_root(root: &Path) -> Result<()> {
    if fs::read(root.join(MARKER)).ok().as_deref() != Some(b"v1\n") {
        return Err(Failure::Infra(
            "refusing unmarked recursive deletion".into(),
        ));
    }
    fs::remove_dir_all(root).map_err(|error| infra("remove run root", error))
}

fn baseline(workspace: &Path, root: &Path, subjects: &[Subject]) -> Result<Manifest> {
    let (home, tmp, cwd, trees) = (
        root.join("home"),
        root.join("tmp"),
        root.join("cwd"),
        root.join("out/base"),
    );
    for directory in [&home, &tmp, &cwd, &trees] {
        fs::create_dir_all(directory)
            .map_err(|error| infra("create controlled directory", error))?;
    }
    let environment = controlled_env(&home, &tmp);
    let mut outcomes = Vec::new();
    for subject in subjects {
        if let Some(prepare) = subject.prepare {
            prepare(workspace)
                .map_err(|error| Failure::Infra(format!("prepare {}: {error}", subject.name)))?;
        }
        let out = trees.join(subject.name);
        fs::create_dir(&out).map_err(|error| infra("create subject output", error))?;
        outcomes.push((
            subject.name,
            execute(subject, &out, &cwd, root, &environment)?,
        ));
    }
    let manifest = scan_subject_trees(&trees).map_err(Failure::Finding)?;
    if let Some(record) = manifest.records().iter().find(|record| {
        !subjects
            .iter()
            .any(|subject| record.path.starts_with(&format!("{}/", subject.name)))
    }) {
        return Err(Failure::Finding(format!(
            "unregistered subject output: {}",
            record.path
        )));
    }
    for (name, outcome) in outcomes {
        println!(
            "determinism: PASS subject={} experiment=baseline capture={}/{}{}{}",
            name,
            outcome.stdout.bytes.len(),
            outcome.stderr.bytes.len(),
            if outcome.stdout.truncated { "+" } else { "" },
            if outcome.stderr.truncated { "+" } else { "" }
        );
    }
    Ok(manifest)
}

fn controlled_env(home: &Path, tmp: &Path) -> Vec<(OsString, OsString)> {
    vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("HOME".into(), home.as_os_str().into()),
        ("TMPDIR".into(), tmp.as_os_str().into()),
        ("TZ".into(), "UTC".into()),
        ("LC_ALL".into(), "C".into()),
        ("SOURCE_DATE_EPOCH".into(), "1735689600".into()),
    ]
}

fn execute(
    subject: &Subject,
    out: &Path,
    cwd: &Path,
    root: &Path,
    environment: &[(OsString, OsString)],
) -> Result<Outcome> {
    let (program, argv) = (subject.argv)(out);
    let mut child = Command::new(program)
        .args(argv)
        .current_dir(cwd)
        .env_clear()
        .envs(environment.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| infra("start subject", error))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdout = thread::spawn(move || capture(stdout));
    let stderr = thread::spawn(move || capture(stderr));
    let status = child
        .wait()
        .map_err(|error| infra("wait for subject", error))?;
    let outcome = Outcome {
        stdout: join_capture(stdout)?,
        stderr: join_capture(stderr)?,
    };
    let capture_dir = root.join("capture");
    fs::create_dir_all(&capture_dir).map_err(|error| infra("create capture directory", error))?;
    fs::write(
        capture_dir.join(format!("{}.stdout", subject.name)),
        &outcome.stdout.bytes,
    )
    .map_err(|error| infra("retain stdout", error))?;
    fs::write(
        capture_dir.join(format!("{}.stderr", subject.name)),
        &outcome.stderr.bytes,
    )
    .map_err(|error| infra("retain stderr", error))?;
    for (stream, truncated) in [
        ("stdout", outcome.stdout.truncated),
        ("stderr", outcome.stderr.truncated),
    ] {
        if truncated {
            fs::write(
                capture_dir.join(format!("{}.{stream}.truncated", subject.name)),
                b"true\n",
            )
            .map_err(|error| infra("record capture truncation", error))?;
        }
    }
    if status.success() {
        Ok(outcome)
    } else {
        Err(Failure::Finding(format!(
            "SUBJECT-FAILURE subject={} experiment=baseline status={status}",
            subject.name
        )))
    }
}

fn capture(mut stream: impl Read) -> io::Result<Capture> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let keep = count.min(MAX_CAPTURE_BYTES.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep != count;
    }
    Ok(Capture { bytes, truncated })
}

fn join_capture(handle: thread::JoinHandle<io::Result<Capture>>) -> Result<Capture> {
    handle
        .join()
        .map_err(|_| Failure::Infra("capture worker panicked".into()))?
        .map_err(|error| infra("read subject capture", error))
}

fn ensure_empty_directory(path: &Path) -> std::result::Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("inspect output: {error}"))?;
    if !metadata.file_type().is_dir() {
        return Err("subject output is not a real directory".into());
    }
    if fs::read_dir(path)
        .map_err(|error| format!("read output: {error}"))?
        .next()
        .is_some()
    {
        return Err("subject output directory is not empty".into());
    }
    Ok(())
}

fn report(error: Failure, root: Option<&Path>) -> u8 {
    let (code, message) = match error {
        Failure::Finding(message) => (1, message),
        Failure::Infra(message) => (2, message),
    };
    eprintln!(
        "determinism: {}: {message}",
        if code == 1 { "FINDING" } else { "ERROR" }
    );
    if let Some(root) = root {
        eprintln!("determinism: retained run root: {}", root.display());
    }
    code
}

fn infra(context: &str, error: impl std::fmt::Display) -> Failure {
    Failure::Infra(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn env_argv(_: &Path) -> (PathBuf, Vec<OsString>) {
        ("/usr/bin/env".into(), Vec::new())
    }
    fn true_argv(_: &Path) -> (PathBuf, Vec<OsString>) {
        ("/usr/bin/true".into(), Vec::new())
    }
    fn file_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
        (
            "/bin/sh".into(),
            vec![
                "-c".into(),
                "printf stable > \"$1/file.txt\"".into(),
                "boxology-test".into(),
                out.into(),
            ],
        )
    }
    fn capped_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
        (
            "/bin/sh".into(),
            vec![
                "-c".into(),
                "dd if=/dev/zero of=\"$1/00-over\" bs=1048577 count=1 2>/dev/null; i=1; while [ $i -le 17 ]; do n=$(printf %02d $i); dd if=/dev/zero of=\"$1/$n\" bs=1048576 count=1 2>/dev/null; i=$((i+1)); done".into(),
                "boxology-test".into(),
                out.into(),
            ],
        )
    }
    fn subject(name: &'static str, argv: fn(&Path) -> (PathBuf, Vec<OsString>)) -> Subject {
        Subject {
            name,
            prepare: None,
            argv,
        }
    }
    fn workspace(name: &str) -> PathBuf {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/determinism-run-tests")
            .join(format!(
                "{name}-{}",
                NEXT_RUN.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn read(root: &Path, path: &str) -> Vec<u8> {
        fs::read(root.join(path)).unwrap()
    }

    #[test]
    fn executor_passes_only_the_controlled_environment() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = create_run_root(&workspace).unwrap();
        let (home, tmp, cwd, out) = (
            root.join("home"),
            root.join("tmp"),
            root.join("cwd"),
            root.join("out"),
        );
        for directory in [&home, &tmp, &cwd, &out] {
            fs::create_dir(directory).unwrap();
        }
        let environment = controlled_env(&home, &tmp);
        let subject = Subject {
            name: "env-probe",
            prepare: None,
            argv: env_argv,
        };
        let outcome = execute(&subject, &out, &cwd, &root, &environment).unwrap();
        let actual: BTreeSet<_> = String::from_utf8(outcome.stdout.bytes)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        let expected: BTreeSet<_> = environment
            .iter()
            .map(|(key, value)| format!("{}={}", key.to_string_lossy(), value.to_string_lossy()))
            .collect();
        assert_eq!(actual, expected);
        run_trivial(&out).unwrap();
        let data = fs::read(out.join("data.bin")).unwrap();
        assert_eq!(data, (0_u8..=255).collect::<Vec<_>>());
        remove_run_root(&root).unwrap();
    }

    #[test]
    fn manifest_publication_is_stable_with_separate_variable_evidence() {
        let workspace = workspace("manifest");
        let subjects = [subject("fixture", file_argv)];
        let (first, second) = (workspace.join("first"), workspace.join("second"));
        assert_eq!(manifest_with(&workspace, &first, &subjects), 0);
        assert_eq!(manifest_with(&workspace, &second, &subjects), 0);
        assert_eq!(read(&first, "MANIFEST"), read(&second, "MANIFEST"));
        assert_ne!(
            read(&first, "evidence/run.json"),
            read(&second, "evidence/run.json")
        );
        let top: BTreeSet<_> = fs::read_dir(&first)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            top,
            ["MANIFEST", "evidence", "trees"].map(OsString::from).into()
        );
        assert_eq!(read(&first, "trees/fixture/file.txt"), b"stable");
        let atomic = workspace.join("atomic");
        let source = workspace.join("source/fixture");
        let capture = workspace.join("capture");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir(&capture).unwrap();
        fs::write(source.join("file.txt"), b"stable").unwrap();
        let manifest = scan_subject_trees(&workspace.join("source")).unwrap();
        fs::write(source.join("file.txt"), b"mutant").unwrap();
        for stream in ["stdout", "stderr"] {
            fs::write(capture.join(format!("fixture.{stream}")), []).unwrap();
        }
        let error = publish(
            &atomic,
            &workspace.join("source"),
            &capture,
            &manifest,
            &["fixture"],
            &[],
            "atomic",
        )
        .unwrap_err();
        assert!(error.contains("retained bytes differ from manifest"));
        assert!(!atomic.exists());
        assert!(
            !workspace
                .join(format!(".boxology-publish-{}-atomic", std::process::id()))
                .exists()
        );
        assert_eq!(
            manifest_with(
                &workspace,
                &workspace.join("invalid"),
                &[subject("bad", true_argv)]
            ),
            1
        );
        assert!(!workspace.join("invalid").exists());
        let prior_manifest = read(&first, "MANIFEST");
        let prior_body = read(&first, "trees/fixture/file.txt");
        assert_eq!(manifest_with(&workspace, &first, &subjects), 2);
        assert_eq!(read(&first, "MANIFEST"), prior_manifest);
        assert_eq!(read(&first, "trees/fixture/file.txt"), prior_body);
        assert!(!fs::read_dir(&workspace).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".boxology-publish-")
        }));
        let capped = workspace.join("capped");
        assert_eq!(
            manifest_with(&workspace, &capped, &[subject("cap", capped_argv)]),
            0
        );
        let envelope =
            String::from_utf8(read(&capped, "evidence/subjects/cap/envelope.json")).unwrap();
        assert!(envelope.contains("file-limit") && envelope.contains("subject-limit"));
        assert!(!capped.join("trees/cap/00-over").exists());
        assert!(capped.join("trees/cap/16").exists());
        assert!(!capped.join("trees/cap/17").exists());
        fs::remove_dir_all(workspace).unwrap();
    }
}
