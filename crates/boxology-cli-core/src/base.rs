//! Git-backed base-revision resolution and schema ingestion for `boxology check`.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use crate::{ExecuteError, GenerationPlan, PackageSchemas, execute::read_optional_file};
use boxology_manifest::RelativePath;
use boxology_workspace::{
    CargoManifestChange, DiffOwnership, FileEntry, Findings, Package, WorkspaceInputs,
};
use std::{
    collections::BTreeMap,
    fmt, fs, io,
    path::Path,
    process::{Command, Output},
};

type Rule = (&'static str, &'static str, &'static str);
const BASE_GIT_SOURCE: &str = "specs/s5-manifest-and-validation.md D6";
const BASE_REVISION_TEXT: &str = "the explicit base revision must resolve to a Git commit";
const BASE_SCHEMA_TEXT: &str = "a base-revision schema object must be readable as a Git blob";
const REVISION: Rule = ("BXW0091", BASE_REVISION_TEXT, BASE_GIT_SOURCE);
const BASE_SCHEMA: Rule = ("BXW0092", BASE_SCHEMA_TEXT, BASE_GIT_SOURCE);
const BASE_DISCOVERY_SOURCE: &str =
    "boxology-details/02-packages.md discovery walk; specs/s5-manifest-and-validation.md D6";
const BASE_LISTING_TEXT: &str =
    "the base revision's Git listings must parse as expected NUL-delimited output";
const BASE_BLOB_TEXT: &str = "a base-revision workspace object must be readable as a Git blob";
const BASE_DECLARATIONS_TEXT: &str =
    "the base revision's package declarations must form a discoverable workspace";
const CANDIDATE_CARGO_TEXT: &str =
    "a changed candidate Cargo manifest must be a readable regular file when present";
const BASE_LISTING: Rule = ("BXW0103", BASE_LISTING_TEXT, BASE_GIT_SOURCE);
const BASE_BLOB: Rule = ("BXW0104", BASE_BLOB_TEXT, BASE_GIT_SOURCE);
const BASE_DECLARATIONS: Rule = ("BXW0105", BASE_DECLARATIONS_TEXT, BASE_DISCOVERY_SOURCE);
const CANDIDATE_CARGO: Rule = ("BXW0106", CANDIDATE_CARGO_TEXT, BASE_GIT_SOURCE);

/// The Git executable could not be started for a base check.
#[derive(Debug, Eq, PartialEq)]
pub struct GitToolError;

impl fmt::Display for GitToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("git could not be executed")
    }
}

impl std::error::Error for GitToolError {}

/// A stable failure to resolve the explicit base or read one of its schema objects.
#[derive(Debug, Eq, PartialEq)]
pub struct BaseError {
    code: &'static str,
    location: String,
    detail: &'static str,
}

impl BaseError {
    /// Returns the stable `BXW####` code.
    pub fn code(&self) -> &'static str {
        self.code
    }
    /// Returns the stable non-secret location (`.git` or a plan-authoritative schema path).
    pub fn location(&self) -> &str {
        &self.location
    }
    /// Returns the stable rule detail.
    pub fn detail(&self) -> &'static str {
        self.detail
    }

    fn revision() -> Self {
        Self {
            code: REVISION.0,
            location: ".git".to_owned(),
            detail: REVISION.1,
        }
    }
    fn schema(plan: &GenerationPlan) -> Self {
        Self {
            code: BASE_SCHEMA.0,
            location: plan.schema_path().as_str().to_owned(),
            detail: BASE_SCHEMA.1,
        }
    }
    fn at(rule: Rule, location: impl Into<String>) -> Self {
        Self {
            code: rule.0,
            location: location.into(),
            detail: rule.1,
        }
    }
    fn listing() -> Self {
        Self::at(BASE_LISTING, ".git")
    }
    fn blob(path: &RelativePath) -> Self {
        Self::at(BASE_BLOB, path.as_str())
    }
    fn declarations() -> Self {
        Self::at(BASE_DECLARATIONS, ".git")
    }
    fn cargo(path: &RelativePath) -> Self {
        Self::at(CANDIDATE_CARGO, path.as_str())
    }
}

impl fmt::Display for BaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}: {}",
            self.code, self.location, self.detail
        )
    }
}

