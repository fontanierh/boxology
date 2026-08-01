#![forbid(unsafe_code)]

mod write;

use boxology_init::{InitRequest, initialize};
use std::{
    env, fs,
    io::{self, Write},
    path::Path,
    process::ExitCode,
};

#[rustfmt::skip]
const CODES: [(&str, &str, &str); 5] = [
    ("BXI0005", "target directory", "target must be an existing readable directory"),
    ("BXI0006", "target entries", "target must contain no entry other than `.git`"),
    ("BXI0007", "generated project", "a target bearing the `boxology.toml` completion sentinel has already been initialized; re-running is refused"),
    ("BXI0008", "staged write", "an interrupted write leaves no tree bearing the completion sentinel; the partial tree is reported for manual cleanup"),
    ("BXI0009", "invocation", "all parameters must be given as explicit flags: `--name`, `--dependency-source`, `--target`"),
];
const D2: &str = "specs/s6-installer-and-generated-project.md D2";
const D1: &str = "specs/s6-installer-and-generated-project.md D1";
const USAGE: &str =
    "usage: boxology-init --name <project-name> --dependency-source <path> --target <directory>";

struct Args {
    name: String,
    dependency_source: String,
    target: String,
}

#[derive(Debug)]
enum Class {
    Ready,
    Occupied(Vec<String>),
    Initialized,
}

fn main() -> ExitCode {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    match env::args_os()
        .skip(1)
        .map(|arg| arg.into_string())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(args) => ExitCode::from(run(&args, &mut stdout, &mut stderr)),
        Err(_) => ExitCode::from(usage_failure(&mut stderr)),
    }
}

fn run(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let args = match parse(args) {
        Ok(args) => args,
        Err(()) => return usage_failure(stderr),
    };
    let request = match InitRequest::new(&args.name, &args.dependency_source) {
        Ok(request) => request,
        Err(diagnostics) => {
            let _ = writeln!(stderr, "{diagnostics}");
            return 1;
        }
    };
    if let Err(code) = validate_target(Path::new(&args.target), stderr) {
        return code;
    }
    let tree = match initialize(&request) {
        Ok(tree) => tree,
        Err(diagnostics) => {
            let _ = writeln!(stderr, "{diagnostics}");
            return 1;
        }
    };
    let files: Vec<_> = tree
        .files()
        .iter()
        .map(|file| (file.path(), file.bytes()))
        .collect();
    if let Err(failure) = write::write_tree(Path::new(&args.target), &files) {
        let _ = writeln!(stderr, "{} {}", fixed(3, &args.target), failure.path);
        return 1;
    }
    let _ = writeln!(stdout, "initialized {}", request.project_name());
    0
}

fn parse(args: &[String]) -> Result<Args, ()> {
    let mut name = None;
    let mut dependency_source = None;
    let mut target = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        index += 1;
        let slot = match flag {
            "--name" => &mut name,
            "--dependency-source" => &mut dependency_source,
            "--target" => &mut target,
            _ => return Err(()),
        };
        if slot.is_some() {
            return Err(());
        }
        *slot = Some(args.get(index).ok_or(())?.clone());
        index += 1;
    }
    Ok(Args {
        name: name.ok_or(())?,
        dependency_source: dependency_source.ok_or(())?,
        target: target.ok_or(())?,
    })
}

fn validate_target(target: &Path, stderr: &mut dyn Write) -> Result<(), u8> {
    let path = target.display().to_string();
    if !fs::metadata(target).is_ok_and(|metadata| metadata.is_dir()) {
        let _ = writeln!(stderr, "{}", fixed(0, &path));
        return Err(1);
    }
    let mut names = Vec::new();
    let Ok(entries) = fs::read_dir(target) else {
        let _ = writeln!(stderr, "{}", fixed(0, &path));
        return Err(1);
    };
    for entry in entries {
        let Ok(entry) = entry else {
            let _ = writeln!(stderr, "{}", fixed(0, &path));
            return Err(1);
        };
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    match classify(&names) {
        Class::Ready => Ok(()),
        Class::Initialized => {
            let _ = writeln!(stderr, "{}", fixed(2, &path));
            Err(1)
        }
        Class::Occupied(entries) => {
            let _ = writeln!(stderr, "{} entries={entries:?}", fixed(1, &path));
            Err(1)
        }
    }
}

fn classify(names: &[String]) -> Class {
    if names.iter().any(|name| name == "boxology.toml") {
        return Class::Initialized;
    }
    let occupied: Vec<_> = names
        .iter()
        .filter(|name| name.as_str() != ".git")
        .cloned()
        .collect();
    if occupied.is_empty() {
        Class::Ready
    } else {
        Class::Occupied(occupied)
    }
}

fn fixed(index: usize, path: &str) -> String {
    let (code, offending, rule) = CODES[index];
    let source = if index == 4 { D1 } else { D2 };
    format!("{code} {path}:1:1-1:1 offending={offending:?} rule={rule:?} source={source:?}")
}

fn usage_failure(stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(stderr, "{}", fixed(4, "<argv>"));
    let _ = writeln!(stderr, "{USAGE}");
    2
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;

    #[test]
    fn cli_code_catalog_matches_golden_suffix() {
        let expected: Vec<_> = include_str!("../test/bxi.golden").lines().skip(4).collect();
        let actual: Vec<_> = (0..5).map(|index| {
            let path = if index == 4 { "<argv>" } else { "<target>" };
            let mut line = fixed(index, path);
            if index == 1 { line.push_str(" entries=[]"); }
            line
        }).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn classify_isolates_git_exemption_and_sentinel() {
        assert!(matches!(classify(&[".git".into()]), Class::Ready));
        match classify(&[".DS_Store".into(), ".git".into()]) {
            Class::Occupied(entries) => assert_eq!(entries, [".DS_Store"]),
            other => panic!("{other:?}"),
        }
        assert!(matches!(classify(&["boxology.toml".into()]), Class::Initialized));
    }
}
