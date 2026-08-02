use crate::determinism::{Manifest, scan_subject_trees};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::{fs, path::Path};

pub(crate) fn from_args(args: &[String]) -> Option<u8> {
    let (dir, target, require_image) = match args {
        [dir, flag, target] if flag == "--target" => (dir, target, false),
        [dir, flag, target, image] if flag == "--target" && image == "--require-image" => {
            (dir, target, true)
        }
        _ => return None,
    };
    if dir.is_empty() || dir.starts_with('-') || target.is_empty() || target.starts_with('-') {
        return None;
    }
    Some(command(Path::new(dir), target, require_image))
}

fn command(dir: &Path, target: &str, require_image: bool) -> u8 {
    match verify(dir, target, require_image) {
        Ok(()) => {
            println!("determinism-verify: PASS {} target={target}", dir.display());
            0
        }
        Err(error) => {
            eprintln!("determinism-verify: ERROR: {error}");
            2
        }
    }
}

pub(crate) fn verify(dir: &Path, target: &str, require_image: bool) -> Result<(), String> {
    let manifest = Manifest::parse(&read_file(&dir.join("MANIFEST"), "MANIFEST")?)
        .map_err(|error| format!("parse MANIFEST: {error}"))?;
    let records: BTreeMap<_, _> = manifest
        .records()
        .iter()
        .map(|r| (r.path.as_str(), r))
        .collect();
    let subjects: BTreeSet<_> = records.keys().map(|path| owner(path)).collect();
    let retained = retained(&dir.join("trees"))?;
    real_dir(&dir.join("evidence"), "evidence")?;
    let subject_root = dir.join("evidence/subjects");
    real_dir(&subject_root, "evidence/subjects")?;
    let mut envelopes = BTreeMap::new();
    let mut omitted = BTreeMap::new();
    for subject in &subjects {
        let label = format!("evidence subject {subject}");
        let subject_dir = subject_root.join(subject);
        real_dir(&subject_dir, &label)?;
        let envelope = read_json(
            &subject_dir.join("envelope.json"),
            &format!("{label} envelope"),
        )?;
        let entries = envelope
            .pointer("/retention/omitted")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{label} retention.omitted must be an array"))?;
        for entry in entries {
            let path = field(entry, "/path", "omission path")?;
            let size = entry
                .get("size")
                .and_then(Value::as_u64)
                .ok_or("omission size must be an unsigned integer")?;
            field(entry, "/reason", "omission reason")?;
            let matches = records.get(path).is_some_and(|record| {
                record.size == size && path.split_once('/').map(|parts| parts.0) == Some(*subject)
            });
            if !matches {
                return Err(format!("unknown omission: {path} size={size}"));
            }
            if omitted.insert(path.to_string(), size).is_some() {
                return Err(format!("duplicate omission: {path}"));
            }
        }
        envelopes.insert(*subject, envelope);
    }
    for (path, (size, sha256)) in &retained {
        let Some(record) = records.get(path.as_str()) else {
            return Err(format!("extra retained file: {path}"));
        };
        if *size != record.size || *sha256 != record.sha256 {
            return Err(format!("retained bytes differ from MANIFEST: {path}"));
        }
        if omitted.contains_key(path) {
            return Err(format!("record is both retained and omitted: {path}"));
        }
    }
    for path in records.keys() {
        if !retained.contains_key(*path) && !omitted.contains_key(*path) {
            return Err(format!(
                "MANIFEST record is neither retained nor omitted: {path}"
            ));
        }
    }

    let run = read_json(&dir.join("evidence/run.json"), "evidence/run.json")?;
    schema(&run, "run.json")?;
    let found = field(&run, "/runner/target", "run.json runner.target")?;
    if found != target {
        return Err(format!(
            "runner target mismatch: expected {target}, found {found}"
        ));
    }
    field(&run, "/tools/rustc", "run.json tools.rustc")?;
    field(&run, "/tools/cargo", "run.json tools.cargo")?;
    for (pointer, image) in [
        ("/runner/image_os", "image_os"),
        ("/runner/image_version", "image_version"),
    ] {
        let value = field(&run, pointer, &format!("run.json runner.{image}"))?;
        if require_image && value == "unavailable" {
            return Err(format!("run.json runner.{image} is unavailable"));
        }
    }

    let seen = evidence_subjects(&subject_root, subjects.len())?;
    if seen.iter().any(|name| !subjects.contains(name.as_str()))
        || subjects.iter().any(|name| !seen.contains(*name))
    {
        return Err("evidence subject set does not match MANIFEST".into());
    }
    for subject in subjects {
        let label = format!("evidence subject {subject}");
        let subject_dir = subject_root.join(subject);
        for stream in ["stdout.bin", "stderr.bin"] {
            real_node(
                &subject_dir.join(stream),
                &format!("{label} {stream}"),
                false,
            )?;
        }
        let envelope = &envelopes[subject];
        schema(envelope, &format!("{label} envelope"))?;
        let found = field(envelope, "/subject", "envelope subject")?;
        if found != subject {
            return Err(format!(
                "envelope subject mismatch: expected {subject}, found {found}"
            ));
        }
        if field(envelope, "/experiment", "envelope experiment")? != "baseline" {
            return Err(format!("{label} experiment must be baseline"));
        }
    }
    Ok(())
}

