use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

// Bootstrap registries. S7 replaces both with manifest-derived classification (S0 D10).
const OWNED_FMT_PACKAGES: &[&str] = &["xtask"];
const FMT_EXCLUDED_PACKAGES: &[&str] = &["generated-style-fmt"];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = match args.as_slice() {
        [command, flag] if command == "ci" && flag == "--no-budget" => run_ci(None),
        [command, flag, base]
            if command == "ci"
                && flag == "--base"
                && !base.is_empty()
                && !base.starts_with('-') =>
        {
            run_ci(Some(base))
        }
        [command] if command == "test" => run_test(),
        _ => {
            usage();
            2
        }
    };
    ExitCode::from(code)
}

fn usage() {
    eprintln!("usage: cargo xtask ci (--base <revision> | --no-budget)\n       cargo xtask test");
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run_ci(base: Option<&str>) -> u8 {
    if let Err(error) = check_toolchain() {
        eprintln!("toolchain: FAIL: {error}");
        eprintln!("summary: FAIL (toolchain)");
        return 1;
    }
    println!("toolchain: PASS");

    let checks = [
        ("fmt", run_fmt()),
        (
            "clippy",
            run_cargo(&[
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings",
            ]),
        ),
        (
            "test",
            run_cargo(&["test", "--workspace", "--all-features"]),
        ),
        ("doc", run_doc()),
        ("whitespace", check_tracked_whitespace()),
    ];
    for (name, passed) in checks {
        println!("{name}: {}", if passed { "PASS" } else { "FAIL" });
    }
    if let Some(base) = base {
        println!(
            "budget: SKIPPED — not implemented until S0-T4 (#89); --base {base:?} recorded but unused"
        );
    }
    let failed: Vec<_> = checks
        .iter()
        .filter_map(|(name, passed)| (!passed).then_some(*name))
        .collect();
    if failed.is_empty() {
        println!("summary: PASS");
        0
    } else {
        eprintln!("summary: FAIL ({})", failed.join(", "));
        1
    }
}

fn check_toolchain() -> Result<(), String> {
    let text = fs::read_to_string(root().join("rust-toolchain.toml"))
        .map_err(|error| format!("cannot read rust-toolchain.toml: {error}"))?;
    let channel = parse_channel(&text)?;
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .map_err(|error| format!("cannot run rustc --version: {error}"))?;
    if !output.status.success() {
        return Err(format!("rustc --version exited with {}", output.status));
    }
    compare_rustc_version(channel, &String::from_utf8_lossy(&output.stdout))
}

fn parse_channel(text: &str) -> Result<&str, String> {
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "channel" {
            continue;
        }
        let value = value.trim();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            let channel = &value[1..value.len() - 1];
            if !channel.is_empty() {
                return Ok(channel);
            }
        }
        return Err("malformed channel in rust-toolchain.toml".into());
    }
    Err("missing channel in rust-toolchain.toml".into())
}

fn compare_rustc_version(expected: &str, output: &str) -> Result<(), String> {
    let found = output
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "malformed rustc --version output".to_string())?;
    if found == expected {
        Ok(())
    } else {
        Err(format!("expected rustc {expected}, found rustc {found}"))
    }
}

fn run_fmt() -> bool {
    OWNED_FMT_PACKAGES.iter().all(|package| {
        debug_assert!(!FMT_EXCLUDED_PACKAGES.contains(package));
        run_cargo(&["fmt", "--check", "-p", package])
    })
}

fn run_test() -> u8 {
    if run_cargo(&["test", "--workspace", "--all-features"]) {
        0
    } else {
        1
    }
}

fn run_doc() -> bool {
    Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .current_dir(root())
        .env("RUSTDOCFLAGS", "-D warnings")
        .status()
        .is_ok_and(|status| status.success())
}

fn run_cargo(args: &[&str]) -> bool {
    Command::new("cargo")
        .args(args)
        .current_dir(root())
        .status()
        .is_ok_and(|status| status.success())
}

fn check_tracked_whitespace() -> bool {
    let output = match Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root())
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!("git ls-files failed with {}", output.status);
            return false;
        }
        Err(error) => {
            eprintln!("cannot run git ls-files: {error}");
            return false;
        }
    };
    let mut passed = true;
    for name in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let relative = String::from_utf8_lossy(name);
        let path = root().join(relative.as_ref());
        let Ok(bytes) = fs::read(&path) else {
            eprintln!("cannot read {}", relative);
            passed = false;
            continue;
        };
        for line in trailing_whitespace_lines(&bytes) {
            eprintln!("{}:{line}: trailing space or tab", relative);
            passed = false;
        }
    }
    passed
}

fn trailing_whitespace_lines(bytes: &[u8]) -> Vec<usize> {
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Vec::new();
    }
    bytes
        .split(|byte| *byte == b'\n')
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            matches!(line.last(), Some(b' ' | b'\t')).then_some(index + 1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn owned_format_gate_passes_while_generated_fixture_fails_directly() {
        assert!(run_fmt());
        assert!(!run_cargo(&["fmt", "--check", "-p", "generated-style-fmt"]));
    }

    #[test]
    fn format_registries_cover_every_crate_once() {
        let owned: BTreeSet<_> = OWNED_FMT_PACKAGES.iter().copied().collect();
        let excluded: BTreeSet<_> = FMT_EXCLUDED_PACKAGES.iter().copied().collect();
        assert!(owned.is_disjoint(&excluded));
        let mut manifests = Vec::new();
        find_manifests(&root().join("crates"), &mut manifests);
        let found: BTreeSet<_> = manifests.iter().map(|path| manifest_name(path)).collect();
        let classified: BTreeSet<_> = owned
            .union(&excluded)
            .map(|name| name.to_string())
            .collect();
        assert_eq!(manifests.len(), found.len());
        assert_eq!(found, classified);
    }

    fn find_manifests(directory: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                find_manifests(&path, found);
            } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
                found.push(path);
            }
        }
    }

    fn manifest_name(path: &Path) -> String {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .find_map(|line| line.trim().strip_prefix("name = \"")?.strip_suffix('"'))
            .unwrap()
            .to_string()
    }

    #[test]
    fn toolchain_parser_and_comparison_are_exact() {
        assert_eq!(
            parse_channel("[toolchain]\nchannel = \"1.97.1\""),
            Ok("1.97.1")
        );
        assert!(compare_rustc_version("1.97.1", "rustc 1.97.1 (hash date)").is_ok());
        assert!(compare_rustc_version("1.97.1", "rustc 1.97.0 (hash date)").is_err());
        assert!(compare_rustc_version("1.97.1", "not rustc").is_err());
        assert!(parse_channel("channel = 1.97.1").is_err());
        assert!(parse_channel("[toolchain]").is_err());
    }

    #[test]
    fn whitespace_detection_handles_text_and_binary() {
        assert_eq!(
            trailing_whitespace_lines(b"clean\ntext\n"),
            Vec::<usize>::new()
        );
        assert_eq!(trailing_whitespace_lines(b"space \ntab\t\n"), vec![1, 2]);
        assert_eq!(
            trailing_whitespace_lines(b"space \0\n"),
            Vec::<usize>::new()
        );
    }
}

fn red_run_scratch () { let unused = 1; }
