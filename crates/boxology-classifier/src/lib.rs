//! A pure, fail-closed scaffold for classifying one schema revision against another.
//!
//! The classifier reads only the supplied [`SchemaDocument`] values. It consults no filesystem,
//! environment, network, clock, locale, process, or execution state, and has no policy controls
//! that could hide or relabel a finding. The later S4 slices add the structural taxonomy and
//! report format; this slice preserves the same public seam and fail-closed default.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_schema::{Diagnostic, Diagnostics, SchemaDocument, SchemaType, SchemaVariant};
use std::collections::BTreeMap;

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
            let roles = reachability(base, submitted);
            let changes = type_changes(base, submitted, &roles);
            if let Some(findings) = conditional_variant_additions(base, submitted, &changes) {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Roles {
    input: bool,
    output: bool,
}

#[derive(Debug, Eq, PartialEq)]
enum TypeChange {
    TypeAdded {
        name: String,
        roles: Roles,
    },
    TypeRemoved {
        name: String,
        roles: Roles,
    },
    TypesReordered,
    TypeDocsChanged {
        name: String,
    },
    TypeDeprecationChanged {
        name: String,
    },
    VariantAdded {
        type_name: String,
        variant_name: String,
        roles: Roles,
    },
    VariantRemoved {
        type_name: String,
        variant_name: String,
        roles: Roles,
    },
    VariantsReordered {
        type_name: String,
    },
    VariantDocsChanged {
        type_name: String,
        variant_name: String,
    },
    VariantDeprecationChanged {
        type_name: String,
        variant_name: String,
    },
    VariantPayloadChanged {
        type_name: String,
        variant_name: String,
        roles: Roles,
    },
}

/// Reachability over the union of both documents' capability graphs.
///
/// Format 1 `InputSlot` / `OutputSlot` hold [`boxology_schema::BoundaryLeaf`] only, so no
/// declared type is input-reachable; `input` is computed but always false. Typed slots land in
/// #103 / #104. A type is output-reachable when any capability on either side names it as
/// `error` (error enums are output-reachable).
fn reachability<'a>(
    base: &'a SchemaDocument,
    submitted: &'a SchemaDocument,
) -> BTreeMap<&'a str, Roles> {
    let mut roles = BTreeMap::new();
    mark_error_outputs(&mut roles, base);
    mark_error_outputs(&mut roles, submitted);
    roles
}

fn mark_error_outputs<'a>(roles: &mut BTreeMap<&'a str, Roles>, document: &'a SchemaDocument) {
    for capability in &document.capabilities {
        let entry = roles.entry(capability.error.as_str()).or_insert(Roles {
            input: false,
            output: false,
        });
        entry.output = true;
    }
}

fn roles_for(roles: &BTreeMap<&str, Roles>, name: &str) -> Roles {
    roles.get(name).copied().unwrap_or(Roles {
        input: false,
        output: false,
    })
}

fn index_types(types: &[SchemaType]) -> BTreeMap<&str, &SchemaType> {
    let mut index = BTreeMap::new();
    for schema_type in types {
        index.insert(schema_type.name.as_str(), schema_type);
    }
    index
}

fn index_variants(variants: &[SchemaVariant]) -> BTreeMap<&str, &SchemaVariant> {
    let mut index = BTreeMap::new();
    for variant in variants {
        index.insert(variant.name.as_str(), variant);
    }
    index
}

fn common_name_sequence_differs<'a, T, U>(
    base_names: impl Iterator<Item = &'a str>,
    submitted_names: impl Iterator<Item = &'a str>,
    base_index: &BTreeMap<&str, &T>,
    submitted_index: &BTreeMap<&str, &U>,
) -> bool {
    let mut base_common = Vec::new();
    for name in base_names {
        if submitted_index.contains_key(name) {
            base_common.push(name);
        }
    }
    let mut submitted_common = Vec::new();
    for name in submitted_names {
        if base_index.contains_key(name) {
            submitted_common.push(name);
        }
    }
    base_common != submitted_common
}

