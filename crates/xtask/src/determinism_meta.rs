use crate::determinism::{ByteDiff, byte_diff};
use crate::determinism_compare::compare;
use crate::determinism_cross::fixture as platform_fixture;
use crate::determinism_run::{
    Subject, create_run_root, finish_local, manifest_with, protocol, registry, remove_run_root,
    validate_registry,
};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAP_A: &str = "a=1\nb=2\n";
const MAP_B: &str = "b=2\na=1\n";
const LF: &str = "line1\nline2\n";
const CRLF: &str = "line1\r\nline2\r\n";
static MAP_ORDINAL: AtomicU64 = AtomicU64::new(0);
static CRLF_ORDINAL: AtomicU64 = AtomicU64::new(0);
static NEXT_ARTIFACT: AtomicU64 = AtomicU64::new(0);
type Argv = fn(&Path) -> (PathBuf, Vec<OsString>);
type Variant<'a> = (&'a [u8], Argv);

fn shell(out: &Path, script: &str, arguments: &[&str]) -> (PathBuf, Vec<OsString>) {
    let mut argv = vec![
        "-c".into(),
        script.into(),
        "boxology-meta".into(),
        out.into(),
    ];
    argv.extend(arguments.iter().map(OsString::from));
    ("/bin/sh".into(), argv)
}

fn fixed(out: &Path, file: &str, value: &str) -> (PathBuf, Vec<OsString>) {
    shell(out, "printf %s \"$3\" > \"$1/$2\"", &[file, value])
}

// Each varying ordinal is used by exactly one test. Fixed publication variants do not touch them.
fn map_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    let value = if MAP_ORDINAL.fetch_add(1, Ordering::SeqCst) == 1 {
        MAP_B
    } else {
        MAP_A
    };
    fixed(out, "map.txt", value)
}

fn crlf_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    let value = if CRLF_ORDINAL.fetch_add(1, Ordering::SeqCst) == 1 {
        CRLF
    } else {
        LF
    };
    fixed(out, "lines.txt", value)
}

fn timestamp_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    shell(
        out,
        "printf %s \"$SOURCE_DATE_EPOCH\" > \"$1/stamp.txt\"",
        &[],
    )
}

fn path_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    shell(out, "printf %s \"$PWD\" > \"$1/path.txt\"", &[])
}

fn locale_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    shell(
        out,
        "if [ \"$LC_ALL\" = C ]; then printf '1.5\\n'; else printf '1,5\\n'; fi > \"$1/number.txt\"",
        &[],
    )
}

fn map_a_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    fixed(out, "map.txt", MAP_A)
}
fn map_b_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    fixed(out, "map.txt", MAP_B)
}
fn lf_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    fixed(out, "lines.txt", LF)
}
fn crlf_fixed_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    fixed(out, "lines.txt", CRLF)
}

