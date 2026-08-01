//! A pure, fail-closed classifier for one schema revision against another.
//!
//! The classifier reads only the supplied [`SchemaDocument`] values. It consults no filesystem,
//! environment, network, clock, locale, process, or execution state, and has no policy controls
//! that could hide or relabel a finding. Named type-graph rows emit structured findings; every
//! unmatched difference falls to the fail-closed default. Five structural capability rows are
//! named; reserved capability metadata and reorder differences remain fail-closed.
//! Canonical report renderings are available as [`render_text`] and [`render_json`].

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_schema::{
    Diagnostic, Diagnostics, SchemaCapability, SchemaDocument, SchemaType, SchemaVariant,
};
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
        // D6 check A: equal revisions with any remaining difference is an integrity error. Short-
        // circuit rather than compute-then-check so an engine miss under equal revisions cannot
        // go silent; fail-closedness under differing revisions guarantees detected differences
        // would have yielded a finding.
        (Some(base), Some(submitted)) if base.revision == submitted.revision => {
            Err(Diagnostics::new(Vec::from([
                Diagnostic::integrity_findings_under_equal_revisions(),
            ]))
            .expect("one integrity diagnostic"))
        }
        (Some(base), Some(submitted)) => {
            let findings = classify_paired_documents(base, submitted);
            // D6 check B: differing revisions with zero findings means the projection and the
            // classifier disagree (for example a revision-only difference).
            // Deviates from AC4's letter deliberately: an empty finding set is now Err(BXC0038)
            // rather than an incompatible report carrying the unclassified-change code. D6 requires
            // failing loudly when revision and findings disagree.
            if findings.is_empty() {
                Err(Diagnostics::new(Vec::from([
                    Diagnostic::integrity_silence_under_differing_revisions(),
                ]))
                .expect("one integrity diagnostic"))
            } else {
                Ok(report(findings))
            }
        }
    }
}

/// Fail-closed unclassified-change code (D5 default).
const CODE_FAIL_CLOSED: &str = "BXC0028";

/// Output-reachable type added (D5 additive row).
const CODE_TYPE_ADDED: &str = "BXC0031";

/// Type removed (D5 incompatible row).
const CODE_TYPE_REMOVED: &str = "BXC0032";

/// Documentation changed on any type-graph element (D5 documentation row).
const CODE_DOCS_CHANGED: &str = "BXC0033";

/// Deprecation metadata changed on any type-graph element (D5 deprecation row).
const CODE_DEPRECATION_CHANGED: &str = "BXC0034";

/// Variant removed (D5 incompatible row).
const CODE_VARIANT_REMOVED: &str = "BXC0035";

/// Referenced error-enum variant added (D5 conditional row).
const CODE_VARIANT_ADDED: &str = "BXC0036";

/// Capability added (D5 additive row).
const CODE_CAPABILITY_ADDED: &str = "BXC0039";

/// Capability removed (D5 incompatible row).
const CODE_CAPABILITY_REMOVED: &str = "BXC0040";

/// Capability input parameter name changed (D5 incompatible row).
const CODE_INPUT_NAME_CHANGED: &str = "BXC0041";

/// Capability input leaf type changed (D5 incompatible row).
const CODE_INPUT_LEAF_CHANGED: &str = "BXC0042";

/// Capability output leaf type changed (D5 incompatible row).
const CODE_OUTPUT_LEAF_CHANGED: &str = "BXC0043";

/// Migration condition for referenced error-enum variant addition.
const CONDITION_UNKNOWN_VARIANT: &str = "unknown-variant tolerance";

fn fail_closed_finding(base: &SchemaDocument) -> Finding {
    Finding {
        code: CODE_FAIL_CLOSED,
        path: base.box_id.as_str().to_owned(),
        class: Class::Incompatible,
        condition: None,
    }
}

