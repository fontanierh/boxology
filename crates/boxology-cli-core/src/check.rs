//! Pure per-package contract classification for `boxology check` step 3.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::BoxId;
use boxology_schema::{Diagnostics, SchemaDocument};
use boxology_workspace::{
    ClassificationFinding, ClassificationFindings, ContractClassificationCompletion,
};
use std::fmt;

type Rule = (&'static str, &'static str, &'static str);
const CHECK_D1_SOURCE: &str =
    "specs/s4-contract-change-classification.md D1; specs/s5-manifest-and-validation.md D6";
const CHECK_PAIRING_SOURCE: &str =
    "specs/s4-contract-change-classification.md D2 D6; specs/s5-manifest-and-validation.md D6";
const CHECK_BASE_TEXT: &str =
    "the base-revision schema document must satisfy the strict format-1 reader";
const CHECK_SUBMITTED_TEXT: &str =
    "the checked-in schema document must satisfy the strict format-1 reader";
const CHECK_PAIRING_TEXT: &str =
    "the base-revision and checked-in schema documents must pair and satisfy classifier integrity";
const CHECK_BASE: Rule = ("BXW0080", CHECK_BASE_TEXT, CHECK_D1_SOURCE);
const CHECK_SUBMITTED: Rule = ("BXW0081", CHECK_SUBMITTED_TEXT, CHECK_D1_SOURCE);
const CHECK_PAIRING: Rule = ("BXW0082", CHECK_PAIRING_TEXT, CHECK_PAIRING_SOURCE);

/// Caller supplied the same package id more than once; not a repository defect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DuplicatePackages;

/// Supplied base and checked-in schema bytes for one package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageSchemas {
    package: BoxId,
    base: Option<Vec<u8>>,
    submitted: Vec<u8>,
}

impl PackageSchemas {
    /// Constructs one package's supplied schema bytes.
    pub fn new(package: BoxId, base: Option<Vec<u8>>, submitted: Vec<u8>) -> Self {
        Self {
            package,
            base,
            submitted,
        }
    }

    /// Returns the package identity.
    pub fn package(&self) -> &BoxId {
        &self.package
    }

    /// Returns the optional base schema bytes.
    pub fn base(&self) -> Option<&[u8]> {
        self.base.as_deref()
    }

    /// Returns the submitted schema bytes.
    pub fn submitted(&self) -> &[u8] {
        &self.submitted
    }
}

/// A coded check-classification failure with schema or classifier diagnostics.
#[derive(Debug)]
pub struct CheckClassificationError {
    package: BoxId,
    code: &'static str,
    side: &'static str,
    detail: &'static str,
    diagnostics: Diagnostics,
}

impl CheckClassificationError {
    /// Returns the package whose schemas failed.
    pub fn package(&self) -> &BoxId {
        &self.package
    }

    /// Returns the stable `BXW####` code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Returns which stage failed: `base`, `submitted`, or `pairing`.
    pub fn side(&self) -> &'static str {
        self.side
    }

    /// Returns the stable static rule detail.
    pub fn detail(&self) -> &'static str {
        self.detail
    }

    /// Returns the schema or classifier diagnostics unmodified.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    fn fail(package: BoxId, rule: Rule, side: &'static str, diagnostics: Diagnostics) -> Self {
        Self {
            package,
            code: rule.0,
            side,
            detail: rule.1,
            diagnostics,
        }
    }
}

impl fmt::Display for CheckClassificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {} {}: {}: {}",
            self.code, self.package, self.side, self.detail, self.diagnostics
        )
    }
}

/// Failure from [`classify_step`]: caller misuse or a coded package failure.
#[derive(Debug)]
pub enum ClassifyStepError {
    /// The same package id was supplied more than once.
    Duplicate(DuplicatePackages),
    /// A schema parse or classifier integrity failure for one package.
    Classification(CheckClassificationError),
}

/// Classifies each package's base-revision and checked-in schema bytes.
///
/// # Errors
/// Returns [`ClassifyStepError::Duplicate`] when a package id appears more than once. Returns
/// [`ClassifyStepError::Classification`] with `BXW0080` when base-revision bytes fail the strict
/// reader, `BXW0081` when checked-in bytes fail the strict reader, or `BXW0082` when pairing or
/// integrity fails. Integrity diagnostics are embedded unmodified; they never become report
/// findings.
pub fn classify_step(
    packages: &[PackageSchemas],
) -> Result<ContractClassificationCompletion, ClassifyStepError> {
    if has_duplicate_package(packages) {
        return Err(ClassifyStepError::Duplicate(DuplicatePackages));
    }
    let mut ordered: Vec<&PackageSchemas> = packages.iter().collect();
    ordered.sort_by_key(|package| package.package.as_str());
    let mut findings = Vec::new();
    for package in ordered {
        findings.extend(classify_package(package).map_err(ClassifyStepError::Classification)?);
    }
    Ok(match ClassificationFindings::new(findings) {
        None => ContractClassificationCompletion::Passed,
        Some(findings) => ContractClassificationCompletion::Failed(findings),
    })
}

fn has_duplicate_package(packages: &[PackageSchemas]) -> bool {
    let mut seen: Vec<&str> = packages
        .iter()
        .map(|package| package.package.as_str())
        .collect();
    seen.sort_unstable();
    seen.windows(2).any(|pair| pair[0] == pair[1])
}

fn classify_package(
    package: &PackageSchemas,
) -> Result<Vec<ClassificationFinding>, CheckClassificationError> {
    let package_id = package.package.clone();
    let base = match package.base.as_deref() {
        Some(bytes) => Some(SchemaDocument::parse(bytes).map_err(|diagnostics| {
            CheckClassificationError::fail(package_id.clone(), CHECK_BASE, "base", diagnostics)
        })?),
        None => None,
    };
    let submitted = SchemaDocument::parse(&package.submitted).map_err(|diagnostics| {
        CheckClassificationError::fail(
            package_id.clone(),
            CHECK_SUBMITTED,
            "submitted",
            diagnostics,
        )
    })?;
    let report =
        boxology_classifier::classify(base.as_ref(), Some(&submitted)).map_err(|diagnostics| {
            CheckClassificationError::fail(
                package_id.clone(),
                CHECK_PAIRING,
                "pairing",
                diagnostics,
            )
        })?;
    Ok(report
        .findings()
        .iter()
        .map(|finding| {
            ClassificationFinding::new(
                package_id.clone(),
                finding.path().to_owned(),
                finding.code().to_owned(),
                finding.class().canonical_name().to_owned(),
                finding.condition().map(str::to_owned),
            )
        })
        .collect())
}