impl std::error::Error for BaseError {}

/// Failure while assembling current and base-revision package schemas.
#[derive(Debug)]
pub enum BaseSchemasError {
    /// The Git executable could not be started.
    Tool(GitToolError),
    /// Git could not resolve or read the requested base.
    Git(BaseError),
    /// The current checked-in schema did not satisfy the existing filesystem guard.
    Submitted(ExecuteError),
}

/// Failure while assembling base-revision ownership inputs.
#[derive(Debug)]
pub enum BaseInputsError {
    /// The Git executable could not be started.
    Tool(GitToolError),
    /// A coded Git listing, blob, or candidate-path failure.
    Data(BaseError),
    /// Base package discovery produced findings under a BXW0105 header.
    Declarations {
        /// The BXW0105 header diagnostic.
        header: BaseError,
        /// Deterministic base discovery findings.
        findings: Findings,
    },
}
impl fmt::Display for BaseInputsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tool(e) => e.fmt(f),
            Self::Data(e) => e.fmt(f),
            Self::Declarations { header, findings } => write!(f, "{header}\n{findings}"),
        }
    }
}
impl std::error::Error for BaseInputsError {}

/// Outcome of resolving the no-flag default base against the fixed v0 branch `main`.
#[derive(Debug, Eq, PartialEq)]
pub enum DefaultBase {
    /// Merge base of `HEAD` and `main`, as a single trimmed Git stdout line.
    Commit(String),
    /// `root` is not inside a Git repository.
    NoRepository,
    /// No merge base exists between `HEAD` and `main`.
    NoMergeBase,
}

/// One validated base commit, resolved once per check.
#[derive(Debug, Eq, PartialEq)]
pub struct ResolvedBase(String);
impl ResolvedBase {
    /// Returns the validated object id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Accepts exactly one trimmed 40- or 64-character ASCII-hex object id.
    ///
    /// # Errors
    /// Returns `BXW0091` when `oid` is not a trimmed full hex object id.
    pub fn from_oid(oid: String) -> Result<Self, BaseError> {
        let oid = oid.trim();
        valid_oid(oid)
            .then(|| Self(oid.to_owned()))
            .ok_or_else(BaseError::revision)
    }
}

/// Resolves the no-flag default base: merge base of `HEAD` with the fixed v0 branch `main`.
///
/// # Errors
/// Returns [`GitToolError`] when the Git executable cannot be started.
pub fn resolve_default_base(root: &Path) -> Result<DefaultBase, GitToolError> {
    let git_dir = git(root, &["rev-parse", "--git-dir"])
        .output()
        .map_err(|_| GitToolError)?;
    if !git_dir.status.success() {
        return Ok(DefaultBase::NoRepository);
    }
    let merge = git(root, &["merge-base", "HEAD", "main"])
        .output()
        .map_err(|_| GitToolError)?;
    if !merge.status.success() {
        return Ok(DefaultBase::NoMergeBase);
    }
    let oid = std::str::from_utf8(&merge.stdout)
        .map(str::trim)
        .unwrap_or("")
        .to_owned();
    Ok(DefaultBase::Commit(oid))
}

/// Resolves an explicit base revision to one validated commit object id.
///
/// # Errors
/// Returns `BXW0091` when `revision` is not a commit, or [`GitToolError`] when Git cannot start.
pub fn resolve_base(root: &Path, revision: &str) -> Result<ResolvedBase, BaseSchemasError> {
    Ok(ResolvedBase(resolve_commit(root, revision)?))
}

/// Assembles schema pairs for every current generation plan against one resolved base.
///
/// The current plan is the sole authority for schema paths. A path absent at the resolved base is
/// represented as `None`; an object present there but not readable as a blob is `BXW0092`.
///
/// # Errors
/// Returns `BXW0092` for an unreadable base object, or the existing `BXW0076` current-schema
/// filesystem error.
pub fn base_package_schemas(
    root: &Path,
    base: &ResolvedBase,
    plans: &[GenerationPlan],
) -> Result<Vec<PackageSchemas>, BaseSchemasError> {
    let oid = base.as_str();
    plans
        .iter()
        .map(|plan| {
            let submitted = read_optional_file(root, plan.schema_path())
                .map_err(BaseSchemasError::Submitted)?
                .ok_or_else(|| {
                    BaseSchemasError::Submitted(crate::execute::missing_schema(root, plan))
                })?;
            let base = read_schema(root, oid, plan)?;
            Ok(PackageSchemas::new(
                plan.package_id().clone(),
                base,
                submitted,
            ))
        })
        .collect()
}