/// Applies the D5 type-graph and structural capability taxonomies, then the fail-closed default.
///
/// Named findings are always emitted individually. Unreferenced *additions* (type or variant) fall
/// to the fail-closed default per D5's preamble — a declared type reachable from no capability is
/// not a named additive/conditional row. Documentation, deprecation, and removals classify by their
/// D5 table row regardless of reachability (D5's preamble tension with those rows is tracked under
/// #319). Capability additions, removals, input-name changes, input-leaf changes, and output-leaf
/// changes use their named rows. Capability documentation, deprecation, declared-error,
/// exposure, idempotency, and reorder changes, like `VariantPayloadChanged`, remain reserved and
/// fail closed at `<box>`. A revision-only difference (no type or capability delta) yields an empty
/// finding list; `classify` turns that empty result into the D6 check-B integrity error.
fn classify_paired_documents(base: &SchemaDocument, submitted: &SchemaDocument) -> Vec<Finding> {
    let roles = reachability(base, submitted);
    let changes = type_changes(base, submitted, &roles);
    let mut findings = Vec::new();
    let mut unclassified = false;
    for change in &changes {
        match change {
            TypeChange::TypeAdded { name, roles } if roles.output => {
                findings.push(Finding {
                    code: CODE_TYPE_ADDED,
                    path: type_path(base, name),
                    class: Class::Additive,
                    condition: None,
                });
            }
            TypeChange::TypeRemoved { name, .. } => {
                findings.push(Finding {
                    code: CODE_TYPE_REMOVED,
                    path: type_path(base, name),
                    class: Class::Incompatible,
                    condition: None,
                });
            }
            TypeChange::TypeDocsChanged { name } => {
                findings.push(Finding {
                    code: CODE_DOCS_CHANGED,
                    path: type_path(base, name),
                    class: Class::Documentation,
                    condition: None,
                });
            }
            TypeChange::TypeDeprecationChanged { name } => {
                findings.push(Finding {
                    code: CODE_DEPRECATION_CHANGED,
                    path: type_path(base, name),
                    class: Class::Deprecation,
                    condition: None,
                });
            }
            TypeChange::VariantAdded {
                type_name,
                variant_name,
                roles,
            } if roles.output => {
                findings.push(Finding {
                    code: CODE_VARIANT_ADDED,
                    path: variant_path(base, type_name, variant_name),
                    class: Class::CompatibleWithConditions,
                    condition: Some(CONDITION_UNKNOWN_VARIANT),
                });
            }
            TypeChange::VariantRemoved {
                type_name,
                variant_name,
                ..
            } => {
                findings.push(Finding {
                    code: CODE_VARIANT_REMOVED,
                    path: variant_path(base, type_name, variant_name),
                    class: Class::Incompatible,
                    condition: None,
                });
            }
            TypeChange::VariantDocsChanged {
                type_name,
                variant_name,
            } => {
                findings.push(Finding {
                    code: CODE_DOCS_CHANGED,
                    path: variant_path(base, type_name, variant_name),
                    class: Class::Documentation,
                    condition: None,
                });
            }
            TypeChange::VariantDeprecationChanged {
                type_name,
                variant_name,
            } => {
                findings.push(Finding {
                    code: CODE_DEPRECATION_CHANGED,
                    path: variant_path(base, type_name, variant_name),
                    class: Class::Deprecation,
                    condition: None,
                });
            }
            // Unreferenced type/variant additions, reorderings, and reserved payload-shape
            // changes have no named row in this slice.
            TypeChange::TypeAdded { .. }
            | TypeChange::VariantAdded { .. }
            | TypeChange::TypesReordered
            | TypeChange::VariantsReordered { .. }
            | TypeChange::VariantPayloadChanged { .. } => {
                unclassified = true;
            }
        }
    }

    let changes = capability_changes(base, submitted);
    for change in &changes {
        match change {
            CapabilityChange::CapabilityAdded { name } => findings.push(Finding {
                code: CODE_CAPABILITY_ADDED,
                path: capability_path(base, name),
                class: Class::Additive,
                condition: None,
            }),
            CapabilityChange::CapabilityRemoved { name } => findings.push(Finding {
                code: CODE_CAPABILITY_REMOVED,
                path: capability_path(base, name),
                class: Class::Incompatible,
                condition: None,
            }),
            CapabilityChange::InputNameChanged { name } => findings.push(Finding {
                code: CODE_INPUT_NAME_CHANGED,
                path: capability_input_path(base, name),
                class: Class::Incompatible,
                condition: None,
            }),
            CapabilityChange::InputLeafChanged { name } => findings.push(Finding {
                code: CODE_INPUT_LEAF_CHANGED,
                path: capability_input_path(base, name),
                class: Class::Incompatible,
                condition: None,
            }),
            CapabilityChange::OutputLeafChanged { name } => findings.push(Finding {
                code: CODE_OUTPUT_LEAF_CHANGED,
                path: capability_output_path(base, name),
                class: Class::Incompatible,
                condition: None,
            }),
            CapabilityChange::CapabilitiesReordered
            | CapabilityChange::CapabilityMetadataChanged { .. } => {
                unclassified = true;
            }
        }
    }

    // Fail-closed default: unmatched type-graph kinds or reserved capability kinds emit one
    // BXC0028 at <box>. Every other capability difference has a named change above. An empty
    // finding list (revision-only) is left empty for classify's check B.
    if unclassified {
        findings.push(fail_closed_finding(base));
    }
    findings
}

