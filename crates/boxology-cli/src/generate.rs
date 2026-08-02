//! Pure selection and assembly of a contract-generation plan.
#![deny(missing_docs)]
#![forbid(unsafe_code)]
use boxology_contract::BoxId;
use boxology_manifest::{CrateRole, GlobPattern, RelativePath};
use boxology_workspace::{Package, Workspace};
use std::fmt;

type Rule = (&'static str, &'static str, &'static str);
const SOURCE: &str = "specs/s5-manifest-and-validation.md D5";
const CONTRACT_GENERATOR: &str = "boxology-contract";
const CARGO_GENERATOR: &str = "cargo";
const UNKNOWN_GENERATOR_TEXT: &str =
    "only the boxology-contract generator is supported by generate";
const UNKNOWN_PACKAGE_TEXT: &str = "the requested package must be a discovered workspace package";
const NO_CANDIDATE_TEXT: &str = "the selected package must declare a contract-generation output";
const IMPLEMENTATION_ROOT_TEXT: &str =
    "a generation candidate must declare exactly one box-implementation crate";
const DUPLICATE_OUTPUTS_TEXT: &str =
    "a package must declare at most one contract-generation output";
const UNKNOWN_IMPORT_TEXT: &str = "a declared import must name a discovered workspace package";
const NO_IMPORT_CANDIDATE_TEXT: &str =
    "an imported package must declare a contract-generation output";
const UNKNOWN_GENERATOR: Rule = ("BXW0064", UNKNOWN_GENERATOR_TEXT, SOURCE);
const UNKNOWN_PACKAGE: Rule = ("BXW0065", UNKNOWN_PACKAGE_TEXT, SOURCE);
const NO_CANDIDATE: Rule = ("BXW0066", NO_CANDIDATE_TEXT, SOURCE);
const IMPLEMENTATION_ROOT: Rule = ("BXW0067", IMPLEMENTATION_ROOT_TEXT, SOURCE);
const DUPLICATE_OUTPUTS: Rule = ("BXW0069", DUPLICATE_OUTPUTS_TEXT, SOURCE);
const UNKNOWN_IMPORT: Rule = ("BXW0084", UNKNOWN_IMPORT_TEXT, SOURCE);
const NO_IMPORT_CANDIDATE: Rule = ("BXW0085", NO_IMPORT_CANDIDATE_TEXT, SOURCE);
const SCHEMA: &str = "generated/schema.json";

/// One declared import resolved to the imported package's checked-in schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedImport {
    package: BoxId,
    schema: RelativePath,
}
impl ResolvedImport {
    /// Returns the imported package identity.
    pub fn package(&self) -> &BoxId {
        &self.package
    }
    /// Returns the workspace-relative path of the imported package's schema.
    pub fn schema(&self) -> &RelativePath {
        &self.schema
    }
}

