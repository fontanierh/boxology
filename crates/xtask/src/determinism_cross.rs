use crate::determinism::{ManifestRecord, byte_diff, manifest_side};
use crate::determinism_compare::{Comparison, evaluate, hex};
use crate::determinism_run::{Failure, Subject, manifest_with, report, validate_registry};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

const LINUX: &[u8] = b"os=linux\narch=aarch64\n";
#[cfg(test)]
const LINUX_X86_64: &[u8] = b"os=linux\narch=x86_64\n";
const MACOS: &[u8] = b"os=macos\narch=aarch64\n";

fn fixed(out: &Path, value: &str) -> (PathBuf, Vec<OsString>) {
    (
        "/bin/sh".into(),
        vec![
            "-c".into(),
            "printf %s \"$3\" > \"$1/$2\"".into(),
            "boxology-meta".into(),
            out.into(),
            "platform.txt".into(),
            value.into(),
        ],
    )
}

fn platform_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
    fixed(
        out,
        &format!(
            "os={}\narch={}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    )
}

pub(crate) fn fixture() -> Subject {
    Subject {
        name: "meta-platform",
        prepare: None,
        argv: platform_argv,
    }
}

pub(crate) fn manifest(workspace: &Path, out: &Path) -> u8 {
    let subjects = [fixture()];
    if let Err(error) = validate_registry(&subjects) {
        return report(Failure::Infra(error), None);
    }
    manifest_with(workspace, out, &subjects)
}

fn record(bytes: &[u8]) -> ManifestRecord {
    ManifestRecord {
        path: "meta-platform/platform.txt".into(),
        size: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}

fn expected_finding() -> String {
    let (left, right) = (record(LINUX), record(MACOS));
    let difference = byte_diff(LINUX, MACOS).expect("platform fixtures differ");
    format!(
        "cross-platform mismatch subject=meta-platform experiment=baseline first=meta-platform/platform.txt left={} right={} offset={} left_window={} right_window={} left_len={} right_len={} differing=1",
        manifest_side(Some(&left)),
        manifest_side(Some(&right)),
        difference.first_offset,
        hex(&difference.left_window),
        hex(&difference.right_window),
        difference.left_len,
        difference.right_len,
    )
}

fn canonical(root: &Path, platform: &str) -> Result<PathBuf, String> {
    fs::canonicalize(root)
        .map_err(|error| format!("canonicalize {platform} root {}: {error}", root.display()))
}

fn gate(linux: &Path, macos: &Path) -> u8 {
    let linux = match canonical(linux, "linux") {
        Ok(path) => path,
        Err(error) => {
            eprintln!("determinism-meta-cross: ERROR: {error}");
            return 2;
        }
    };
    let macos = match canonical(macos, "macos") {
        Ok(path) => path,
        Err(error) => {
            eprintln!("determinism-meta-cross: ERROR: {error}");
            return 2;
        }
    };
    match evaluate(&linux, &macos) {
        Ok(Comparison::Match { .. }) => {
            eprintln!("determinism-meta-cross: FAIL: comparator unexpectedly passed");
            1
        }
        Ok(Comparison::Finding(message)) | Err(Failure::Finding(message)) => {
            let expected = expected_finding();
            if message == expected {
                eprintln!("determinism: FINDING: {message}");
                println!("determinism-meta-cross: PASS");
                0
            } else {
                eprintln!("determinism-meta-cross: FAIL: comparator finding did not match");
                eprintln!("determinism-meta-cross: expected: determinism: FINDING: {expected}");
                eprintln!("determinism-meta-cross: actual: determinism: FINDING: {message}");
                1
            }
        }
        Err(Failure::Infra(message)) => {
            eprintln!("determinism-meta-cross: ERROR: comparator exited with exit status: 2");
            eprintln!("determinism-meta-cross: actual stderr:");
            eprintln!("determinism: ERROR: {message}");
            2
        }
    }
}

pub(crate) fn from_args(args: &[String]) -> Option<u8> {
    match args {
        [linux, macos]
            if !linux.is_empty()
                && !linux.starts_with('-')
                && !macos.is_empty()
                && !macos.starts_with('-') =>
        {
            Some(gate(Path::new(linux), Path::new(macos)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::determinism_run::{create_run_root, finish_local, protocol};
    use crate::determinism_verify::verify;
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new(name: &str) -> Self {
            let parent = workspace().join("target/determinism-cross-tests");
            fs::create_dir_all(&parent).unwrap();
            let path = loop {
                let candidate = parent.join(format!(
                    "{name}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match fs::create_dir(&candidate) {
                    Ok(()) => break candidate,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create temp: {error}"),
                }
            };
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn temp_skips_same_pid_residue() {
        let pid = std::process::id();
        let parent = workspace().join("target/determinism-cross-tests");
        fs::create_dir_all(&parent).unwrap();
        let start = NEXT.load(Ordering::Relaxed);
        let mut blocked = Vec::new();
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
            fs::write(path.join("stale"), b"adopt-me").unwrap();
            blocked.push(Temp(path));
            if last > NEXT.load(Ordering::Relaxed) + budget {
                break;
            }
            assert!(
                step + 1 < 4096,
                "planting hit cap without covering counter+sibling"
            );
        }
        let got = Temp::new("residue");
        let name = got.0.file_name().unwrap().to_string_lossy();
        let drawn: u64 = name.rsplit('-').next().unwrap().parse().unwrap();
        assert_eq!(
            drawn,
            last + 1,
            "Temp::new must draw last-planted+1; got {name}"
        );
        for planted in &blocked {
            assert_eq!(
                fs::read(planted.0.join("stale")).unwrap(),
                b"adopt-me",
                "Temp::new must leave same-pid residue untouched"
            );
        }
    }

    fn workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn host() -> (&'static [u8], &'static str) {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => (LINUX_X86_64, "x86_64-unknown-linux-gnu"),
            ("linux", "aarch64") => (LINUX, "aarch64-unknown-linux-gnu"),
            ("macos", "aarch64") => (MACOS, "aarch64-apple-darwin"),
            pair => panic!("unsupported fixture host: {pair:?}"),
        }
    }

    fn linux_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
        fixed(out, std::str::from_utf8(LINUX).unwrap())
    }

    fn macos_argv(out: &Path) -> (PathBuf, Vec<OsString>) {
        fixed(out, std::str::from_utf8(MACOS).unwrap())
    }

    fn variants(temp: &Temp) -> (PathBuf, PathBuf) {
        let (linux, macos) = (temp.0.join("linux"), temp.0.join("macos"));
        for (out, argv) in [
            (&linux, linux_argv as fn(&Path) -> (PathBuf, Vec<OsString>)),
            (&macos, macos_argv),
        ] {
            assert_eq!(
                manifest_with(
                    &workspace(),
                    out,
                    &[Subject {
                        name: "meta-platform",
                        prepare: None,
                        argv,
                    }]
                ),
                0
            );
        }
        (linux, macos)
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn host_fixture_passes_protocol_publishes_and_verifies() {
        let (host_bytes, target) = host();
        let expected = format!(
            "os={}\narch={}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        assert_eq!(expected.as_bytes(), host_bytes);
        let workspace = workspace();
        let run = create_run_root(&workspace).unwrap();
        let result = protocol(&workspace, &run, &[fixture()]);
        assert!(result.as_ref().unwrap().is_empty());
        assert_eq!(finish_local(&run, result), 0);
        let temp = Temp::new("host");
        let published = temp.0.join("published");
        assert_eq!(manifest(&workspace, &published), 0);
        assert_eq!(
            fs::read(published.join("trees/meta-platform/platform.txt")).unwrap(),
            host_bytes
        );
        assert_eq!(verify(&published, target, false), Ok(()));
    }

    #[test]
    fn expected_finding_is_frozen() {
        assert_eq!(
            expected_finding(),
            "cross-platform mismatch subject=meta-platform experiment=baseline first=meta-platform/platform.txt left=22:223ad4017c303e41 right=22:dcd00cef10fb8eac offset=3 left_window=6c696e75780a617263683d6161726368 right_window=6d61636f730a617263683d6161726368 left_len=22 right_len=22 differing=1"
        );
    }

    #[test]
    fn argv_is_exact_and_fixed_order_gate_passes() {
        for args in [
            vec![],
            vec!["one"],
            vec!["a", "b", "c"],
            vec!["", "b"],
            vec!["a", ""],
            vec!["-a", "b"],
            vec!["a", "--b"],
        ] {
            assert_eq!(from_args(&strings(&args)), None);
        }
        let temp = Temp::new("argv");
        let (linux, macos) = variants(&temp);
        let (linux, macos) = (
            linux.to_string_lossy().into_owned(),
            macos.to_string_lossy().into_owned(),
        );
        assert_eq!(from_args(&[linux.clone(), macos.clone()]), Some(0));
        assert_eq!(from_args(&[macos, linux]), Some(1));
    }

    #[test]
    fn gate_rejects_success_missing_corrupt_and_malformed_inputs() {
        let temp = Temp::new("reject");
        let (linux, macos) = variants(&temp);
        assert_eq!(gate(&linux, &linux), 1);
        let retained = linux.join("trees/meta-platform/platform.txt");
        fs::remove_file(&retained).unwrap();
        assert_eq!(gate(&linux, &macos), 1);
        let mut corrupt = LINUX.to_vec();
        corrupt[0] = b'x';
        fs::write(&retained, corrupt).unwrap();
        assert_eq!(gate(&linux, &macos), 2);
        fs::write(&retained, LINUX).unwrap();
        fs::write(linux.join("MANIFEST"), b"bad\n").unwrap();
        assert_eq!(gate(&linux, &macos), 2);
    }
}