fn resolve_commit(root: &Path, revision: &str) -> Result<String, BaseSchemasError> {
    let requested = format!("{revision}^{{commit}}");
    let output = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &requested],
    )
    .output()
    .map_err(|_| BaseSchemasError::Tool(GitToolError))?;
    if !output.status.success() {
        return Err(BaseSchemasError::Git(BaseError::revision()));
    }
    let oid = std::str::from_utf8(&output.stdout)
        .ok()
        .map(str::trim)
        .filter(|oid| valid_oid(oid))
        .ok_or_else(|| BaseSchemasError::Git(BaseError::revision()))?;
    Ok(oid.to_owned())
}

fn read_schema(
    root: &Path,
    oid: &str,
    plan: &GenerationPlan,
) -> Result<Option<Vec<u8>>, BaseSchemasError> {
    // Git's `<tree>:./<path>` spelling resolves from the command's working directory. Without
    // `./`, object paths are always repository-root-relative and nested managed workspaces break.
    let object = format!("{oid}:./{}", plan.schema_path().as_str());
    let listed = git(
        root,
        &[
            "ls-tree",
            "--name-only",
            "-z",
            oid,
            "--",
            plan.schema_path().as_str(),
        ],
    )
    .output()
    .map_err(|_| BaseSchemasError::Tool(GitToolError))?;
    if !listed.status.success() {
        return Err(BaseSchemasError::Git(BaseError::schema(plan)));
    }
    if listed.stdout.is_empty() {
        return Ok(None);
    }
    let mut expected = plan.schema_path().as_str().as_bytes().to_vec();
    expected.push(0);
    if listed.stdout != expected {
        return Err(BaseSchemasError::Git(BaseError::schema(plan)));
    }
    let exists = git(root, &["cat-file", "-e", &object])
        .output()
        .map_err(|_| BaseSchemasError::Tool(GitToolError))?;
    if !exists.status.success() {
        return Err(BaseSchemasError::Git(BaseError::schema(plan)));
    }
    let output = git(root, &["cat-file", "blob", &object])
        .output()
        .map_err(|_| BaseSchemasError::Tool(GitToolError))?;
    if !output.status.success() {
        return Err(BaseSchemasError::Git(BaseError::schema(plan)));
    }
    Ok(Some(output.stdout))
}

/// Base and submitted packages, changed paths, and tree object index for diff ownership.
#[derive(Debug)]
pub struct BaseDiffInputs {
    base_packages: Vec<Package>,
    submitted_packages: Vec<Package>,
    changed: Vec<RelativePath>,
    bootstrapping: bool,
    /// Mode/type class and object id for every validated path, including gitlinks.
    objects: BTreeMap<RelativePath, (TreeKind, String)>,
}
impl BaseDiffInputs {
    /// Returns submitted packages for a wholly absent-root bootstrap, and base packages otherwise.
    pub fn packages(&self) -> &[Package] {
        if self.bootstrapping {
            &self.submitted_packages
        } else {
            &self.base_packages
        }
    }
    /// Returns packages discovered from validated submitted declarations.
    pub fn submitted_packages(&self) -> &[Package] {
        &self.submitted_packages
    }
    /// Returns the sorted, deduplicated changed-path set.
    pub fn changed(&self) -> &[RelativePath] {
        &self.changed
    }
    /// Returns whether candidate declarations were selected for a wholly absent base root.
    pub fn is_bootstrapping(&self) -> bool {
        self.bootstrapping
    }
    /// Returns the sorted paths which exist as objects in the base tree.
    pub fn base_paths(&self) -> impl Iterator<Item = &RelativePath> {
        self.objects.keys()
    }
    /// Loads base/candidate bytes for accountable changed `Cargo.toml` paths under `ownership`.
    ///
    /// # Errors
    /// Returns `BXW0104`/`BXW0106` for unreadable base blobs or non-regular candidate paths.
    pub fn manifest_changes(
        &self,
        root: &Path,
        ownership: &DiffOwnership,
    ) -> Result<Vec<CargoManifestChange>, BaseInputsError> {
        if ownership
            .classifications()
            .iter()
            .all(|held| held.path().as_str() != "Cargo.lock")
        {
            return Ok(Vec::new());
        }
        ownership
            .classifications()
            .iter()
            .filter(|held| {
                ownership.classifications().iter().any(|source| {
                    source.derived_output().is_none() && source.package() == held.package()
                }) && held.path().as_str().rsplit('/').next() == Some("Cargo.toml")
            })
            .map(|held| {
                let path = held.path();
                let base = match self.objects.get(path) {
                    Some((TreeKind::Gitlink, _)) | None => None,
                    Some((_, oid)) => Some(read_blob(root, oid, path)?),
                };
                Ok(CargoManifestChange::new(
                    path.clone(),
                    base,
                    read_candidate_cargo(root, path)?,
                ))
            })
            .collect()
    }
}

