use crate::determinism::{Manifest, ManifestRecord, byte_diff, diff_records, manifest_side};
use crate::determinism_publish::FILE_LIMIT;
use crate::determinism_run::{Failure, report};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug, Eq, PartialEq)]
enum Comparison {
    Match { records: usize },
    Finding(String),
}
fn evaluate(a: &Path, b: &Path) -> Result<Comparison, Failure> {
    let left_manifest = load(a, "left")?;
    let right_manifest = load(b, "right")?;
    let differences = diff_records(&left_manifest, &right_manifest);
    if differences.is_empty() {
        return Ok(Comparison::Match {
            records: left_manifest.records().len(),
        });
    }
    let (left, right) = differences[0];
    let record = left.or(right).expect("one record differs");
    let subject = record.path.split_once('/').expect("validated path").0;
    let head = format!(
        "cross-platform mismatch subject={subject} experiment=baseline first={} left={} right={}",
        record.path,
        manifest_side(left),
        manifest_side(right)
    );
    let differing = differences.len();
    let message = match (left, right) {
        (Some(left), Some(right)) => {
            let left_bytes = retained(a, left)?;
            let right_bytes = retained(b, right)?;
            match (left_bytes, right_bytes) {
                (Some(left_bytes), Some(right_bytes)) => {
                    let difference =
                        byte_diff(&left_bytes, &right_bytes).ok_or_else(|| corrupt(record))?;
                    format!(
                        "{head} offset={} left_window={} right_window={} left_len={} right_len={} differing={differing}",
                        difference.first_offset,
                        hex(&difference.left_window),
                        hex(&difference.right_window),
                        difference.left_len,
                        difference.right_len
                    )
                }
                _ => format!("{head} retained=absent differing={differing}"),
            }
        }
        _ => format!("{head} differing={differing}"),
    };
    Ok(Comparison::Finding(message))
}
fn load(root: &Path, side: &str) -> Result<Manifest, Failure> {
    let bytes = fs::read(root.join("MANIFEST"))
        .map_err(|error| Failure::Infra(format!("read {side} MANIFEST: {error}")))?;
    Manifest::parse(&bytes)
        .map_err(|error| Failure::Infra(format!("parse {side} MANIFEST: {error}")))
}
fn retained(root: &Path, record: &ManifestRecord) -> Result<Option<Vec<u8>>, Failure> {
    let path = root.join("trees").join(&record.path);
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(corrupt(record)),
    };
    if record.size > FILE_LIMIT {
        return Err(corrupt(record));
    }
    let mut bytes = Vec::new();
    match file
        .take(record.size.min(FILE_LIMIT) + 1)
        .read_to_end(&mut bytes)
    {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(corrupt(record)),
    }
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() as u64 != record.size || sha256 != record.sha256 {
        return Err(corrupt(record));
    }
    Ok(Some(bytes))
}
fn corrupt(record: &ManifestRecord) -> Failure {
    Failure::Infra(format!(
        "retained bytes differ from manifest: {}",
        record.path
    ))
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(text, "{byte:02x}").expect("writing to String cannot fail");
    }
    text
}
pub(crate) fn compare(a: &Path, b: &Path) -> u8 {
    match evaluate(a, b) {
        Ok(Comparison::Match { records }) => {
            println!("determinism-compare: PASS records={records}");
            0
        }
        Ok(Comparison::Finding(message)) => report(Failure::Finding(message), None),
        Err(error) => report(error, None),
    }
}
pub(crate) fn from_args(args: &[String]) -> Option<u8> {
    match args {
        [a, b] if !a.is_empty() && !a.starts_with('-') && !b.is_empty() && !b.starts_with('-') => {
            Some(compare(Path::new(a), Path::new(b)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::determinism::{manifest_side, scan_subject_trees};
    use crate::determinism_run::{Subject, manifest_with};
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn temp(name: &str) -> PathBuf {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/determinism-compare-tests")
            .join(format!(
                "{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn artifact(root: &Path, files: &[(&str, &[u8])]) {
        for (path, bytes) in files {
            let path = root.join("trees").join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let manifest = scan_subject_trees(&root.join("trees")).unwrap();
        fs::write(root.join("MANIFEST"), manifest.serialize()).unwrap();
    }
    fn side(root: &Path, path: &str) -> String {
        let manifest = Manifest::parse(&fs::read(root.join("MANIFEST")).unwrap()).unwrap();
        manifest_side(manifest.records().iter().find(|record| record.path == path))
    }
    fn finding(a: &Path, b: &Path) -> String {
        match evaluate(a, b).unwrap() {
            Comparison::Finding(message) => message,
            other => panic!("expected finding, got {other:?}"),
        }
    }
    fn infra_error(a: &Path, b: &Path) -> String {
        match evaluate(a, b).unwrap_err() {
            Failure::Infra(message) => message,
            other => panic!("expected infrastructure error, got {other:?}"),
        }
    }

    fn stable_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
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

    #[test]
    fn published_match_ignores_evidence_and_accepts_self_comparison() {
        let workspace = temp("match");
        let subject = [Subject {
            name: "stable",
            prepare: None,
            argv: stable_argv,
        }];
        let (a, b) = (workspace.join("a"), workspace.join("b"));
        assert_eq!(manifest_with(&workspace, &a, &subject), 0);
        assert_eq!(manifest_with(&workspace, &b, &subject), 0);
        fs::remove_dir_all(a.join("evidence")).unwrap();
        assert!(matches!(
            evaluate(&a, &b),
            Ok(Comparison::Match { records: 1 })
        ));
        assert_eq!(compare(&a, &b), 0);
        assert_eq!(compare(&a, &a), 0);
        fs::remove_dir_all(workspace).unwrap();
    }
    #[test]
    fn differing_bytes_have_the_exact_full_diagnostic() {
        let root = temp("bytes");
        let (a, b) = (root.join("a"), root.join("b"));
        artifact(&a, &[("s/file", b"abc")]);
        artifact(&b, &[("s/file", b"aXyz")]);
        assert_eq!(
            finding(&a, &b),
            format!(
                "cross-platform mismatch subject=s experiment=baseline first=s/file left={} right={} offset=1 left_window=6263 right_window=58797a left_len=3 right_len=4 differing=1",
                side(&a, "s/file"),
                side(&b, "s/file")
            )
        );
        assert_eq!(compare(&a, &b), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn absent_record_has_no_offset_fields() {
        let root = temp("absent-record");
        let (a, b) = (root.join("a"), root.join("b"));
        artifact(&a, &[("s/a", b"same")]);
        artifact(&b, &[("s/a", b"same"), ("s/b", b"extra")]);
        let message = finding(&a, &b);
        assert_eq!(
            message,
            format!(
                "cross-platform mismatch subject=s experiment=baseline first=s/b left=absent right={} differing=1",
                side(&b, "s/b")
            )
        );
        assert!(!message.contains("offset="));
        assert_eq!(compare(&a, &b), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn multiple_differences_count_all_pairs_and_choose_bytewise_first() {
        let root = temp("multiple");
        let (a, b) = (root.join("a"), root.join("b"));
        artifact(&a, &[("s/a", b"a"), ("s/z", b"z")]);
        artifact(&b, &[("s/a", b"x"), ("s/b", b"b")]);
        let message = finding(&a, &b);
        assert!(
            message.starts_with("cross-platform mismatch subject=s experiment=baseline first=s/a ")
        );
        assert!(message.ends_with(" differing=3"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn argv_requires_exactly_two_plain_nonempty_paths() {
        let root = temp("argv");
        artifact(&root, &[("s/file", b"same")]);
        for args in [
            vec![],
            vec!["one".into()],
            vec!["a".into(), "b".into(), "c".into()],
            vec!["".into(), "b".into()],
            vec!["a".into(), "".into()],
            vec!["-a".into(), "b".into()],
            vec!["a".into(), "--b".into()],
        ] {
            assert_eq!(from_args(&args), None);
        }
        let path = root.to_string_lossy().into_owned();
        assert_eq!(from_args(&[path.clone(), path]), Some(0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_roots_and_manifests_are_side_specific_class_two() {
        let root = temp("invalid");
        let valid = root.join("valid");
        artifact(&valid, &[("s/file", b"same")]);
        let absent = root.join("absent");
        let file = root.join("file");
        fs::write(&file, b"not a root").unwrap();
        let missing = root.join("missing");
        fs::create_dir(&missing).unwrap();
        let directory = root.join("directory");
        fs::create_dir_all(directory.join("MANIFEST")).unwrap();
        for (bad, side) in [
            (&absent, "left"),
            (&file, "left"),
            (&missing, "right"),
            (&directory, "right"),
        ] {
            let (a, b) = if side == "left" {
                (bad.as_path(), valid.as_path())
            } else {
                (valid.as_path(), bad.as_path())
            };
            assert_eq!(compare(a, b), 2);
            assert!(infra_error(a, b).starts_with(&format!("read {side} MANIFEST:")));
        }
        let malformed = root.join("malformed");
        fs::create_dir(&malformed).unwrap();
        fs::write(malformed.join("MANIFEST"), b"bad\n").unwrap();
        for (a, b, side) in [
            (malformed.as_path(), valid.as_path(), "left"),
            (valid.as_path(), malformed.as_path(), "right"),
        ] {
            assert_eq!(compare(a, b), 2);
            assert_eq!(
                infra_error(a, b),
                format!("parse {side} MANIFEST: malformed manifest header")
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retained_absence_degrades_but_corruption_is_class_two() {
        let root = temp("retained");
        let (a, b) = (root.join("a"), root.join("b"));
        artifact(&a, &[("s/file", b"abc")]);
        artifact(&b, &[("s/file", b"axc")]);
        let left_file = a.join("trees/s/file");
        let right_file = b.join("trees/s/file");
        fs::remove_file(&left_file).unwrap();
        let message = finding(&a, &b);
        assert!(message.ends_with(" retained=absent differing=1"));
        assert!(!message.contains("offset="));
        assert_eq!(compare(&a, &b), 1);
        fs::write(&right_file, b"bad").unwrap();
        assert_eq!(
            infra_error(&a, &b),
            "retained bytes differ from manifest: s/file"
        );
        fs::write(&right_file, b"axc").unwrap();
        for corrupt_bytes in [&b"abd"[..], &b"abc!"[..]] {
            fs::write(&left_file, corrupt_bytes).unwrap();
            assert_eq!(compare(&a, &b), 2);
            assert_eq!(
                infra_error(&a, &b),
                "retained bytes differ from manifest: s/file"
            );
        }
        let oversized = vec![b'a'; FILE_LIMIT as usize + 1];
        let mut different = oversized.clone();
        different[0] = b'b';
        artifact(&a, &[("s/file", &oversized)]);
        artifact(&b, &[("s/file", &different)]);
        assert_eq!(compare(&a, &b), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