fn capability_path(base: &SchemaDocument, name: &str) -> String {
    [base.box_id.as_str(), ".", name].concat()
}

fn capability_input_path(base: &SchemaDocument, name: &str) -> String {
    [capability_path(base, name).as_str(), "/input"].concat()
}

fn capability_output_path(base: &SchemaDocument, name: &str) -> String {
    [capability_path(base, name).as_str(), "/output"].concat()
}

fn type_path(base: &SchemaDocument, name: &str) -> String {
    [base.box_id.as_str(), "/type/", name].concat()
}

fn variant_path(base: &SchemaDocument, type_name: &str, variant_name: &str) -> String {
    [
        base.box_id.as_str(),
        "/type/",
        type_name,
        "/variant/",
        variant_name,
    ]
    .concat()
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

#[derive(Debug, Eq, PartialEq)]
enum CapabilityChange {
    CapabilityAdded { name: String },
    CapabilityRemoved { name: String },
    CapabilitiesReordered,
    CapabilityMetadataChanged { name: String },
    InputNameChanged { name: String },
    InputLeafChanged { name: String },
    OutputLeafChanged { name: String },
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

fn index_capabilities(capabilities: &[SchemaCapability]) -> BTreeMap<&str, &SchemaCapability> {
    let mut index = BTreeMap::new();
    for capability in capabilities {
        index.insert(capability.name.as_str(), capability);
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

fn capability_changes(base: &SchemaDocument, submitted: &SchemaDocument) -> Vec<CapabilityChange> {
    let mut changes = Vec::new();
    let base_by_name = index_capabilities(&base.capabilities);
    let submitted_by_name = index_capabilities(&submitted.capabilities);

    for base_capability in &base.capabilities {
        match submitted_by_name.get(base_capability.name.as_str()) {
            None => changes.push(CapabilityChange::CapabilityRemoved {
                name: base_capability.name.as_str().to_owned(),
            }),
            Some(submitted_capability) => append_matched_capability_changes(
                &mut changes,
                base_capability,
                submitted_capability,
            ),
        }
    }

    for submitted_capability in &submitted.capabilities {
        if base_by_name.contains_key(submitted_capability.name.as_str()) {
            continue;
        }
        changes.push(CapabilityChange::CapabilityAdded {
            name: submitted_capability.name.as_str().to_owned(),
        });
    }

    if common_name_sequence_differs(
        base.capabilities
            .iter()
            .map(|capability| capability.name.as_str()),
        submitted
            .capabilities
            .iter()
            .map(|capability| capability.name.as_str()),
        &base_by_name,
        &submitted_by_name,
    ) {
        changes.push(CapabilityChange::CapabilitiesReordered);
    }

    changes
}

fn append_matched_capability_changes(
    changes: &mut Vec<CapabilityChange>,
    base: &SchemaCapability,
    submitted: &SchemaCapability,
) {
    if base.input.leaf != submitted.input.leaf {
        changes.push(CapabilityChange::InputLeafChanged {
            name: base.name.as_str().to_owned(),
        });
    }
    if base.input.name != submitted.input.name {
        changes.push(CapabilityChange::InputNameChanged {
            name: base.name.as_str().to_owned(),
        });
    }
    if base.output.leaf != submitted.output.leaf {
        changes.push(CapabilityChange::OutputLeafChanged {
            name: base.name.as_str().to_owned(),
        });
    }
    if base.docs != submitted.docs
        || base.deprecation != submitted.deprecation
        || base.error != submitted.error
        || base.max_exposure != submitted.max_exposure
        || base.idempotency != submitted.idempotency
    {
        changes.push(CapabilityChange::CapabilityMetadataChanged {
            name: base.name.as_str().to_owned(),
        });
    }
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

mod report;
pub use report::{render_json, render_text};

#[cfg(test)]
mod tests;
