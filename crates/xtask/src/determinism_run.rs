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
const UNUSUAL_CONTEXT: &str = "päth context 01";
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

struct Context {
    home: PathBuf,
    tmp: PathBuf,
    cwd: PathBuf,
    trees: PathBuf,
}

#[derive(Clone, Copy)]
enum Delta {
    None,
    Path,
    Env(&'static str, &'static str),
}

const EXPERIMENTS: [(&str, &str, Delta); 5] = [
    ("repeat", "repeat mismatch", Delta::None),
    ("path", "path-context mismatch", Delta::Path),
    (
        "time",
        "env-context mismatch",
        Delta::Env("SOURCE_DATE_EPOCH", "946684800"),
    ),
    (
        "locale",
        "env-context mismatch",
        Delta::Env("LC_ALL", "en_US.UTF-8"),
    ),
    ("timezone", "env-context mismatch", Delta::Env("TZ", "EST5")),
];

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
    finish_local(&root, protocol(workspace, &root, &subjects))
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
            &root.join("capture/baseline"),
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
pub(crate) fn child_from_args(args: &[String]) -> Option<u8> {
    match args {
        [name, flag, out]
            if flag == "--out"
                && !name.is_empty()
                && !name.starts_with('-')
                && !out.is_empty()
                && !out.starts_with('-') =>
        {
            Some(child(name, Path::new(out)))
        }
        _ => None,
    }
}
fn registry() -> std::result::Result<Vec<Subject>, String> {
    let subjects = vec![Subject {
        name: "trivial-tree",
        prepare: None,
        argv: trivial_argv,
    }];
    validate_registry(&subjects)?;
    Ok(subjects)
}
fn validate_registry(subjects: &[Subject]) -> std::result::Result<(), String> {
    let mut previous: Option<&str> = None;
    for subject in subjects {
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
    Ok(())
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

impl Context {
    fn at(root: &Path, unusual: bool) -> Self {
        let base = if unusual {
            root.join(UNUSUAL_CONTEXT)
        } else {
            root.to_path_buf()
        };
        Self {
            home: base.join("home"),
            tmp: base.join("tmp"),
            cwd: base.join("cwd"),
            trees: base.join("out/base"),
        }
    }

    fn create(&self) -> Result<()> {
        fs::create_dir_all(self.home.parent().expect("context has parent"))
            .map_err(|error| infra("create context root", error))?;
        for directory in [&self.home, &self.tmp, &self.cwd] {
            fs::create_dir(directory)
                .map_err(|error| infra("create controlled directory", error))?;
        }
        fs::create_dir_all(self.trees.parent().expect("output has parent"))
            .map_err(|error| infra("create controlled directory", error))?;
        fs::create_dir(&self.trees).map_err(|error| infra("create subject tree", error))
    }
}

fn baseline(workspace: &Path, root: &Path, subjects: &[Subject]) -> Result<Manifest> {
    let context = Context::at(root, false);
    let environment = controlled_env(&context.home, &context.tmp);
    run_subjects(
        workspace,
        root,
        &context,
        subjects,
        "baseline",
        &environment,
    )
}

fn run_subjects(
    workspace: &Path,
    root: &Path,
    context: &Context,
    subjects: &[Subject],
    experiment: &str,
    environment: &[(OsString, OsString)],
) -> Result<Manifest> {
    context.create()?;
    let mut outcomes = Vec::new();
    for subject in subjects {
        if let Some(prepare) = subject.prepare {
            prepare(workspace)
                .map_err(|error| Failure::Infra(format!("prepare {}: {error}", subject.name)))?;
        }
        let out = context.trees.join(subject.name);
        fs::create_dir(&out).map_err(|error| infra("create subject output", error))?;
        outcomes.push((
            subject.name,
            execute(
                subject,
                &out,
                &context.cwd,
                &root.join("capture").join(experiment),
                experiment,
                environment,
            )?,
        ));
    }
    let manifest = scan_subject_trees(&context.trees).map_err(Failure::Finding)?;
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
            "determinism: PASS subject={} experiment={} capture={}/{}{}{}",
            name,
            experiment,
            outcome.stdout.bytes.len(),
            outcome.stderr.bytes.len(),
            if outcome.stdout.truncated { "+" } else { "" },
            if outcome.stderr.truncated { "+" } else { "" }
        );
    }
    Ok(manifest)
}