fn type_changes(
    base: &SchemaDocument,
    submitted: &SchemaDocument,
    roles: &BTreeMap<&str, Roles>,
) -> Vec<TypeChange> {
    let mut changes = Vec::new();
    let base_by_name = index_types(&base.types);
    let submitted_by_name = index_types(&submitted.types);

    for base_type in &base.types {
        match submitted_by_name.get(base_type.name.as_str()) {
            None => {
                changes.push(TypeChange::TypeRemoved {
                    name: base_type.name.clone(),
                    roles: roles_for(roles, base_type.name.as_str()),
                });
            }
            Some(submitted_type) => {
                append_matched_type_changes(
                    &mut changes,
                    base_type,
                    submitted_type,
                    roles_for(roles, base_type.name.as_str()),
                );
            }
        }
    }

    for submitted_type in &submitted.types {
        if base_by_name.contains_key(submitted_type.name.as_str()) {
            continue;
        }
        changes.push(TypeChange::TypeAdded {
            name: submitted_type.name.clone(),
            roles: roles_for(roles, submitted_type.name.as_str()),
        });
    }

    if common_name_sequence_differs(
        base.types
            .iter()
            .map(|schema_type| schema_type.name.as_str()),
        submitted
            .types
            .iter()
            .map(|schema_type| schema_type.name.as_str()),
        &base_by_name,
        &submitted_by_name,
    ) {
        changes.push(TypeChange::TypesReordered);
    }

    changes
}

fn append_matched_type_changes(
    changes: &mut Vec<TypeChange>,
    base: &SchemaType,
    submitted: &SchemaType,
    roles: Roles,
) {
    if base.docs != submitted.docs {
        changes.push(TypeChange::TypeDocsChanged {
            name: base.name.clone(),
        });
    }
    if base.deprecation != submitted.deprecation {
        changes.push(TypeChange::TypeDeprecationChanged {
            name: base.name.clone(),
        });
    }

    let base_by_name = index_variants(&base.variants);
    let submitted_by_name = index_variants(&submitted.variants);

    for base_variant in &base.variants {
        match submitted_by_name.get(base_variant.name.as_str()) {
            None => {
                changes.push(TypeChange::VariantRemoved {
                    type_name: base.name.clone(),
                    variant_name: base_variant.name.clone(),
                    roles,
                });
            }
            Some(submitted_variant) => {
                append_matched_variant_changes(
                    changes,
                    base.name.as_str(),
                    base_variant,
                    submitted_variant,
                    roles,
                );
            }
        }
    }

    for submitted_variant in &submitted.variants {
        if base_by_name.contains_key(submitted_variant.name.as_str()) {
            continue;
        }
        changes.push(TypeChange::VariantAdded {
            type_name: base.name.clone(),
            variant_name: submitted_variant.name.clone(),
            roles,
        });
    }

    if common_name_sequence_differs(
        base.variants.iter().map(|variant| variant.name.as_str()),
        submitted
            .variants
            .iter()
            .map(|variant| variant.name.as_str()),
        &base_by_name,
        &submitted_by_name,
    ) {
        changes.push(TypeChange::VariantsReordered {
            type_name: base.name.clone(),
        });
    }
}

fn append_matched_variant_changes(
    changes: &mut Vec<TypeChange>,
    type_name: &str,
    base: &SchemaVariant,
    submitted: &SchemaVariant,
    roles: Roles,
) {
    if base.docs != submitted.docs {
        changes.push(TypeChange::VariantDocsChanged {
            type_name: type_name.to_owned(),
            variant_name: base.name.clone(),
        });
    }
    if base.deprecation != submitted.deprecation {
        changes.push(TypeChange::VariantDeprecationChanged {
            type_name: type_name.to_owned(),
            variant_name: base.name.clone(),
        });
    }
    if base.payload != submitted.payload {
        changes.push(TypeChange::VariantPayloadChanged {
            type_name: type_name.to_owned(),
            variant_name: base.name.clone(),
            roles,
        });
    }
}

fn conditional_variant_additions(
    base: &SchemaDocument,
    submitted: &SchemaDocument,
    changes: &[TypeChange],
) -> Option<Vec<Finding>> {
    if base.capabilities != submitted.capabilities
        || base.revision == submitted.revision
        || changes.is_empty()
    {
        return None;
    }

    let mut findings = Vec::new();
    for change in changes {
        let TypeChange::VariantAdded {
            type_name,
            variant_name,
            roles,
        } = change
        else {
            return None;
        };
        if !roles.output {
            return None;
        }
        findings.push(Finding {
            code: "BXC0029",
            path: [
                base.box_id.as_str(),
                "/type/",
                type_name.as_str(),
                "/variant/",
                variant_name.as_str(),
            ]
            .concat(),
            class: Class::CompatibleWithConditions,
            condition: Some("unknown-variant tolerance"),
        });
    }
    Some(findings)
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