/// Loads base packages, the working-tree changed set, and the full tree object index.
///
/// Untracked/ignored paths stay outside the Git changed set at V0 until staged or committed.
///
/// # Errors
/// Returns coded listing/blob/discovery failures or [`GitToolError`] when Git cannot start.
pub fn base_diff_inputs(
    root: &Path,
    base: &ResolvedBase,
) -> Result<BaseDiffInputs, BaseInputsError> {
    base_diff_inputs_inner(root, base, None)
}

/// Loads base and validated submitted declarations for exact-path ownership classification.
///
/// A changed path which exists in the base tree uses base declarations; an introduced path uses
/// submitted declarations. A wholly absent base root is additionally marked for bootstrap rules.
///
/// # Errors
/// Returns coded listing/blob/discovery failures or [`GitToolError`] when Git cannot start.
pub fn base_diff_inputs_with_candidate(
    root: &Path,
    base: &ResolvedBase,
    candidate_files: &[FileEntry],
    candidate_manifests: &[(RelativePath, Vec<u8>)],
) -> Result<BaseDiffInputs, BaseInputsError> {
    base_diff_inputs_inner(
        root,
        base,
        Some(CandidateDeclarations {
            files: candidate_files,
            manifests: candidate_manifests,
        }),
    )
}

struct CandidateDeclarations<'a> {
    files: &'a [FileEntry],
    manifests: &'a [(RelativePath, Vec<u8>)],
}