fn owner(path: &str) -> &str {
    path.split_once('/').expect("validated manifest path").0
}

fn retained(root: &Path) -> Result<BTreeMap<String, (u64, String)>, String> {
    real_dir(root, "trees")?;
    if fs::read_dir(root)
        .map_err(|error| format!("read trees: {error}"))?
        .next()
        .transpose()
        .map_err(|error| format!("read trees: {error}"))?
        .is_none()
    {
        return Ok(BTreeMap::new());
    }
    let scanned = scan_subject_trees(root).map_err(|error| format!("scan trees: {error}"))?;
    Ok(scanned
        .records()
        .iter()
        .map(|record| (record.path.clone(), (record.size, record.sha256.clone())))
        .collect())
}

fn evidence_subjects(root: &Path, expected: usize) -> Result<BTreeSet<String>, String> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(root)
        .map_err(|error| format!("read evidence/subjects: {error}"))?
        .take(expected + 1)
    {
        let entry = entry.map_err(|error| format!("read evidence/subjects: {error}"))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "evidence subject name is not UTF-8")?;
        names.insert(name);
    }
    Ok(names)
}

fn schema(value: &Value, label: &str) -> Result<(), String> {
    if value.get("schema").and_then(Value::as_u64) != Some(1) {
        return Err(format!("{label} schema must equal 1"));
    }
    Ok(())
}

fn field<'a>(value: &'a Value, pointer: &str, label: &str) -> Result<&'a str, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{label} must be a nonempty string"))
}

fn read_json(path: &Path, label: &str) -> Result<Value, String> {
    serde_json::from_slice(&read_file(path, label)?)
        .map_err(|error| format!("parse {label}: {error}"))
}

fn read_file(path: &Path, label: &str) -> Result<Vec<u8>, String> {
    real_node(path, label, false)?;
    fs::read(path).map_err(|error| format!("read {label}: {error}"))
}

fn real_dir(path: &Path, label: &str) -> Result<(), String> {
    real_node(path, label, true)
}