fn protocol(workspace: &Path, root: &Path, subjects: &[Subject]) -> Result<Vec<String>> {
    let baseline_context = Context::at(root, false);
    let reference = baseline(workspace, root, subjects)?;
    retain_experiment(root, &baseline_context, "baseline")?;
    let mut findings = Vec::new();
    for (name, label, delta) in EXPERIMENTS {
        let context = Context::at(root, matches!(delta, Delta::Path));
        let mut environment = controlled_env(&context.home, &context.tmp);
        if let Delta::Env(key, value) = delta {
            environment
                .iter_mut()
                .find(|(name, _)| name == key)
                .ok_or_else(|| Failure::Infra(format!("controlled environment lacks {key}")))?
                .1 = value.into();
        }
        match run_subjects(workspace, root, &context, subjects, name, &environment) {
            Ok(observed) => {
                if let Some(finding) = manifest_finding(&reference, &observed, name, label) {
                    findings.push(finding);
                }
            }
            Err(Failure::Finding(finding)) => findings.push(finding),
            Err(error @ Failure::Infra(_)) => return Err(error),
        }
        retain_experiment(root, &context, name)?;
    }
    Ok(findings)
}

fn retain_experiment(root: &Path, context: &Context, experiment: &str) -> Result<()> {
    let retained = root.join("out/retained");
    fs::create_dir_all(&retained).map_err(|error| infra("create retained directory", error))?;
    fs::rename(&context.trees, retained.join(experiment))
        .map_err(|error| infra("retain experiment tree", error))?;
    let parent = root.join("scratch");
    fs::create_dir_all(&parent).map_err(|error| infra("create scratch retention", error))?;
    let scratch = parent.join(experiment);
    fs::create_dir(&scratch).map_err(|error| infra("create experiment retention", error))?;
    for (name, source) in [
        ("home", &context.home),
        ("tmp", &context.tmp),
        ("cwd", &context.cwd),
    ] {
        fs::rename(source, scratch.join(name))
            .map_err(|error| infra("retain experiment scratch", error))?;
    }
    Ok(())
}

fn manifest_finding(
    reference: &Manifest,
    observed: &Manifest,
    experiment: &str,
    label: &str,
) -> Option<String> {
    let mut left = reference.records().iter().peekable();
    let mut right = observed.records().iter().peekable();
    loop {
        let (baseline, perturbed) = match (left.peek(), right.peek()) {
            (Some(a), Some(b)) if a.path == b.path => (left.next(), right.next()),
            (Some(a), Some(b)) if a.path.as_bytes() < b.path.as_bytes() => (left.next(), None),
            (Some(_), Some(_)) => (None, right.next()),
            (Some(_), None) => (left.next(), None),
            (None, Some(_)) => (None, right.next()),
            (None, None) => return None,
        };
        if baseline != perturbed {
            let record = baseline.or(perturbed).expect("one record differs");
            let subject = record.path.split_once('/').expect("validated path").0;
            return Some(format!(
                "{} subject={} experiment={} first={} baseline={} perturbed={}",
                label,
                subject,
                experiment,
                record.path,
                manifest_side(baseline),
                manifest_side(perturbed)
            ));
        }
    }
}

