use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub(crate) const VERSION: &str = "0.20.2";
const INSTALL: &str = "cargo install cargo-deny --version 0.20.2 --locked";
const CHECKS: &[&str] = &["deny", "check", "bans", "licenses", "sources"];

pub fn run(root: &Path) -> u8 {
    if let Err(error) = require_version(root) {
        eprintln!("{error}");
        return 1;
    }

    let status = Command::new("cargo")
        .args(CHECKS)
        .current_dir(root)
        .status();
    if !status.is_ok_and(|status| status.success()) {
        return 1;
    }

    match negative_controls(root) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("cargo-deny negative controls failed: {error}");
            1
        }
    }
}

pub fn require_version(root: &Path) -> Result<(), String> {
    let version = Command::new("cargo")
        .args(["deny", "--version"])
        .current_dir(root)
        .output();
    if !version.is_ok_and(|output| output.status.success() && exact_version(&output.stdout)) {
        return Err(format!(
            "cargo-deny: expected exactly {VERSION}; install it with: {INSTALL}"
        ));
    }
    Ok(())
}

fn exact_version(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    bytes == format!("cargo-deny {VERSION}").as_bytes()
}

fn negative_controls(root: &Path) -> Result<(), String> {
    let fixtures = root
        .join("target")
        .join(format!("xtask-deny-fixtures-{}", std::process::id()));
    if fixtures.exists() {
        fs::remove_dir_all(&fixtures).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&fixtures).map_err(|error| error.to_string())?;
    let result = run_negative_controls(root, &fixtures);
    let cleanup = fs::remove_dir_all(&fixtures).map_err(|error| error.to_string());
    result.and(cleanup)
}

fn run_negative_controls(root: &Path, fixtures: &Path) -> Result<(), String> {
    let license = fixtures.join("license");
    package(&license, "root", "0.1.0", None)?;
    package(
        &license.join("bad"),
        "bad-license",
        "0.1.0",
        Some("GPL-3.0-only"),
    )?;
    write(
        license.join("Cargo.toml"),
        "[package]\nname='root'\nversion='0.1.0'\nedition='2024'\npublish=false\n\
         [dependencies]\nbad-license={path='bad'}\n[workspace]\nexclude=['bad']\n",
    )?;
    expect_exit(root, &license, "licenses", 4)?;

    let bans = fixtures.join("bans");
    package(&bans, "root", "0.1.0", None)?;
    package(&bans.join("one"), "duplicate", "1.0.0", Some("MIT"))?;
    package(&bans.join("two"), "duplicate", "2.0.0", Some("MIT"))?;
    write(
        bans.join("Cargo.toml"),
        "[package]\nname='root'\nversion='0.1.0'\nedition='2024'\npublish=false\n\
         [dependencies]\none={package='duplicate',version='1.0.0',path='one'}\n\
         two={package='duplicate',version='2.0.0',path='two'}\n\
         [workspace]\nexclude=['one','two']\n",
    )?;
    expect_exit(root, &bans, "bans", 2)?;

    let wildcard = fixtures.join("wildcard");
    package(&wildcard, "root", "0.1.0", None)?;
    package(&wildcard.join("bad"), "wildcard", "1.0.0", Some("MIT"))?;
    write(
        wildcard.join("Cargo.toml"),
        "[package]\nname='root'\nversion='0.1.0'\nedition='2024'\npublish=false\n\
         [dependencies]\nwildcard={version='*',path='bad'}\n[workspace]\nexclude=['bad']\n",
    )?;
    expect_exit(root, &wildcard, "bans", 2)?;

    let sources = fixtures.join("sources");
    package(&sources, "root", "0.1.0", None)?;
    let repository = sources.join("git dependency '100%#?é");
    package(&repository, "git-dependency", "0.1.0", Some("MIT"))?;
    git(&repository, &["init", "--quiet"])?;
    git(&repository, &["add", "."])?;
    git(
        &repository,
        &[
            "-c",
            "user.name=Boxology",
            "-c",
            "user.email=fixture@invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    )?;
    write(
        sources.join("Cargo.toml"),
        &format!(
            "[package]\nname='root'\nversion='0.1.0'\nedition='2024'\npublish=false\n\
             [dependencies]\ngit-dependency={{git='file://{}'}}\n[workspace]\n",
            percent_encode_path(
                fs::canonicalize(&repository)
                    .map_err(|error| error.to_string())?
                    .to_str()
                    .ok_or("fixture path is not UTF-8")?
            )
        ),
    )?;
    expect_exit(root, &sources, "sources", 8)
}

fn package(
    directory: &Path,
    name: &str,
    version: &str,
    license: Option<&str>,
) -> Result<(), String> {
    fs::create_dir_all(directory.join("src")).map_err(|error| error.to_string())?;
    let license = license.map_or(String::new(), |value| format!("license='{value}'\n"));
    write(
        directory.join("Cargo.toml"),
        &format!("[package]\nname='{name}'\nversion='{version}'\nedition='2024'\n{license}"),
    )?;
    write(directory.join("src/lib.rs"), "")
}

fn write(path: PathBuf, contents: &str) -> Result<(), String> {
    fs::write(&path, contents).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn git(directory: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        command_error(output, "git")
    }
}

fn expect_exit(root: &Path, fixture: &Path, check: &str, expected: i32) -> Result<(), String> {
    let output = Command::new("cargo")
        .args(["deny", "--manifest-path"])
        .arg(fixture.join("Cargo.toml"))
        .arg("--config")
        .arg(root.join("deny.toml"))
        .args(["check", check])
        .env("CARGO_HOME", fixture.join(".cargo-home"))
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.code() == Some(expected) {
        Ok(())
    } else {
        Err(format!(
            "{check} expected exit {expected}, got {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}

pub(crate) fn command_error<T>(output: Output, label: &str) -> Result<T, String> {
    Err(format!(
        "{label} exited with {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_invocation_are_exact() {
        assert!(exact_version(b"cargo-deny 0.20.2\n"));
        assert!(exact_version(b"cargo-deny 0.20.2\r\n"));
        assert!(!exact_version(b"cargo-deny 0.20.1\n"));
        assert!(!exact_version(b"cargo-deny 0.20.2 extra\n"));
        assert_eq!(CHECKS, ["deny", "check", "bans", "licenses", "sources"]);
        assert!(INSTALL.ends_with("--version 0.20.2 --locked"));
    }

    #[test]
    fn workflow_pin_matches_xtask() {
        let workflow = include_str!("../../../.github/workflows/ci.yml");
        let pins: Vec<_> = workflow
            .lines()
            .filter_map(|line| line.trim().strip_prefix("CARGO_DENY_VERSION: "))
            .collect();
        assert_eq!(pins, [format!("\"{VERSION}\"")]);
    }

    #[test]
    fn file_url_paths_are_percent_encoded() {
        assert_eq!(
            percent_encode_path("/tmp/a b/'x\"%#?é"),
            "/tmp/a%20b/%27x%22%25%23%3F%C3%A9"
        );
    }
}