/// The pure inputs needed by the next generation-execution slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationPlan {
    package: BoxId,
    manifest_path: RelativePath,
    package_root: Option<RelativePath>,
    derived_output: BoxId,
    crate_root: RelativePath,
    inputs: Vec<RelativePath>,
    imports: Vec<ResolvedImport>,
    outputs: Vec<GlobPattern>,
}
impl GenerationPlan {
    /// Returns the selected package identity.
    pub fn package_id(&self) -> &BoxId {
        &self.package
    }
    /// Returns the workspace-relative package manifest path.
    pub fn manifest_path(&self) -> &RelativePath {
        &self.manifest_path
    }
    /// Returns the package root, or `None` for the workspace-root package.
    pub fn package_root(&self) -> Option<&RelativePath> {
        self.package_root.as_ref()
    }
    /// Returns the selected derived-output identity.
    pub fn derived_output_id(&self) -> &BoxId {
        &self.derived_output
    }
    /// Returns the exact package-relative implementation crate root.
    pub fn crate_root(&self) -> &RelativePath {
        &self.crate_root
    }
    /// Returns matching package-relative non-derived inputs in stable classification order.
    pub fn inputs(&self) -> &[RelativePath] {
        &self.inputs
    }
    /// Returns declared imports resolved in manifest declaration order.
    pub fn imports(&self) -> &[ResolvedImport] {
        &self.imports
    }
    /// Returns the selected output's declared patterns in declaration order.
    pub fn outputs(&self) -> &[GlobPattern] {
        &self.outputs
    }
}
/// A stable planning failure with a validated logical path and no filesystem payload.
#[derive(Debug, Eq, PartialEq)]
pub struct PlanError(&'static str, RelativePath, &'static str);
impl PlanError {
    /// Returns the stable `BXW####` code.
    pub fn code(&self) -> &'static str {
        self.0
    }
    /// Returns the validated workspace-relative location of the failure.
    pub fn path(&self) -> &RelativePath {
        &self.1
    }
    /// Returns the stable rule detail.
    pub fn detail(&self) -> &'static str {
        self.2
    }

    /// Returns whether this is the invocation-level unknown-package failure.
    pub fn is_unknown_package(&self) -> bool {
        self.0 == UNKNOWN_PACKAGE.0
    }
}
impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {:?}: {}", self.0, self.1.as_str(), self.2)
    }
}
impl std::error::Error for PlanError {}
/// Selects contract-generator candidates and assembles their pure plans in package-id order.
pub fn plan(
    workspace: &Workspace,
    selection: Option<&BoxId>,
) -> Result<Vec<GenerationPlan>, PlanError> {
    let selected = selection
        .map(|id| {
            workspace
                .packages()
                .iter()
                .find(|package| package.id() == id)
                .ok_or_else(|| failure(UNKNOWN_PACKAGE, request_path()))
        })
        .transpose()?;
    let mut plans = Vec::new();
    for package in workspace.packages() {
        if selected.is_some_and(|wanted| wanted.id() != package.id()) {
            continue;
        }
        let candidates = contract_outputs(package)?;
        if candidates.is_empty() {
            if selected.is_some() {
                return Err(failure(NO_CANDIDATE, package.manifest_path().clone()));
            }
            continue;
        }
        if candidates.len() > 1 {
            return Err(failure(DUPLICATE_OUTPUTS, package.manifest_path().clone()));
        }
        plans.push(assemble(workspace, package, candidates[0])?);
    }
    Ok(plans)
}
fn contract_outputs(
    package: &Package,
) -> Result<Vec<&boxology_manifest::DerivedOutput>, PlanError> {
    let mut candidates = Vec::new();
    for output in package.manifest().derived() {
        if output.generator() == CARGO_GENERATOR {
            continue;
        }
        if output.generator() == CONTRACT_GENERATOR {
            candidates.push(output);
        } else {
            return Err(failure(UNKNOWN_GENERATOR, package.manifest_path().clone()));
        }
    }
    Ok(candidates)
}
fn assemble(
    workspace: &Workspace,
    package: &Package,
    output: &boxology_manifest::DerivedOutput,
) -> Result<GenerationPlan, PlanError> {
    let implementations: Vec<_> = package
        .manifest()
        .crates()
        .iter()
        .filter(|entry| entry.role() == CrateRole::BoxImplementation)
        .collect();
    if implementations.len() != 1 {
        return Err(failure(
            IMPLEMENTATION_ROOT,
            package.manifest_path().clone(),
        ));
    }
    let imports = package
        .manifest()
        .imports()
        .iter()
        .map(|import| {
            let Some(target) = workspace
                .packages()
                .iter()
                .find(|target| target.id() == import.package())
            else {
                return Err(failure(UNKNOWN_IMPORT, package.manifest_path().clone()));
            };
            let candidates = contract_outputs(target)?;
            if candidates.is_empty() {
                return Err(failure(
                    NO_IMPORT_CANDIDATE,
                    package.manifest_path().clone(),
                ));
            }
            if candidates.len() > 1 {
                return Err(failure(DUPLICATE_OUTPUTS, target.manifest_path().clone()));
            }
            let schema = target.root().map_or_else(
                || SCHEMA.to_owned(),
                |root| format!("{}/{}", root.as_str(), SCHEMA),
            );
            let schema = RelativePath::new(schema).expect("fixed schema path is valid");
            Ok(ResolvedImport {
                package: import.package().clone(),
                schema,
            })
        })
        .collect::<Result<Vec<_>, PlanError>>()?;
    let raw_root = format!("{}/src/lib.rs", implementations[0].path().as_str());
    let Some(crate_root) = RelativePath::new(raw_root).ok() else {
        return Err(failure(
            IMPLEMENTATION_ROOT,
            package.manifest_path().clone(),
        ));
    };
    let inputs = workspace
        .classifications()
        .iter()
        .filter(|classification| {
            classification.package() == package.id() && classification.derived_output().is_none()
        })
        .filter_map(|classification| {
            let path = package.relative(classification.path())?;
            output
                .inputs()
                .iter()
                .any(|input| input.matches(&path))
                .then_some(path)
        })
        .collect();
    Ok(GenerationPlan {
        package: package.id().clone(),
        manifest_path: package.manifest_path().clone(),
        package_root: package.root().cloned(),
        derived_output: output.id().clone(),
        crate_root,
        inputs,
        imports,
        outputs: output.outputs().to_vec(),
    })
}
fn request_path() -> RelativePath {
    RelativePath::new("<request>").expect("static request path is valid")
}

fn failure(rule: Rule, path: RelativePath) -> PlanError {
    PlanError(rule.0, path, rule.1)
}