fn base_diff_inputs_inner(
    root: &Path,
    base: &ResolvedBase,
    candidate: Option<CandidateDeclarations<'_>>,
) -> Result<BaseDiffInputs, BaseInputsError> {
    let listed = git_ok(root, &["ls-tree", "-r", "-z", base.as_str(), "--", "."])?;
    let mut files = Vec::new();
    let mut manifests = Vec::new();
    let mut objects = BTreeMap::new();
    for entry in parse_nul(&listed.stdout, parse_tree)? {
        if objects
            .insert(entry.path.clone(), (entry.kind, entry.oid.clone()))
            .is_some()
        {
            return Err(data(BaseError::listing()));
        }
        // Gitlinks have no filesystem-walk counterpart at V0; retain them in the index only.
        match entry.kind {
            TreeKind::Gitlink => {}
            TreeKind::File | TreeKind::Executable => {
                if entry.path.as_str().rsplit('/').next() == Some("boxology.toml") {
                    manifests.push((
                        entry.path.clone(),
                        read_blob(root, &entry.oid, &entry.path)?,
                    ));
                }
                files.push(FileEntry::file(entry.path));
            }
            TreeKind::Symlink => {
                let target = String::from_utf8(read_blob(root, &entry.oid, &entry.path)?)
                    .map_err(|_| data(BaseError::blob(&entry.path)))?;
                files.push(FileEntry::symlink(entry.path, target));
            }
        }
    }
    let diffed = git_ok(
        root,
        &[
            "diff",
            "--name-only",
            "--relative",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            base.as_str(),
            "--",
            ".",
        ],
    )?;
    let mut changed = parse_nul(&diffed.stdout, parse_path)?;
    changed.sort();
    changed.dedup();
    let bootstrapping = candidate.is_some() && objects.is_empty();
    let inputs =
        WorkspaceInputs::new(files, manifests, "").map_err(|_| data(BaseError::listing()))?;
    let (base_packages, findings) = inputs.discover();
    if let Some(findings) = findings {
        return Err(BaseInputsError::Declarations {
            header: BaseError::declarations(),
            findings,
        });
    }
    let submitted_packages = match candidate {
        Some(candidate) => {
            let inputs =
                WorkspaceInputs::new(candidate.files.to_vec(), candidate.manifests.to_vec(), "")
                    .map_err(|_| data(BaseError::listing()))?;
            let (packages, findings) = inputs.discover();
            if let Some(findings) = findings {
                return Err(BaseInputsError::Declarations {
                    header: BaseError::declarations(),
                    findings,
                });
            }
            packages
        }
        None => Vec::new(),
    };
    Ok(BaseDiffInputs {
        base_packages,
        submitted_packages,
        changed,
        bootstrapping,
        objects,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TreeKind {
    File,
    Executable,
    Symlink,
    Gitlink,
}
struct TreeEntry {
    kind: TreeKind,
    oid: String,
    path: RelativePath,
}

fn data(error: BaseError) -> BaseInputsError {
    BaseInputsError::Data(error)
}
fn git_ok(root: &Path, args: &[&str]) -> Result<Output, BaseInputsError> {
    let output = git(root, args)
        .output()
        .map_err(|_| BaseInputsError::Tool(GitToolError))?;
    output
        .status
        .success()
        .then_some(output)
        .ok_or_else(|| data(BaseError::listing()))
}
fn read_blob(root: &Path, oid: &str, path: &RelativePath) -> Result<Vec<u8>, BaseInputsError> {
    let output = git(root, &["cat-file", "blob", oid])
        .output()
        .map_err(|_| BaseInputsError::Tool(GitToolError))?;
    output
        .status
        .success()
        .then_some(output.stdout)
        .ok_or_else(|| data(BaseError::blob(path)))
}
fn read_candidate_cargo(
    root: &Path,
    path: &RelativePath,
) -> Result<Option<Vec<u8>>, BaseInputsError> {
    let at = root.join(path.as_str());
    match fs::symlink_metadata(&at) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Ok(meta) if meta.file_type().is_file() => fs::read(&at)
            .map(Some)
            .map_err(|_| data(BaseError::cargo(path))),
        _ => Err(data(BaseError::cargo(path))),
    }
}
fn parse_nul<T>(
    stdout: &[u8],
    parse: fn(&[u8]) -> Result<T, BaseInputsError>,
) -> Result<Vec<T>, BaseInputsError> {
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    if stdout.last() != Some(&0) {
        return Err(data(BaseError::listing()));
    }
    stdout[..stdout.len() - 1]
        .split(|byte| *byte == 0)
        .map(|record| {
            if record.is_empty() {
                Err(data(BaseError::listing()))
            } else {
                parse(record)
            }
        })
        .collect()
}
fn parse_tree(record: &[u8]) -> Result<TreeEntry, BaseInputsError> {
    let tab = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| data(BaseError::listing()))?;
    let meta = std::str::from_utf8(&record[..tab]).map_err(|_| data(BaseError::listing()))?;
    let mut parts = meta.split(' ');
    let (Some(mode), Some(kind), Some(oid), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(data(BaseError::listing()));
    };
    if !valid_oid(oid) {
        return Err(data(BaseError::listing()));
    }
    let kind = match (mode, kind) {
        ("100644", "blob") => TreeKind::File,
        ("100755", "blob") => TreeKind::Executable,
        ("120000", "blob") => TreeKind::Symlink,
        ("160000", "commit") => TreeKind::Gitlink,
        _ => return Err(data(BaseError::listing())),
    };
    Ok(TreeEntry {
        kind,
        oid: oid.to_owned(),
        path: parse_path(&record[tab + 1..])?,
    })
}
fn parse_path(bytes: &[u8]) -> Result<RelativePath, BaseInputsError> {
    let text = std::str::from_utf8(bytes).map_err(|_| data(BaseError::listing()))?;
    RelativePath::new(text.to_owned()).map_err(|_| data(BaseError::listing()))
}
fn valid_oid(oid: &str) -> bool {
    (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn git(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.args(args).current_dir(root);
    command
}
