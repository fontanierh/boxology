//! Git-backed base-revision schema ingestion for `boxology check --base`.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use crate::{ExecuteError, GenerationPlan, PackageSchemas, execute::read_optional_file};
use std::{fmt, path::Path, process::Command};

type Rule = (&'static str, &'static str, &'static str);
const BASE_GIT_SOURCE: &str = "specs/s5-manifest-and-validation.md D6";
const BASE_REVISION_TEXT: &str = "the explicit base revision must resolve to a Git commit";
const BASE_SCHEMA_TEXT: &str = "a base-revision schema object must be readable as a Git blob";
const REVISION: Rule = ("BXW0091", BASE_REVISION_TEXT, BASE_GIT_SOURCE);
const BASE_SCHEMA: Rule = ("BXW0092", BASE_SCHEMA_TEXT, BASE_GIT_SOURCE);

/// The Git executable could not be started for an explicit-base check.
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

/// Resolves `revision` once and assembles schema pairs for every current generation plan.
///
/// The current plan is the sole authority for schema paths. A path absent at the resolved base is
/// represented as `None`; an object present there but not readable as a blob is `BXW0092`.
///
/// # Errors
/// Returns `BXW0091` when `revision` is not a commit, `BXW0092` for an unreadable base object, or
/// the existing `BXW0076` current-schema filesystem error.
pub fn base_package_schemas(
    root: &Path,
    revision: &str,
    plans: &[GenerationPlan],
) -> Result<Vec<PackageSchemas>, BaseSchemasError> {
    let oid = resolve_commit(root, revision)?;
    plans
        .iter()
        .map(|plan| {
            let submitted = read_optional_file(root, plan.schema_path())
                .map_err(BaseSchemasError::Submitted)?
                .ok_or_else(|| {
                    BaseSchemasError::Submitted(crate::execute::missing_schema(root, plan))
                })?;
            let base = read_schema(root, &oid, plan)?;
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
        .filter(|oid| {
            (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .ok_or_else(|| BaseSchemasError::Git(BaseError::revision()))?;
    Ok(oid.to_owned())
}

fn read_schema(
    root: &Path,
    oid: &str,
    plan: &GenerationPlan,
) -> Result<Option<Vec<u8>>, BaseSchemasError> {
    let object = format!("{oid}:{}", plan.schema_path().as_str());
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

fn git(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.args(args).current_dir(root);
    command
}