fn subject(name: &'static str, argv: Argv) -> Subject {
    Subject {
        name,
        prepare: None,
        argv,
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn side(bytes: &[u8]) -> String {
    let digest = format!("{:x}", Sha256::digest(bytes));
    format!("{}:{}", bytes.len(), &digest[..16])
}

fn expected_finding(
    label: &str,
    name: &str,
    experiment: &str,
    file: &str,
    baseline: &[u8],
    perturbed: &[u8],
) -> String {
    format!(
        "{label} subject={name} experiment={experiment} first={name}/{file} baseline={} perturbed={}",
        side(baseline),
        side(perturbed)
    )
}

fn retained(root: &Path, experiment: &str, name: &str, file: &str) -> Vec<u8> {
    fs::read(
        root.join("out/retained")
            .join(experiment)
            .join(name)
            .join(file),
    )
    .unwrap()
}

fn exercise(
    fixture: Subject,
    label: &str,
    experiment: &str,
    file: &str,
    bytes: impl FnOnce(&Path) -> (Vec<u8>, Vec<u8>),
) {
    let workspace = workspace();
    let root = create_run_root(&workspace).unwrap();
    let (baseline, perturbed) = bytes(&root);
    let name = fixture.name;
    let findings = protocol(&workspace, &root, &[fixture]).unwrap();
    assert_eq!(
        findings,
        [expected_finding(
            label, name, experiment, file, &baseline, &perturbed,
        )]
    );
    assert_eq!(retained(&root, "baseline", name, file), baseline);
    assert_eq!(retained(&root, experiment, name, file), perturbed);
    assert_eq!(finish_local(&root, Ok(findings)), 1);
    assert!(root.is_dir());
    remove_run_root(&root).unwrap();
    assert!(!root.exists());
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Temp {
    let parent = workspace().join("target/determinism-meta-tests");
    fs::create_dir_all(&parent).unwrap();
    let path = loop {
        let candidate = parent.join(format!(
            "{name}-{}-{}",
            std::process::id(),
            NEXT_ARTIFACT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => break candidate,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("create scratch: {error}"),
        }
    };
    Temp(path)
}

#[test]
fn scratch_skips_same_pid_residue() {
    let pid = std::process::id();
    let parent = workspace().join("target/determinism-meta-tests");
    fs::create_dir_all(&parent).unwrap();
    let start = NEXT_ARTIFACT.load(Ordering::Relaxed);
    let mut blocked = Vec::new();
    // Size from the live counter, plus a sibling budget for allocates that
    // can advance NEXT_ARTIFACT between the end of planting and our scratch() call.
    let budget = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(4);
    loop {
        let n = start + blocked.len() as u64;
        let path = parent.join(format!("residue-{pid}-{n}"));
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("plant residue: {error}"),
        }
        fs::write(path.join("stale"), b"adopt-me").unwrap();
        blocked.push(Temp(path));
        if n > NEXT_ARTIFACT.load(Ordering::Relaxed) + budget {
            break;
        }
    }
    let before = NEXT_ARTIFACT.load(Ordering::Relaxed);
    let got = scratch("residue");
    let after = NEXT_ARTIFACT.load(Ordering::Relaxed);
    assert!(
        after > before + 1,
        "scratch() must iterate past residue (NEXT_ARTIFACT {before} -> {after}), not adopt on first try"
    );
    assert!(
        !blocked.iter().any(|temp| temp.0 == got.0),
        "scratch() must skip same-pid residue, not adopt it; got {}",
        got.0.display()
    );
    for planted in &blocked {
        assert_eq!(
            fs::read(planted.0.join("stale")).unwrap(),
            b"adopt-me",
            "scratch() must leave same-pid residue untouched"
        );
    }
}

fn compare_variants(
    name: &'static str,
    file: &str,
    left: Variant<'_>,
    right: Variant<'_>,
    difference: ByteDiff,
) {
    let temp = scratch(name);
    let (a, b) = (temp.0.join("a"), temp.0.join("b"));
    assert_eq!(manifest_with(&temp.0, &a, &[subject(name, left.1)]), 0);
    assert_eq!(manifest_with(&temp.0, &b, &[subject(name, right.1)]), 0);
    assert_eq!(
        fs::read(a.join("trees").join(name).join(file)).unwrap(),
        left.0
    );
    assert_eq!(
        fs::read(b.join("trees").join(name).join(file)).unwrap(),
        right.0
    );
    assert_eq!(compare(&a, &b), 1);
    assert_eq!(byte_diff(left.0, right.0), Some(difference));
}

#[test]
fn map_order_is_a_deterministic_repeat_finding() {
    MAP_ORDINAL.store(0, Ordering::SeqCst);
    exercise(
        subject("meta-map-order", map_argv),
        "repeat mismatch",
        "repeat",
        "map.txt",
        |_| (MAP_A.as_bytes().to_vec(), MAP_B.as_bytes().to_vec()),
    );
    compare_variants(
        "meta-map-order",
        "map.txt",
        (MAP_A.as_bytes(), map_a_argv),
        (MAP_B.as_bytes(), map_b_argv),
        ByteDiff {
            first_offset: 0,
            left_window: MAP_A.as_bytes().to_vec(),
            right_window: MAP_B.as_bytes().to_vec(),
            common_prefix_len: 0,
            left_len: 8,
            right_len: 8,
        },
    );
}

#[test]
fn crlf_is_a_deterministic_repeat_finding() {
    CRLF_ORDINAL.store(0, Ordering::SeqCst);
    exercise(
        subject("meta-crlf", crlf_argv),
        "repeat mismatch",
        "repeat",
        "lines.txt",
        |_| (LF.as_bytes().to_vec(), CRLF.as_bytes().to_vec()),
    );
    compare_variants(
        "meta-crlf",
        "lines.txt",
        (LF.as_bytes(), lf_argv),
        (CRLF.as_bytes(), crlf_fixed_argv),
        ByteDiff {
            first_offset: 5,
            left_window: b"\nline2\n".to_vec(),
            right_window: b"\r\nline2\r\n".to_vec(),
            common_prefix_len: 5,
            left_len: 12,
            right_len: 14,
        },
    );
}

#[test]
fn timestamp_is_a_deterministic_time_finding() {
    exercise(
        subject("meta-timestamp", timestamp_argv),
        "env-context mismatch",
        "time",
        "stamp.txt",
        |_| (b"1735689600".to_vec(), b"946684800".to_vec()),
    );
}

#[test]
fn absolute_path_is_a_deterministic_path_finding() {
    exercise(
        subject("meta-absolute-path", path_argv),
        "path-context mismatch",
        "path",
        "path.txt",
        |root| {
            let baseline = root.join("cwd").to_string_lossy().into_owned().into_bytes();
            let perturbed = root
                .join("päth context 01/cwd")
                .to_string_lossy()
                .into_owned()
                .into_bytes();
            assert!(
                std::str::from_utf8(&perturbed)
                    .unwrap()
                    .contains("päth context 01")
            );
            (baseline, perturbed)
        },
    );
}

#[test]
fn locale_format_is_a_deterministic_locale_finding() {
    exercise(
        subject("meta-locale-format", locale_argv),
        "env-context mismatch",
        "locale",
        "number.txt",
        |_| (b"1.5\n".to_vec(), b"1,5\n".to_vec()),
    );
}

#[test]
fn meta_fixtures_are_canonical_and_outside_the_live_registry() {
    let registered = registry().unwrap();
    assert_eq!(
        registered.iter().map(|item| item.name).collect::<Vec<_>>(),
        [
            "classifier-report",
            "generated-project",
            "generator-model",
            "trivial-tree",
            "workspace-report",
        ]
    );
    let fixtures = [
        subject("meta-absolute-path", path_argv),
        subject("meta-crlf", crlf_argv),
        subject("meta-locale-format", locale_argv),
        subject("meta-map-order", map_argv),
        platform_fixture(),
        subject("meta-timestamp", timestamp_argv),
    ];
    assert_eq!(
        fixtures.iter().map(|item| item.name).collect::<Vec<_>>(),
        [
            "meta-absolute-path",
            "meta-crlf",
            "meta-locale-format",
            "meta-map-order",
            "meta-platform",
            "meta-timestamp",
        ]
    );
    validate_registry(&fixtures).unwrap();
    assert!(fixtures.iter().all(|fixture| {
        registered
            .iter()
            .all(|subject| subject.name != fixture.name)
    }));
}
