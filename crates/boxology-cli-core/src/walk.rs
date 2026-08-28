use boxology_manifest::RelativePath;
use boxology_workspace::FileEntry;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};
// Current dense block begins at BXW0061; 02-packages discovery and S5-T4 #326 PR1 allocate it.
type Rule = (&'static str, &'static str, &'static str);
const RULE_SOURCE: &str =
    "boxology-details/02-packages.md discovery walk; S5-T4 #326 PR1 task authority";
const ROOT_TEXT: &str = "workspace root must be a real directory containing a regular Cargo.toml";
const IO_TEXT: &str = "filesystem refused a directory, symlink, or manifest read";
const PATH_TEXT: &str = "walked name/path is not a valid RelativePath";
const ROOT: Rule = ("BXW0061", ROOT_TEXT, RULE_SOURCE);
const IO: Rule = ("BXW0062", IO_TEXT, RULE_SOURCE);
const PATH: Rule = ("BXW0063", PATH_TEXT, RULE_SOURCE);
const CARGO: &str = "Cargo.toml";
const MANIFEST: &str = "boxology.toml";
/// A payload-safe failure while materializing raw workspace filesystem inputs.
#[derive(Debug, Eq, PartialEq)]
pub struct WalkError(&'static str, PathBuf, &'static str);
impl WalkError {
    /// Returns the stable `BXW####` code.
    pub fn code(&self) -> &'static str {
        self.0
    }
    /// Returns the exact filesystem path at which the walk failed.
    pub fn path(&self) -> &Path {
        &self.1
    }
    /// Returns stable detail without an operating-system error payload.
    pub fn detail(&self) -> &'static str {
        self.2
    }
}
impl fmt::Display for WalkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {:?}: {}", self.0, self.1, self.2)
    }
}
impl std::error::Error for WalkError {}
/// Raw filesystem material for `boxology-workspace`.
#[derive(Debug, Eq, PartialEq)]
pub struct WalkedWorkspace(Vec<FileEntry>, Vec<(RelativePath, Vec<u8>)>);
impl WalkedWorkspace {
    /// Returns regular files and symlinks in bytewise logical-path order.
    pub fn files(&self) -> &[FileEntry] {
        &self.0
    }
    /// Returns exact-final-name `boxology.toml` files and bytes in path order.
    pub fn manifests(&self) -> &[(RelativePath, Vec<u8>)] {
        &self.1
    }
}
/// Walks `root` without following symlink entries. In a Git worktree the walk includes cached
/// paths and non-ignored untracked paths, so ignored dependency/build trees stay outside discovery
/// while tracked files remain visible. Without a containing Git worktree it falls back to the full
/// filesystem walk. Real `.git` and `target` directories are always pruned at every depth.
///
/// # Errors
///
/// Returns `BXW0061` unless the root is a real directory with a regular manifest, `BXW0062` for
/// a refused read, and `BXW0063` for an invalid logical path.
pub fn walk(root: &Path) -> Result<WalkedWorkspace, WalkError> {
    if !fs::symlink_metadata(root).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(failure(ROOT, root.to_owned()));
    }
    let cargo = root.join(CARGO);
    if !fs::symlink_metadata(&cargo).is_ok_and(|metadata| metadata.is_file()) {
        return Err(failure(ROOT, cargo));
    }
    let mut files = Vec::new();
    let mut manifests = Vec::new();
    if !git_marker(root) || !visit_git(root, &mut files, &mut manifests)? {
        visit(root, root, &mut files, &mut manifests)?;
    }
    files.sort_unstable_by(|left, right| left.path().cmp(right.path()));
    manifests.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    Ok(WalkedWorkspace(files, manifests))
}
fn git_marker(root: &Path) -> bool {
    root.ancestors()
        .map(|directory| directory.join(".git"))
        .any(|candidate| {
            fs::symlink_metadata(candidate)
                .is_ok_and(|metadata| metadata.is_dir() || metadata.is_file())
        })
}
fn visit_git(
    root: &Path,
    files: &mut Vec<FileEntry>,
    manifests: &mut Vec<(RelativePath, Vec<u8>)>,
) -> Result<bool, WalkError> {
    let output = match Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--deduplicate",
            "--exclude-standard",
            "--",
            ".",
        ])
        .current_dir(root)
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(false),
    };
    if !output.status.success() {
        return Ok(false);
    }
    if !output.stdout.is_empty() && output.stdout.last() != Some(&0) {
        return Err(failure(PATH, root.to_owned()));
    }
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
    {
        let spelling = std::str::from_utf8(raw).map_err(|_| failure(PATH, root.to_owned()))?;
        let logical = RelativePath::new(spelling).map_err(|_| failure(PATH, root.to_owned()))?;
        if pruned_git_path(root, &logical) {
            continue;
        }
        let physical = root.join(logical.as_str());
        let metadata = match fs::symlink_metadata(&physical) {
            Ok(metadata) => metadata,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || error.kind() == std::io::ErrorKind::NotADirectory =>
            {
                continue;
            }
            Err(_) => return Err(failure(IO, physical)),
        };
        materialize(&physical, logical, metadata.file_type(), files, manifests)?;
        // A listed directory is a nested repository/gitlink boundary, not a workspace file.
    }
    Ok(true)
}
fn pruned_git_path(root: &Path, logical: &RelativePath) -> bool {
    let mut physical = root.to_owned();
    logical.as_str().split('/').any(|component| {
        physical.push(component);
        (component == ".git" || component == "target")
            && fs::symlink_metadata(&physical).is_ok_and(|metadata| metadata.is_dir())
    })
}
fn visit(
    root: &Path,
    directory: &Path,
    files: &mut Vec<FileEntry>,
    manifests: &mut Vec<(RelativePath, Vec<u8>)>,
) -> Result<(), WalkError> {
    let entries = fs::read_dir(directory).map_err(|_| failure(IO, directory.to_owned()))?;
    for entry in entries {
        let entry = entry.map_err(|_| failure(IO, directory.to_owned()))?;
        if entry.file_name() == ".git" {
            continue;
        }
        let physical = entry.path();
        let logical = logical_path(root, &physical)?;
        let kind = entry
            .file_type()
            .map_err(|_| failure(IO, physical.clone()))?;
        if kind.is_dir() {
            if entry.file_name() == "target" {
                continue;
            }
            visit(root, &physical, files, manifests)?;
        } else {
            materialize(&physical, logical, kind, files, manifests)?;
        }
    }
    Ok(())
}
fn materialize(
    physical: &Path,
    logical: RelativePath,
    kind: fs::FileType,
    files: &mut Vec<FileEntry>,
    manifests: &mut Vec<(RelativePath, Vec<u8>)>,
) -> Result<(), WalkError> {
    if kind.is_symlink() {
        let target = fs::read_link(physical).map_err(|_| failure(IO, physical.to_owned()))?;
        let target = target
            .to_str()
            .ok_or_else(|| failure(PATH, physical.to_owned()))?;
        files.push(FileEntry::symlink(logical, target.to_owned()));
    } else if kind.is_file() {
        if physical.file_name().is_some_and(|name| name == MANIFEST) {
            let bytes = read_manifest(physical, |path| fs::read(path))?;
            manifests.push((logical.clone(), bytes));
        }
        files.push(FileEntry::file(logical));
    }
    Ok(())
}
fn read_manifest(
    path: &Path,
    reader: impl FnOnce(&Path) -> std::io::Result<Vec<u8>>,
) -> Result<Vec<u8>, WalkError> {
    reader(path).map_err(|_| failure(IO, path.to_owned()))
}
fn logical_path(root: &Path, physical: &Path) -> Result<RelativePath, WalkError> {
    let relative = physical
        .strip_prefix(root)
        .map_err(|_| failure(PATH, physical.to_owned()))?;
    let spelling = relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| failure(PATH, physical.to_owned()))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join("/");
    RelativePath::new(spelling).map_err(|_| failure(PATH, physical.to_owned()))
}
fn failure(rule: Rule, path: PathBuf) -> WalkError {
    WalkError(rule.0, path, rule.1)
}
#[cfg(test)]
mod tests {
    use super::{IO_TEXT, read_manifest};
    use std::{io, path::Path};
    #[test]
    fn refused_manifest_read_is_stable_and_payload_safe() {
        let path = Path::new("blocked/boxology.toml");
        let error = read_manifest(path, |_| {
            Err(io::Error::other("SECRET operating-system payload"))
        })
        .expect_err("injected refusal must map through the production helper");
        assert_eq!(error.code(), "BXW0062");
        assert_eq!(error.path(), path);
        assert_eq!(error.detail(), IO_TEXT);
        assert_eq!(error.to_string(), format!("BXW0062 {path:?}: {IO_TEXT}"));
        assert!(!error.to_string().contains("SECRET"));
    }
}
