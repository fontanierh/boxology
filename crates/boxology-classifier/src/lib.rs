//! A pure, fail-closed scaffold for classifying one schema revision against another.
//!
//! The classifier reads only the supplied [`SchemaDocument`] values. It consults no filesystem,
//! environment, network, clock, locale, process, or execution state, and has no policy controls
//! that could hide or relabel a finding. The later S4 slices add the structural taxonomy and
//! report format; this slice preserves the same public seam and fail-closed default.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_schema::{Diagnostic, Diagnostics, SchemaDocument, SchemaVariant};

/// The compatibility class of one schema change, ordered from least to most severe.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Class {
    /// No compatibility-relevant change occurred.
    Unchanged,
    /// Only documentation changed.
    Documentation,
    /// Only deprecation metadata changed.
    Deprecation,
    /// New surface was introduced.
    Additive,
    /// A change is compatible only under a stated migration condition.
    CompatibleWithConditions,
    /// Existing surface was tightened or removed, or the change is unclassified.
    Incompatible,
}

impl Class {
    /// Returns the exact snake-case name of this class.
    pub fn canonical_name(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Documentation => "documentation",
            Self::Deprecation => "deprecation",
            Self::Additive => "additive",
            Self::CompatibleWithConditions => "compatible_with_conditions",
            Self::Incompatible => "incompatible",
        }
    }
}

/// One classified schema change.
#[derive(Debug, Eq, PartialEq)]
pub struct Finding {
    code: &'static str,
    path: String,
    class: Class,
    condition: Option<&'static str>,
}

impl Finding {
    /// Returns the stable classifier code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the canonical identity path of the change.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the compatibility class of the change.
    pub fn class(&self) -> Class {
        self.class
    }

    /// Returns the migration condition, when this finding is conditional.
    pub fn condition(&self) -> Option<&'static str> {
        self.condition
    }
}

/// The findings and maximum-severity verdict for one classification.
#[derive(Debug, Eq, PartialEq)]
pub struct ClassificationReport {
    findings: Vec<Finding>,
    verdict: Class,
}

impl ClassificationReport {
    /// Returns every finding in report order.
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// Returns the maximum class, or [`Class::Unchanged`] when there are no findings.
    pub fn verdict(&self) -> Class {
        self.verdict
    }
}

/// Classifies the supplied base and submitted schema documents.
pub fn classify(
    base: Option<&SchemaDocument>,
    submitted: Option<&SchemaDocument>,
) -> Result<ClassificationReport, Diagnostics> {
    match (base, submitted) {
        (None, None) => {
            Err(
                Diagnostics::new(Vec::from([Diagnostic::classification_requires_document()]))
                    .expect("one classification diagnostic"),
            )
        }
        (None, Some(document)) => Ok(report(Vec::from([Finding {
            code: "BXC0026",
            path: document.box_id.as_str().to_owned(),
            class: Class::Additive,
            condition: None,
        }]))),
        (Some(document), None) => Ok(report(Vec::from([Finding {
            code: "BXC0027",
            path: document.box_id.as_str().to_owned(),
            class: Class::Incompatible,
            condition: None,
        }]))),
        (Some(base), Some(submitted)) if base.box_id != submitted.box_id => {
            Err(Diagnostics::new(Vec::from([Diagnostic::box_id_mismatch()]))
                .expect("one classification diagnostic"))
        }
        (Some(base), Some(submitted)) if equal_modulo_provenance(base, submitted) => {
            Ok(report(Vec::new()))
        }
        (Some(base), Some(submitted)) => {
            if let Some(findings) = variant_addition_findings(base, submitted) {
                Ok(report(findings))
            } else {
                Ok(report(Vec::from([Finding {
                    code: "BXC0028",
                    path: base.box_id.as_str().to_owned(),
                    class: Class::Incompatible,
                    condition: None,
                }])))
            }
        }
    }
}

fn variant_addition_findings(
    base: &SchemaDocument,
    submitted: &SchemaDocument,
) -> Option<Vec<Finding>> {
    if base.revision == submitted.revision || base.capabilities != submitted.capabilities {
        return None;
    }

    let mut findings = Vec::new();
    if base.types.len() != submitted.types.len() {
        return None;
    }

    for (base_type, submitted_type) in base.types.iter().zip(&submitted.types) {
        if base_type.name != submitted_type.name
            || base_type.docs != submitted_type.docs
            || base_type.deprecation != submitted_type.deprecation
        {
            return None;
        }

        let added_variants = added_variants(&base_type.variants, &submitted_type.variants)?;
        if added_variants.is_empty() {
            continue;
        }
        if !base
            .capabilities
            .iter()
            .any(|capability| capability.error.as_str() == submitted_type.name.as_str())
        {
            return None;
        }

        findings.extend(added_variants.into_iter().map(|variant| {
            Finding {
                code: "BXC0029",
                path: [
                    base.box_id.as_str(),
                    "/type/",
                    submitted_type.name.as_str(),
                    "/variant/",
                    variant.name.as_str(),
                ]
                .concat(),
                class: Class::CompatibleWithConditions,
                condition: Some("unknown-variant tolerance"),
            }
        }));
    }

    (!findings.is_empty()).then_some(findings)
}

fn added_variants<'a>(
    base: &'a [SchemaVariant],
    submitted: &'a [SchemaVariant],
) -> Option<Vec<&'a SchemaVariant>> {
    let mut base_index = 0;
    let mut added = Vec::new();
    for variant in submitted {
        if base_index < base.len() && base[base_index].eq(variant) {
            base_index += 1;
        } else {
            added.push(variant);
        }
    }
    (base_index == base.len()).then_some(added)
}

fn equal_modulo_provenance(base: &SchemaDocument, submitted: &SchemaDocument) -> bool {
    let SchemaDocument {
        box_id: base_box_id,
        capabilities: base_capabilities,
        provenance: _,
        revision: base_revision,
        types: base_types,
    } = base;
    let SchemaDocument {
        box_id: submitted_box_id,
        capabilities: submitted_capabilities,
        provenance: _,
        revision: submitted_revision,
        types: submitted_types,
    } = submitted;
    base_box_id == submitted_box_id
        && base_capabilities == submitted_capabilities
        && base_revision == submitted_revision
        && base_types == submitted_types
}

fn report(findings: Vec<Finding>) -> ClassificationReport {
    let mut findings = findings;
    findings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.cmp(right.code))
    });
    let verdict = findings
        .iter()
        .map(|finding| finding.class)
        .max()
        .unwrap_or(Class::Unchanged);
    ClassificationReport { findings, verdict }
}

#[cfg(test)]
mod tests;