fn manifest_side(record: Option<&crate::determinism::ManifestRecord>) -> String {
    record.map_or_else(
        || "absent".into(),
        |record| format!("{}:{}", record.size, &record.sha256[..16]),
    )
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
    capture_dir: &Path,
    experiment: &str,
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
    fs::create_dir_all(capture_dir).map_err(|error| infra("create capture directory", error))?;
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
            "SUBJECT-FAILURE subject={} experiment={} status={status}",
            subject.name, experiment
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

fn finish_local(root: &Path, result: Result<Vec<String>>) -> u8 {
    match result {
        Ok(findings) if findings.is_empty() => match remove_run_root(root) {
            Ok(()) => {
                println!("determinism: PASS (local protocol)");
                0
            }
            Err(error) => report(error, Some(root)),
        },
        Ok(findings) => {
            for finding in findings {
                eprintln!("determinism: FINDING: {finding}");
            }
            eprintln!("determinism: retained run root: {}", root.display());
            1
        }
        Err(error) => report(error, Some(root)),
    }
}

fn infra(context: &str, error: impl std::fmt::Display) -> Failure {
    Failure::Infra(format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    static REPEAT_CALLS: AtomicU64 = AtomicU64::new(0);

    fn env_argv(_: &Path) -> (PathBuf, Vec<OsString>) {
        ("/usr/bin/env".into(), Vec::new())
    }
    fn true_argv(_: &Path) -> (PathBuf, Vec<OsString>) {
        ("/usr/bin/true".into(), Vec::new())
    }
    fn false_argv(_: &Path) -> (PathBuf, Vec<OsString>) {
        ("/usr/bin/false".into(), Vec::new())
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
    fn context_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
        (
            "/bin/sh".into(),
            vec![
                "-c".into(),
                "printf 'HOME=%s\nLC_ALL=%s\nPATH=%s\nPWD=%s\nSOURCE_DATE_EPOCH=%s\nTMPDIR=%s\nTZ=%s\n' \"$HOME\" \"$LC_ALL\" \"$PATH\" \"$PWD\" \"$SOURCE_DATE_EPOCH\" \"$TMPDIR\" \"$TZ\" > \"$1/context.txt\"".into(),
                "boxology-test".into(),
                out.into(),
            ],
        )
    }
    fn repeat_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
        let changed = REPEAT_CALLS.fetch_add(1, Ordering::SeqCst) == 1;
        (
            "/bin/sh".into(),
            vec![
                "-c".into(),
                "printf %s \"$2\" > \"$1/value.txt\"; printf %s \"$3\"".into(),
                "boxology-test".into(),
                out.into(),
                if changed { "changed\n" } else { "stable\n" }.into(),
                if changed {
                    "repeat-capture\n"
                } else {
                    "stable-capture\n"
                }
                .into(),
            ],
        )
    }
    fn scratch_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
        (
            "/bin/sh".into(),
            vec![
                "-c".into(),
                "test -z \"$(/bin/ls -A \"$HOME\")$(/bin/ls -A \"$TMPDIR\")$(/bin/ls -A .)\" || exit 19; printf %s \"$HOME\" > \"$HOME/seen\"; printf %s \"$TMPDIR\" > \"$TMPDIR/seen\"; printf %s \"$PWD\" > seen; printf stable > \"$1/file.txt\"".into(),
                "boxology-test".into(),
                out.into(),
            ],
        )
    }
    fn flood_payload() -> Vec<u8> {
        (0..(MAX_CAPTURE_BYTES * 32 + 123))
            .map(|index| ((index * 31 + 7) % 251) as u8)
            .collect()
    }
    fn flood_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
        fs::write(out.join("payload"), flood_payload()).unwrap();
        (
            "/bin/sh".into(),
            vec![
                "-c".into(),
                "/bin/cat \"$1/payload\" & /bin/cat \"$1/payload\" >&2 & wait".into(),
                "boxology-test".into(),
                out.into(),
            ],
        )
    }
    fn fail_prepare(_: &Path) -> std::result::Result<(), String> {
        Err("forced setup failure".into())
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
    fn context(root: &Path, experiment: &str) -> BTreeMap<String, String> {
        String::from_utf8(read(
            root,
            &format!("out/retained/{experiment}/context-probe/context.txt"),
        ))
        .unwrap()
        .lines()
        .map(|line| line.split_once('=').unwrap())
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
    }
    fn changed(left: &BTreeMap<String, String>, right: &BTreeMap<String, String>) -> Vec<String> {
        assert_eq!(left.len(), right.len());
        left.iter()
            .filter(|(key, value)| right.get(*key) != Some(*value))
            .map(|(key, _)| key.clone())
            .collect()
    }
    fn child_args(name: &str, flag: &str, out: &Path) -> Vec<String> {
        vec![name.into(), flag.into(), out.to_string_lossy().into_owned()]
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
        let outcome = execute(
            &subject,
            &out,
            &cwd,
            &root.join("capture/baseline"),
            "baseline",
            &environment,
        )
        .unwrap();
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
    fn stable_subject_passes_the_protocol_and_removes_its_root() {
        let workspace = workspace("protocol-green");
        let root = create_run_root(&workspace).unwrap();
        let result = protocol(&workspace, &root, &[subject("stable", file_argv)]);
        assert_eq!(finish_local(&root, result), 0);
        assert!(!root.exists());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn context_probe_isolates_each_controlled_delta() {
        let workspace = workspace("context-probe");
        let root = create_run_root(&workspace).unwrap();
        let findings =
            protocol(&workspace, &root, &[subject("context-probe", context_argv)]).unwrap();
        let heads: Vec<_> = findings
            .iter()
            .map(|finding| finding.split_once(" baseline=").unwrap().0)
            .collect();
        assert_eq!(
            heads,
            [
                "path-context mismatch subject=context-probe experiment=path first=context-probe/context.txt",
                "env-context mismatch subject=context-probe experiment=time first=context-probe/context.txt",
                "env-context mismatch subject=context-probe experiment=locale first=context-probe/context.txt",
                "env-context mismatch subject=context-probe experiment=timezone first=context-probe/context.txt",
            ]
        );
        let baseline = context(&root, "baseline");
        assert_eq!(baseline, context(&root, "repeat"));
        assert_eq!(
            changed(&baseline, &context(&root, "path")),
            ["HOME", "PWD", "TMPDIR"]
        );
        for (experiment, key) in [
            ("time", "SOURCE_DATE_EPOCH"),
            ("locale", "LC_ALL"),
            ("timezone", "TZ"),
        ] {
            assert_eq!(changed(&baseline, &context(&root, experiment)), [key]);
        }
        remove_run_root(&root).unwrap();
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn unusual_context_has_every_required_path_property() {
        assert!(UNUSUAL_CONTEXT.len() > "baseline".len());
        assert!(UNUSUAL_CONTEXT.contains(' '));
        assert!(!UNUSUAL_CONTEXT.is_ascii());
    }

    #[test]
    fn scratch_is_empty_reused_and_retained_for_every_experiment() {
        let workspace = workspace("scratch-isolation");
        let root = create_run_root(&workspace).unwrap();
        let result = protocol(&workspace, &root, &[subject("scratch-probe", scratch_argv)]);
        assert!(result.as_ref().unwrap().is_empty());
        for component in ["home", "tmp", "cwd"] {
            let path = |experiment| format!("scratch/{experiment}/{component}/seen");
            let baseline = read(&root, &path("baseline"));
            for experiment in ["repeat", "time", "locale", "timezone"] {
                assert_eq!(read(&root, &path(experiment)), baseline);
            }
            assert_ne!(read(&root, &path("path")), baseline);
        }
        assert_eq!(finish_local(&root, result), 0);
        assert!(!root.exists());
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn repeat_mismatch_is_exact_and_retains_namespaced_evidence() {
        REPEAT_CALLS.store(0, Ordering::SeqCst);
        let workspace = workspace("repeat-mismatch");
        let root = create_run_root(&workspace).unwrap();
        let findings =
            protocol(&workspace, &root, &[subject("repeat-probe", repeat_argv)]).unwrap();
        assert_eq!(
            findings,
            [
                "repeat mismatch subject=repeat-probe experiment=repeat first=repeat-probe/value.txt baseline=7:2b92ea252be0fbc2 perturbed=8:7f8b1dfc466b6249"
            ]
        );
        assert_eq!(
            read(&root, "out/retained/baseline/repeat-probe/value.txt"),
            b"stable\n"
        );
        assert_eq!(
            read(&root, "out/retained/repeat/repeat-probe/value.txt"),
            b"changed\n"
        );
        assert_eq!(
            read(&root, "capture/repeat/repeat-probe.stdout"),
            b"repeat-capture\n"
        );
        assert_eq!(finish_local(&root, Ok(findings)), 1);
        assert!(root.exists());
        remove_run_root(&root).unwrap();
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn capture_drains_both_pipes_and_persists_exact_bounded_prefixes() {
        let workspace = workspace("capture");
        let published = workspace.join("published");
        assert_eq!(
            manifest_with(
                &workspace,
                &published,
                &[subject("capture-probe", flood_argv)]
            ),
            0
        );
        let payload = flood_payload();
        let expected = &payload[..MAX_CAPTURE_BYTES];
        for stream in ["stdout", "stderr"] {
            let actual = read(
                &published,
                &format!("evidence/subjects/capture-probe/{stream}.bin"),
            );
            assert_eq!(actual.len(), MAX_CAPTURE_BYTES);
            assert_eq!(
                actual, expected,
                "published {stream} retained the wrong prefix"
            );
        }
        let envelope: serde_json::Value = serde_json::from_slice(&read(
            &published,
            "evidence/subjects/capture-probe/envelope.json",
        ))
        .unwrap();
        assert_eq!(envelope["capture"]["stdout_truncated"], true);
        assert_eq!(envelope["capture"]["stderr_truncated"], true);
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn run_root_guard_refuses_recursive_deletion_and_preserves_contents() {
        let workspace = workspace("deletion-guard");
        for (name, marker) in [("absent", None), ("invalid", Some(b"not-v1\n"))] {
            let root = workspace.join(name);
            fs::create_dir(&root).unwrap();
            if let Some(marker) = marker {
                fs::write(root.join(MARKER), marker).unwrap();
            }
            fs::write(root.join("sentinel"), b"preserve me").unwrap();
            assert!(matches!(remove_run_root(&root), Err(Failure::Infra(_))));
            assert_eq!(read(&root, "sentinel"), b"preserve me");
            assert!(root.is_dir());
        }
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn registry_validation_accepts_only_canonical_unique_sorted_names() {
        let subjects = |names: &[&'static str]| {
            names
                .iter()
                .map(|name| subject(name, true_argv))
                .collect::<Vec<_>>()
        };
        assert!(validate_registry(&subjects(&["alpha", "beta-2"])).is_ok());
        for (label, names) in [
            ("noncanonical", &["Alpha"][..]),
            ("duplicate", &["alpha", "alpha"][..]),
            ("unsorted", &["beta", "alpha"][..]),
        ] {
            assert!(
                validate_registry(&subjects(names)).is_err(),
                "accepted {label} registry"
            );
        }
    }

    #[test]
    fn command_exit_classes_are_complete_and_table_driven() {
        #[derive(Clone, Copy, Debug)]
        enum Case {
            Success,
            Unknown,
            Malformed,
            MissingOutput,
            FileOutput,
            NonemptyOutput,
            Setup,
            SubprocessNonzero,
            InvalidTree,
        }
        let cases = [
            (Case::Success, 0),
            (Case::Unknown, 2),
            (Case::Malformed, 2),
            (Case::MissingOutput, 2),
            (Case::FileOutput, 2),
            (Case::NonemptyOutput, 2),
            (Case::Setup, 2),
            (Case::SubprocessNonzero, 1),
            (Case::InvalidTree, 1),
        ];
        let workspace = workspace("exit-classes");
        for (index, (case, expected)) in cases.into_iter().enumerate() {
            let out = workspace.join(format!("case-{index}"));
            let code = match case {
                Case::Success | Case::Unknown => {
                    fs::create_dir(&out).unwrap();
                    let name = if matches!(case, Case::Success) {
                        "trivial-tree"
                    } else {
                        "unknown"
                    };
                    child_from_args(&child_args(name, "--out", &out)).unwrap_or(2)
                }
                Case::Malformed => {
                    child_from_args(&child_args("trivial-tree", "--bad", &out)).unwrap_or(2)
                }
                Case::MissingOutput => {
                    child_from_args(&child_args("trivial-tree", "--out", &out)).unwrap_or(2)
                }
                Case::FileOutput => {
                    fs::write(&out, b"not a directory").unwrap();
                    child_from_args(&child_args("trivial-tree", "--out", &out)).unwrap_or(2)
                }
                Case::NonemptyOutput => {
                    fs::create_dir(&out).unwrap();
                    fs::write(out.join("sentinel"), b"occupied").unwrap();
                    child_from_args(&child_args("trivial-tree", "--out", &out)).unwrap_or(2)
                }
                Case::Setup => manifest_with(
                    &workspace,
                    &out,
                    &[Subject {
                        name: "setup",
                        prepare: Some(fail_prepare),
                        argv: file_argv,
                    }],
                ),
                Case::SubprocessNonzero => {
                    manifest_with(&workspace, &out, &[subject("nonzero", false_argv)])
                }
                Case::InvalidTree => {
                    manifest_with(&workspace, &out, &[subject("invalid-tree", true_argv)])
                }
            };
            assert_eq!(code, expected, "wrong exit class for {case:?}");
        }
        fs::remove_dir_all(workspace).unwrap();
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
