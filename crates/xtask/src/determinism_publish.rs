use crate::determinism::{Manifest, hash_file};
use serde_json::json;
use std::{collections::BTreeMap, env, ffi::OsString, fs};
use std::{path::Path, process::Command};

const MARKER: &str = ".boxology-determinism-publish";
pub(crate) const FILE_LIMIT: u64 = 1024 * 1024;
const SUBJECT_LIMIT: u64 = 16 * 1024 * 1024;

pub(crate) fn publish(
    out: &Path,
    trees: &Path,
    capture: &Path,
    manifest: &Manifest,
    subjects: &[&str],
    environment: &[(OsString, OsString)],
    run_id: &str,
) -> Result<(), String> {
    if fs::symlink_metadata(out).is_ok() {
        return Err("manifest output already exists".into());
    }
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !fs::metadata(parent).is_ok_and(|meta| meta.is_dir()) || out.file_name().is_none() {
        return Err("manifest output parent is not a directory".into());
    }
    let stage = parent.join(format!(".boxology-publish-{}-{run_id}", std::process::id()));
    fs::create_dir(&stage).map_err(|error| format!("create publication stage: {error}"))?;
    if let Err(error) = fs::write(stage.join(MARKER), b"v1\n") {
        let _ = fs::remove_dir(&stage);
        return Err(format!("write publication marker: {error}"));
    }
    if let Err(error) = build(
        &stage,
        trees,
        capture,
        manifest,
        subjects,
        environment,
        run_id,
    ) {
        let _ = guarded_remove(&stage);
        return Err(error);
    }
    fs::remove_file(stage.join(MARKER)).map_err(|error| fail(&stage, error))?;
    if let Err(error) = fs::rename(&stage, out) {
        let _ = fs::write(stage.join(MARKER), b"v1\n");
        let _ = guarded_remove(&stage);
        return Err(format!("publish manifest atomically: {error}"));
    }
    Ok(())
}

fn build(
    stage: &Path,
    trees: &Path,
    capture: &Path,
    manifest: &Manifest,
    subjects: &[&str],
    environment: &[(OsString, OsString)],
    run_id: &str,
) -> Result<(), String> {
    let retained = stage.join("trees");
    let evidence = stage.join("evidence");
    fs::create_dir(&retained).map_err(|error| error.to_string())?;
    fs::create_dir(&evidence).map_err(|error| error.to_string())?;
    fs::write(stage.join("MANIFEST"), manifest.serialize()).map_err(|error| error.to_string())?;
    let mut totals = BTreeMap::<&str, u64>::new();
    let mut omissions = BTreeMap::<&str, Vec<_>>::new();
    for record in manifest.records() {
        let (subject, _) = record
            .path
            .split_once('/')
            .ok_or("manifest record has no subject")?;
        if !subjects.contains(&subject) {
            return Err(format!("unregistered subject output: {subject}"));
        }
        let total = totals.entry(subject).or_default();
        let reason = if record.size > FILE_LIMIT {
            Some("file-limit")
        } else if total
            .checked_add(record.size)
            .is_none_or(|sum| sum > SUBJECT_LIMIT)
        {
            Some("subject-limit")
        } else {
            None
        };
        if let Some(reason) = reason {
            omissions
                .entry(subject)
                .or_default()
                .push(json!({"path": record.path, "size": record.size, "reason": reason}));
            continue;
        }
        let destination = retained.join(&record.path);
        fs::create_dir_all(destination.parent().ok_or("retained path has no parent")?)
            .map_err(|error| error.to_string())?;
        fs::copy(trees.join(&record.path), &destination)
            .map_err(|error| format!("retain {}: {error}", record.path))?;
        let (size, sha256) = hash_file(&destination, u64::MAX)
            .map_err(|error| format!("verify retained {}: {error}", record.path))?;
        if size != record.size || sha256 != record.sha256 {
            return Err(format!(
                "retained bytes differ from manifest: {}",
                record.path
            ));
        }
        *total += size;
    }
    let subject_dir = evidence.join("subjects");
    fs::create_dir(&subject_dir).map_err(|error| error.to_string())?;
    for subject in subjects {
        let directory = subject_dir.join(subject);
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        for stream in ["stdout", "stderr"] {
            fs::copy(
                capture.join(format!("{subject}.{stream}")),
                directory.join(format!("{stream}.bin")),
            )
            .map_err(|error| format!("retain {subject} {stream}: {error}"))?;
        }
        let envelope = json!({
            "schema": 1,
            "subject": subject,
            "experiment": "baseline",
            "capture": {
                "stdout_truncated": capture.join(format!("{subject}.stdout.truncated")).exists(),
                "stderr_truncated": capture.join(format!("{subject}.stderr.truncated")).exists()
            },
            "retention": {
                "file_limit": FILE_LIMIT,
                "subject_limit": SUBJECT_LIMIT,
                "omitted": omissions.remove(subject).unwrap_or_default()
            }
        });
        write_json(&directory.join("envelope.json"), &envelope)?;
    }
    let controlled: BTreeMap<_, _> = environment
        .iter()
        .map(|(key, value)| (key.to_string_lossy(), value.to_string_lossy()))
        .collect();
    let run = json!({
        "schema": 1,
        "run_id": run_id,
        "runner": {
            "image_os": env::var("ImageOS").unwrap_or_else(|_| "unavailable".into()),
            "image_version": env::var("ImageVersion").unwrap_or_else(|_| "unavailable".into()),
            "os": env::consts::OS,
            "arch": env::consts::ARCH,
            "target": target()?
        },
        "tools": {"rustc": version("rustc")?, "cargo": version("cargo")?},
        "controlled_environment": controlled
    });
    write_json(&evidence.join("run.json"), &run)
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|error| format!("write {}: {error}", path.display()))
}

fn version(program: &str) -> Result<String, String> {
    let output = Command::new(program)
        .arg("--version")
        .output()
        .map_err(|error| format!("run {program} --version: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} --version exited with {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

fn target() -> Result<String, String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|error| format!("run rustc -vV: {error}"))?;
    if !output.status.success() {
        return Err(format!("rustc -vV exited with {}", output.status));
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
        .ok_or("rustc -vV did not report a host target".into())
}

fn guarded_remove(stage: &Path) -> Result<(), String> {
    if fs::read(stage.join(MARKER)).ok().as_deref() != Some(b"v1\n") {
        return Err("refusing unmarked publication cleanup".into());
    }
    fs::remove_dir_all(stage).map_err(|error| error.to_string())
}

fn fail(stage: &Path, error: impl std::fmt::Display) -> String {
    let _ = guarded_remove(stage);
    error.to_string()
}