fn real_node(path: &Path, label: &str, directory: bool) -> Result<(), String> {
    let kind = fs::symlink_metadata(path)
        .map_err(|error| format!("inspect {label}: {error}"))?
        .file_type();
    if (directory && kind.is_dir()) || (!directory && kind.is_file()) {
        Ok(())
    } else {
        Err(format!(
            "{label} is not a real {}",
            if directory { "directory" } else { "file" }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    const TARGET: &str = "test-target";
    const RUN: &[u8] = br#"{"schema":1,"runner":{"target":"test-target","image_os":"unavailable","image_version":"unavailable"},"tools":{"rustc":"rustc","cargo":"cargo"}}"#;
    const ENVELOPE: &[u8] =
        br#"{"schema":1,"subject":"s","experiment":"baseline","retention":{"omitted":[]}}"#;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn artifact(name: &str) -> Temp {
        let parent =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/determinism-verify-tests");
        fs::create_dir_all(&parent).unwrap();
        let root = loop {
            let candidate = parent.join(format!(
                "{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create artifact: {error}"),
            }
        };
        fs::create_dir_all(root.join("trees/s")).unwrap();
        fs::write(root.join("trees/s/file"), b"bytes").unwrap();
        let manifest = scan_subject_trees(&root.join("trees")).unwrap();
        fs::write(root.join("MANIFEST"), manifest.serialize()).unwrap();
        fs::create_dir_all(root.join("evidence/subjects/s")).unwrap();
        fs::write(root.join("evidence/run.json"), RUN).unwrap();
        fs::write(root.join("evidence/subjects/s/envelope.json"), ENVELOPE).unwrap();
        fs::write(root.join("evidence/subjects/s/stdout.bin"), []).unwrap();
        fs::write(root.join("evidence/subjects/s/stderr.bin"), []).unwrap();
        Temp(root)
    }
    #[test]
    fn artifact_skips_same_pid_residue() {
        let pid = std::process::id();
        let parent =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/determinism-verify-tests");
        fs::create_dir_all(&parent).unwrap();
        let start = NEXT.load(Ordering::Relaxed);
        let mut blocked = Vec::new();
        // Plant trees/poison so adopt is visible beside the helper's own "s".
        let budget = std::thread::available_parallelism()
            .map(|n| n.get() as u64)
            .unwrap_or(4);
        let mut last = start;
        for step in 0..4096u64 {
            last = start + step;
            let path = parent.join(format!("residue-{pid}-{last}"));
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("plant residue: {error}"),
            }
            fs::create_dir_all(path.join("trees/poison")).unwrap();
            fs::write(path.join("trees/poison/file"), b"adopt-me").unwrap();
            blocked.push(Temp(path));
            if last > NEXT.load(Ordering::Relaxed) + budget {
                break;
            }
            assert!(
                step + 1 < 4096,
                "planting hit cap without covering counter+sibling"
            );
        }
        let got = artifact("residue");
        let name = got.0.file_name().unwrap().to_string_lossy();
        let drawn: u64 = name.rsplit('-').next().unwrap().parse().unwrap();
        assert_eq!(
            drawn,
            last + 1,
            "artifact() must draw last-planted+1; got {name}"
        );
        let mut names: Vec<_> = fs::read_dir(got.0.join("trees"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [std::ffi::OsString::from("s")],
            "adopted a foreign subject tree"
        );
        assert_eq!(verify(&got.0, TARGET, false), Ok(()));
        for planted in &blocked {
            assert_eq!(
                fs::read(planted.0.join("trees/poison/file")).unwrap(),
                b"adopt-me",
                "artifact() must leave same-pid residue untouched"
            );
        }
    }
    fn edit(root: &Path, relative: &str, change: impl FnOnce(&mut Value)) {
        let path = root.join(relative);
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        change(&mut value);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn omission(path: &str, size: u64) -> Value {
        json!({"path": path, "size": size, "reason": "file-limit"})
    }
    fn set_omissions(root: &Path, value: Value) {
        edit(root, "evidence/subjects/s/envelope.json", |envelope| {
            envelope["retention"]["omitted"] = value;
        });
    }
    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }
    enum Bad {
        Remove(&'static str),
        Write(&'static str, &'static [u8]),
        Directory(&'static str),
        Envelope(&'static str, Value),
        Omissions(Value),
    }
    fn reject(bad: Bad) {
        let temp = artifact("bad");
        match bad {
            Bad::Remove(path) => fs::remove_file(temp.0.join(path)).unwrap(),
            Bad::Write(path, bytes) => fs::write(temp.0.join(path), bytes).unwrap(),
            Bad::Directory(path) => {
                let path = temp.0.join(path);
                if path.exists() {
                    fs::remove_file(&path).unwrap();
                }
                fs::create_dir(path).unwrap();
            }
            Bad::Envelope(key, value) => {
                edit(&temp.0, "evidence/subjects/s/envelope.json", |envelope| {
                    envelope[key] = value
                })
            }
            Bad::Omissions(value) => set_omissions(&temp.0, value),
        }
        assert_eq!(command(&temp.0, TARGET, false), 2);
    }

    #[test]
    fn complete_target_image_and_argv_contract() {
        let temp = artifact("green");
        assert_eq!(verify(&temp.0, TARGET, false), Ok(()));
        assert_eq!(
            verify(&temp.0, "wrong", false).unwrap_err(),
            "runner target mismatch: expected wrong, found test-target"
        );
        assert_eq!(command(&temp.0, "wrong", false), 2);
        assert!(
            verify(&temp.0, TARGET, true)
                .unwrap_err()
                .contains("unavailable")
        );
        assert_eq!(command(&temp.0, TARGET, true), 2);
        edit(&temp.0, "evidence/run.json", |run| {
            run["runner"]["image_os"] = json!("macos");
            run["runner"]["image_version"] = json!("1");
        });
        assert_eq!(verify(&temp.0, TARGET, true), Ok(()));
        for args in [
            vec![],
            vec!["x"],
            vec!["x", "--target"],
            vec!["x", TARGET, "--target"],
            vec!["x", "--bad", TARGET],
            vec!["x", "--target", TARGET, "--bad"],
            vec!["", "--target", TARGET],
            vec!["-x", "--target", TARGET],
            vec!["x", "--target", ""],
            vec!["x", "--target", "-target"],
        ] {
            assert_eq!(from_args(&strings(&args)), None);
        }
        let dir = temp.0.to_string_lossy().into_owned();
        assert_eq!(from_args(&strings(&[&dir, "--target", TARGET])), Some(0));
        assert_eq!(
            from_args(&strings(&[&dir, "--target", TARGET, "--require-image"])),
            Some(0)
        );
    }

    #[test]
    fn retained_and_omitted_records_reconcile_exactly() {
        let missing = artifact("missing-retained");
        fs::remove_dir_all(missing.0.join("trees/s")).unwrap();
        assert_eq!(command(&missing.0, TARGET, false), 2);
        set_omissions(&missing.0, json!([omission("s/file", 5)]));
        assert_eq!(verify(&missing.0, TARGET, false), Ok(()));
        let corrupt = artifact("corrupt");
        fs::write(corrupt.0.join("trees/s/file"), b"bytez").unwrap();
        assert_eq!(command(&corrupt.0, TARGET, false), 2);
    }

    #[test]
    fn malformed_and_incomplete_artifacts_are_rejected() {
        for bad in [
            Bad::Remove("MANIFEST"),
            Bad::Write("MANIFEST", b"bad\n"),
            Bad::Remove("evidence/run.json"),
            Bad::Write("evidence/run.json", b"{"),
            Bad::Remove("evidence/subjects/s/stdout.bin"),
            Bad::Directory("evidence/subjects/s/envelope.json"),
            Bad::Write("trees/s/extra", b"x"),
            Bad::Directory("evidence/subjects/extra"),
            Bad::Omissions(json!([omission("s/file", 5), omission("s/file", 5)])),
            Bad::Omissions(json!([omission("s/unknown", 1)])),
            Bad::Omissions(json!([omission("s/file", 5)])),
            Bad::Write("evidence/subjects/s/envelope.json", b"{"),
            Bad::Envelope("schema", json!(2)),
            Bad::Envelope("subject", json!("other")),
            Bad::Envelope("experiment", json!("repeat")),
            Bad::Omissions(json!([{"path": "s/file"}])),
        ] {
            reject(bad);
        }
    }
}
