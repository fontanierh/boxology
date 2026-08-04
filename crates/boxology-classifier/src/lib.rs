//! A pure, fail-closed classifier for one schema revision against another.
//!
//! The classifier reads only the supplied [`SchemaDocument`] values. It consults no filesystem,
//! environment, network, clock, locale, process, or execution state, and has no policy controls
//! that could hide or relabel a finding. Named type-graph rows emit structured findings; every
//! unmatched difference falls to the fail-closed default. Structural capability, metadata,
//! named-field, and payload rows are named; reorder differences remain fail-closed.
//! Canonical report renderings are available as [`render_text`] and [`render_json`].

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_schema::{
    Diagnostic, Diagnostics, ExposureLevel, Idempotency, SchemaCapability, SchemaDocument,
    SchemaField, SchemaPayload, SchemaType, SchemaVariant,
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
    kind: &'static str,
    class: Class,
    base_excerpt: Option<String>,
    submitted_excerpt: Option<String>,
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

    /// Returns the D5 change-kind name for this finding.
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// Returns the compatibility class of the change.
    pub fn class(&self) -> Class {
        self.class
    }

    /// Returns the base-side compared-value excerpt, when present.
    pub fn base_excerpt(&self) -> Option<&str> {
        self.base_excerpt.as_deref()
    }

    /// Returns the submitted-side compared-value excerpt, when present.
    pub fn submitted_excerpt(&self) -> Option<&str> {
        self.submitted_excerpt.as_deref()
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
            kind: KIND_CONTRACT_INTRODUCED,
            class: Class::Additive,
            base_excerpt: None,
            submitted_excerpt: Some(document.box_id.as_str().to_owned()),
            condition: None,
        }]))),
        (Some(document), None) => Ok(report(Vec::from([Finding {
            code: "BXC0027",
            path: document.box_id.as_str().to_owned(),
            kind: KIND_CONTRACT_REMOVED,
            class: Class::Incompatible,
            base_excerpt: Some(document.box_id.as_str().to_owned()),
            submitted_excerpt: None,
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

/// Capability declared error changed (D5 incompatible row).
const CODE_ERROR_CHANGED: &str = "BXC0044";

/// Capability exposure raised (D5 additive row).
const CODE_EXPOSURE_RAISED: &str = "BXC0045";

/// Capability exposure lowered (D5 incompatible row).
const CODE_EXPOSURE_LOWERED: &str = "BXC0046";

/// Capability idempotency strengthened (D5 additive row).
const CODE_IDEMPOTENCY_STRENGTHENED: &str = "BXC0047";

/// Capability idempotency weakened (D5 incompatible row).
const CODE_IDEMPOTENCY_WEAKENED: &str = "BXC0048";

/// Named payload field added (D5 role-sensitive row).
const CODE_FIELD_ADDED: &str = "BXC0049";

/// Named payload field removed (D5 incompatible row).
const CODE_FIELD_REMOVED: &str = "BXC0050";

/// Named payload field type changed (D5 incompatible row).
const CODE_FIELD_TYPE_CHANGED: &str = "BXC0051";

/// Error payload kind or value type changed (D5 incompatible row).
const CODE_PAYLOAD_CHANGED: &str = "BXC0052";

const KIND_CONTRACT_INTRODUCED: &str = "contract introduced";
const KIND_CONTRACT_REMOVED: &str = "contract removed";
const KIND_UNCLASSIFIED: &str = "unclassified change";
const KIND_TYPE_ADDED: &str = "type added";
const KIND_TYPE_REMOVED: &str = "type removed";
const KIND_DOCS_CHANGED: &str = "documentation changed";
const KIND_DEPRECATION_CHANGED: &str = "deprecation changed";
const KIND_VARIANT_REMOVED: &str = "variant removed";
const KIND_VARIANT_ADDED: &str = "error variant added";
const KIND_CAPABILITY_ADDED: &str = "capability added";
const KIND_CAPABILITY_REMOVED: &str = "capability removed";
const KIND_INPUT_NAME_CHANGED: &str = "capability input parameter name changed";
const KIND_INPUT_LEAF_CHANGED: &str = "capability input type changed";
const KIND_OUTPUT_LEAF_CHANGED: &str = "capability output type changed";
const KIND_ERROR_CHANGED: &str = "capability declared error changed";
const KIND_EXPOSURE_RAISED: &str = "max exposure raised";
const KIND_EXPOSURE_LOWERED: &str = "max exposure lowered";
const KIND_IDEMPOTENCY_STRENGTHENED: &str = "idempotency strengthened";
const KIND_IDEMPOTENCY_WEAKENED: &str = "idempotency weakened";
const KIND_FIELD_ADDED: &str = "field added";
const KIND_FIELD_REMOVED: &str = "field removed";
const KIND_FIELD_TYPE_CHANGED: &str = "field type changed";
const KIND_PAYLOAD_CHANGED: &str = "error payload changed";

/// Migration condition for referenced error-enum variant addition.
const CONDITION_UNKNOWN_VARIANT: &str = "unknown-variant tolerance";

fn fail_closed_finding(base: &SchemaDocument) -> Finding {
    Finding {
        code: CODE_FAIL_CLOSED,
        path: base.box_id.as_str().to_owned(),
        kind: KIND_UNCLASSIFIED,
        class: Class::Incompatible,
        base_excerpt: None,
        submitted_excerpt: None,
        condition: None,
    }
}

fn docs_excerpt(docs: &[String]) -> String {
    docs.join("\n")
}

fn exposure_excerpt(level: ExposureLevel) -> &'static str {
    match level {
        ExposureLevel::CodeOnly => "code_only",
        ExposureLevel::Internal => "internal",
        ExposureLevel::External => "external",
    }
}

fn idempotency_excerpt(value: Idempotency) -> &'static str {
    match value {
        Idempotency::None => "none",
        Idempotency::Inherent => "inherent",
    }
}

fn payload_kind_excerpt(payload: &SchemaPayload) -> &'static str {
    match payload {
        SchemaPayload::Unit => "unit",
        SchemaPayload::Value { .. } => "value",
        SchemaPayload::Named(_) => "named",
    }
}

fn capability_named<'a>(document: &'a SchemaDocument, name: &str) -> Option<&'a SchemaCapability> {
    document
        .capabilities
        .iter()
        .find(|c| c.name.as_str() == name)
}

fn type_named<'a>(document: &'a SchemaDocument, name: &str) -> Option<&'a SchemaType> {
    document.types.iter().find(|t| t.name == name)
}

fn variant_named<'a>(schema_type: &'a SchemaType, name: &str) -> Option<&'a SchemaVariant> {
    schema_type.variants.iter().find(|v| v.name == name)
}

fn field_named<'a>(variant: &'a SchemaVariant, name: &str) -> Option<&'a SchemaField> {
    match &variant.payload {
        SchemaPayload::Named(fields) => fields.iter().find(|f| f.name == name),
        _ => None,
    }
}

/// Applies the D5 type-graph and structural capability taxonomies, then the fail-closed default.
///
/// Named findings are always emitted individually. Unreferenced *additions* (type, variant, or
/// field) fall to the fail-closed default per D5's preamble — a declared type reachable from no
/// capability is not a named additive/conditional row. Documentation, deprecation, and removal rows
/// classify by their D5 table wording regardless of reachability; only addition rows are
/// reachability-gated. Capability additions, removals, input-name changes, input-leaf changes, and
/// output-leaf changes use their named rows. Capability metadata and named payload fields use their
/// D5 rows; capability, type, variant, and field reorders remain fail closed at `<box>`. A
/// revision-only difference (no type or capability delta) yields an empty finding list; `classify`
/// turns that empty result into the D6 check-B integrity error.
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
                    kind: KIND_TYPE_ADDED,
                    class: Class::Additive,
                    base_excerpt: None,
                    submitted_excerpt: Some(name.clone()),
                    condition: None,
                });
            }
            TypeChange::TypeRemoved { name, .. } => {
                findings.push(Finding {
                    code: CODE_TYPE_REMOVED,
                    path: type_path(base, name),
                    kind: KIND_TYPE_REMOVED,
                    class: Class::Incompatible,
                    base_excerpt: Some(name.clone()),
                    submitted_excerpt: None,
                    condition: None,
                });
            }
            TypeChange::TypeDocsChanged { name } => {
                findings.push(Finding {
                    code: CODE_DOCS_CHANGED,
                    path: type_path(base, name),
                    kind: KIND_DOCS_CHANGED,
                    class: Class::Documentation,
                    base_excerpt: type_named(base, name).map(|ty| docs_excerpt(&ty.docs)),
                    submitted_excerpt: type_named(submitted, name).map(|ty| docs_excerpt(&ty.docs)),
                    condition: None,
                });
            }
            TypeChange::TypeDeprecationChanged { name } => {
                findings.push(Finding {
                    code: CODE_DEPRECATION_CHANGED,
                    path: type_path(base, name),
                    kind: KIND_DEPRECATION_CHANGED,
                    class: Class::Deprecation,
                    base_excerpt: type_named(base, name).and_then(|ty| ty.deprecation.clone()),
                    submitted_excerpt: type_named(submitted, name)
                        .and_then(|ty| ty.deprecation.clone()),
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
                    kind: KIND_VARIANT_ADDED,
                    class: Class::CompatibleWithConditions,
                    base_excerpt: None,
                    submitted_excerpt: Some(variant_name.clone()),
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
                    kind: KIND_VARIANT_REMOVED,
                    class: Class::Incompatible,
                    base_excerpt: Some(variant_name.clone()),
                    submitted_excerpt: None,
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
                    kind: KIND_DOCS_CHANGED,
                    class: Class::Documentation,
                    base_excerpt: type_named(base, type_name)
                        .and_then(|ty| variant_named(ty, variant_name))
                        .map(|v| docs_excerpt(&v.docs)),
                    submitted_excerpt: type_named(submitted, type_name)
                        .and_then(|ty| variant_named(ty, variant_name))
                        .map(|v| docs_excerpt(&v.docs)),
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
                    kind: KIND_DEPRECATION_CHANGED,
                    class: Class::Deprecation,
                    base_excerpt: type_named(base, type_name)
                        .and_then(|ty| variant_named(ty, variant_name))
                        .and_then(|v| v.deprecation.clone()),
                    submitted_excerpt: type_named(submitted, type_name)
                        .and_then(|ty| variant_named(ty, variant_name))
                        .and_then(|v| v.deprecation.clone()),
                    condition: None,
                });
            }
            TypeChange::PayloadDocsChanged {
                type_name,
                variant_name,
            }
            | TypeChange::FieldDocsChanged {
                type_name,
                variant_name,
                field_name: _,
            } => {
                let (path, base_excerpt, submitted_excerpt) = match change {
                    TypeChange::FieldDocsChanged { field_name, .. } => (
                        field_path(base, type_name, variant_name, field_name),
                        type_named(base, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .and_then(|v| field_named(v, field_name))
                            .map(|f| docs_excerpt(&f.docs)),
                        type_named(submitted, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .and_then(|v| field_named(v, field_name))
                            .map(|f| docs_excerpt(&f.docs)),
                    ),
                    _ => (
                        variant_path(base, type_name, variant_name),
                        type_named(base, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .map(|v| match &v.payload {
                                SchemaPayload::Value { docs, .. } => docs_excerpt(docs),
                                _ => docs_excerpt(&v.docs),
                            }),
                        type_named(submitted, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .map(|v| match &v.payload {
                                SchemaPayload::Value { docs, .. } => docs_excerpt(docs),
                                _ => docs_excerpt(&v.docs),
                            }),
                    ),
                };
                findings.push(Finding {
                    code: CODE_DOCS_CHANGED,
                    path,
                    kind: KIND_DOCS_CHANGED,
                    class: Class::Documentation,
                    base_excerpt,
                    submitted_excerpt,
                    condition: None,
                });
            }
            TypeChange::PayloadDeprecationChanged {
                type_name,
                variant_name,
            }
            | TypeChange::FieldDeprecationChanged {
                type_name,
                variant_name,
                field_name: _,
            } => {
                let (path, base_excerpt, submitted_excerpt) = match change {
                    TypeChange::FieldDeprecationChanged { field_name, .. } => (
                        field_path(base, type_name, variant_name, field_name),
                        type_named(base, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .and_then(|v| field_named(v, field_name))
                            .and_then(|f| f.deprecation.clone()),
                        type_named(submitted, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .and_then(|v| field_named(v, field_name))
                            .and_then(|f| f.deprecation.clone()),
                    ),
                    _ => (
                        variant_path(base, type_name, variant_name),
                        type_named(base, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .and_then(|v| match &v.payload {
                                SchemaPayload::Value { deprecation, .. } => deprecation.clone(),
                                _ => v.deprecation.clone(),
                            }),
                        type_named(submitted, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .and_then(|v| match &v.payload {
                                SchemaPayload::Value { deprecation, .. } => deprecation.clone(),
                                _ => v.deprecation.clone(),
                            }),
                    ),
                };
                findings.push(Finding {
                    code: CODE_DEPRECATION_CHANGED,
                    path,
                    kind: KIND_DEPRECATION_CHANGED,
                    class: Class::Deprecation,
                    base_excerpt,
                    submitted_excerpt,
                    condition: None,
                });
            }
            TypeChange::FieldAdded {
                type_name,
                variant_name,
                field_name,
                roles,
            } if roles.input || roles.output => findings.push(Finding {
                code: CODE_FIELD_ADDED,
                path: field_path(base, type_name, variant_name, field_name),
                kind: KIND_FIELD_ADDED,
                class: if roles.input {
                    Class::Incompatible
                } else {
                    Class::Additive
                },
                base_excerpt: None,
                submitted_excerpt: Some(field_name.clone()),
                condition: None,
            }),
            TypeChange::FieldRemoved {
                type_name,
                variant_name,
                field_name,
                ..
            } => findings.push(Finding {
                code: CODE_FIELD_REMOVED,
                path: field_path(base, type_name, variant_name, field_name),
                kind: KIND_FIELD_REMOVED,
                class: Class::Incompatible,
                base_excerpt: Some(field_name.clone()),
                submitted_excerpt: None,
                condition: None,
            }),
            TypeChange::FieldTypeChanged {
                type_name,
                variant_name,
                field_name,
            } => findings.push(Finding {
                code: CODE_FIELD_TYPE_CHANGED,
                path: field_path(base, type_name, variant_name, field_name),
                kind: KIND_FIELD_TYPE_CHANGED,
                class: Class::Incompatible,
                base_excerpt: type_named(base, type_name)
                    .and_then(|ty| variant_named(ty, variant_name))
                    .and_then(|v| field_named(v, field_name))
                    .map(|f| f.ty.canonical_name().to_owned()),
                submitted_excerpt: type_named(submitted, type_name)
                    .and_then(|ty| variant_named(ty, variant_name))
                    .and_then(|v| field_named(v, field_name))
                    .map(|f| f.ty.canonical_name().to_owned()),
                condition: None,
            }),
            TypeChange::VariantPayloadChanged { type_name, .. }
            | TypeChange::PayloadTypeChanged { type_name, .. } => {
                let paths = error_paths(base, submitted, type_name);
                if paths.is_empty() {
                    unclassified = true;
                }
                let (base_excerpt, submitted_excerpt) = match change {
                    TypeChange::VariantPayloadChanged { variant_name, .. } => (
                        type_named(base, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .map(|v| payload_kind_excerpt(&v.payload).to_owned()),
                        type_named(submitted, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .map(|v| payload_kind_excerpt(&v.payload).to_owned()),
                    ),
                    TypeChange::PayloadTypeChanged { variant_name, .. } => (
                        type_named(base, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .and_then(|v| match &v.payload {
                                SchemaPayload::Value { ty, .. } => {
                                    Some(ty.canonical_name().to_owned())
                                }
                                _ => None,
                            }),
                        type_named(submitted, type_name)
                            .and_then(|ty| variant_named(ty, variant_name))
                            .and_then(|v| match &v.payload {
                                SchemaPayload::Value { ty, .. } => {
                                    Some(ty.canonical_name().to_owned())
                                }
                                _ => None,
                            }),
                    ),
                    _ => (None, None),
                };
                findings.extend(paths.into_iter().map(|path| Finding {
                    code: CODE_PAYLOAD_CHANGED,
                    path,
                    kind: KIND_PAYLOAD_CHANGED,
                    class: Class::Incompatible,
                    base_excerpt: base_excerpt.clone(),
                    submitted_excerpt: submitted_excerpt.clone(),
                    condition: None,
                }));
            }
            // Unreferenced type/variant/field additions and reorderings have no named row.
            TypeChange::TypeAdded { .. }
            | TypeChange::VariantAdded { .. }
            | TypeChange::TypesReordered
            | TypeChange::VariantsReordered { .. }
            | TypeChange::FieldAdded { .. }
            | TypeChange::FieldsReordered { .. } => {
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
                kind: KIND_CAPABILITY_ADDED,
                class: Class::Additive,
                base_excerpt: None,
                submitted_excerpt: Some(name.clone()),
                condition: None,
            }),
            CapabilityChange::CapabilityRemoved { name } => findings.push(Finding {
                code: CODE_CAPABILITY_REMOVED,
                path: capability_path(base, name),
                kind: KIND_CAPABILITY_REMOVED,
                class: Class::Incompatible,
                base_excerpt: Some(name.clone()),
                submitted_excerpt: None,
                condition: None,
            }),
            CapabilityChange::InputNameChanged { name } => findings.push(Finding {
                code: CODE_INPUT_NAME_CHANGED,
                path: capability_input_path(base, name),
                kind: KIND_INPUT_NAME_CHANGED,
                class: Class::Incompatible,
                base_excerpt: capability_named(base, name).map(|c| c.input.name.clone()),
                submitted_excerpt: capability_named(submitted, name).map(|c| c.input.name.clone()),
                condition: None,
            }),
            CapabilityChange::InputLeafChanged { name } => findings.push(Finding {
                code: CODE_INPUT_LEAF_CHANGED,
                path: capability_input_path(base, name),
                kind: KIND_INPUT_LEAF_CHANGED,
                class: Class::Incompatible,
                base_excerpt: capability_named(base, name)
                    .map(|c| c.input.leaf.canonical_name().to_owned()),
                submitted_excerpt: capability_named(submitted, name)
                    .map(|c| c.input.leaf.canonical_name().to_owned()),
                condition: None,
            }),
            CapabilityChange::OutputLeafChanged { name } => findings.push(Finding {
                code: CODE_OUTPUT_LEAF_CHANGED,
                path: capability_output_path(base, name),
                kind: KIND_OUTPUT_LEAF_CHANGED,
                class: Class::Incompatible,
                base_excerpt: capability_named(base, name)
                    .map(|c| c.output.leaf.canonical_name().to_owned()),
                submitted_excerpt: capability_named(submitted, name)
                    .map(|c| c.output.leaf.canonical_name().to_owned()),
                condition: None,
            }),
            CapabilityChange::CapabilityDocsChanged { name } => findings.push(Finding {
                code: CODE_DOCS_CHANGED,
                path: capability_path(base, name),
                kind: KIND_DOCS_CHANGED,
                class: Class::Documentation,
                base_excerpt: capability_named(base, name).map(|c| docs_excerpt(&c.docs)),
                submitted_excerpt: capability_named(submitted, name).map(|c| docs_excerpt(&c.docs)),
                condition: None,
            }),
            CapabilityChange::CapabilityDeprecationChanged { name } => findings.push(Finding {
                code: CODE_DEPRECATION_CHANGED,
                path: capability_path(base, name),
                kind: KIND_DEPRECATION_CHANGED,
                class: Class::Deprecation,
                base_excerpt: capability_named(base, name).and_then(|c| c.deprecation.clone()),
                submitted_excerpt: capability_named(submitted, name)
                    .and_then(|c| c.deprecation.clone()),
                condition: None,
            }),
            CapabilityChange::CapabilityErrorChanged { name } => findings.push(Finding {
                code: CODE_ERROR_CHANGED,
                path: capability_suffix_path(base, name, "error"),
                kind: KIND_ERROR_CHANGED,
                class: Class::Incompatible,
                base_excerpt: capability_named(base, name).map(|c| c.error.clone()),
                submitted_excerpt: capability_named(submitted, name).map(|c| c.error.clone()),
                condition: None,
            }),
            CapabilityChange::CapabilityExposureChanged {
                name,
                base: base_level,
                submitted: submitted_level,
            } => match exposure_classification(*base_level, *submitted_level) {
                Some((code, class)) => findings.push(Finding {
                    code,
                    path: capability_suffix_path(base, name, "exposure"),
                    kind: if code == CODE_EXPOSURE_RAISED {
                        KIND_EXPOSURE_RAISED
                    } else {
                        KIND_EXPOSURE_LOWERED
                    },
                    class,
                    base_excerpt: Some(exposure_excerpt(*base_level).to_owned()),
                    submitted_excerpt: Some(exposure_excerpt(*submitted_level).to_owned()),
                    condition: None,
                }),
                None => unclassified = true,
            },
            CapabilityChange::CapabilityIdempotencyChanged {
                name,
                base: base_property,
                submitted: submitted_property,
            } => match idempotency_classification(*base_property, *submitted_property) {
                Some((code, class)) => findings.push(Finding {
                    code,
                    path: capability_suffix_path(base, name, "idempotency"),
                    kind: if code == CODE_IDEMPOTENCY_STRENGTHENED {
                        KIND_IDEMPOTENCY_STRENGTHENED
                    } else {
                        KIND_IDEMPOTENCY_WEAKENED
                    },
                    class,
                    base_excerpt: Some(idempotency_excerpt(*base_property).to_owned()),
                    submitted_excerpt: Some(idempotency_excerpt(*submitted_property).to_owned()),
                    condition: None,
                }),
                None => unclassified = true,
            },
            CapabilityChange::CapabilitiesReordered => {
                unclassified = true;
            }
        }
    }

    // Fail-closed default: unmatched type-graph kinds or capability reorderings emit one BXC0028
    // at <box>. Every other capability difference has a named change above. An empty
    // finding list (revision-only) is left empty for classify's check B.
    if unclassified {
        findings.push(fail_closed_finding(base));
    }
    findings
}

fn exposure_classification(
    base: ExposureLevel,
    submitted: ExposureLevel,
) -> Option<(&'static str, Class)> {
    match (base, submitted) {
        (ExposureLevel::CodeOnly, ExposureLevel::Internal | ExposureLevel::External)
        | (ExposureLevel::Internal, ExposureLevel::External) => {
            Some((CODE_EXPOSURE_RAISED, Class::Additive))
        }
        (ExposureLevel::Internal | ExposureLevel::External, ExposureLevel::CodeOnly)
        | (ExposureLevel::External, ExposureLevel::Internal) => {
            Some((CODE_EXPOSURE_LOWERED, Class::Incompatible))
        }
        (ExposureLevel::CodeOnly, ExposureLevel::CodeOnly)
        | (ExposureLevel::Internal, ExposureLevel::Internal)
        | (ExposureLevel::External, ExposureLevel::External) => None,
    }
}

fn idempotency_classification(
    base: Idempotency,
    submitted: Idempotency,
) -> Option<(&'static str, Class)> {
    match (base, submitted) {
        (Idempotency::None, Idempotency::Inherent) => {
            Some((CODE_IDEMPOTENCY_STRENGTHENED, Class::Additive))
        }
        (Idempotency::Inherent, Idempotency::None) => {
            Some((CODE_IDEMPOTENCY_WEAKENED, Class::Incompatible))
        }
        (Idempotency::None, Idempotency::None) | (Idempotency::Inherent, Idempotency::Inherent) => {
            None
        }
    }
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

fn capability_suffix_path(base: &SchemaDocument, name: &str, suffix: &str) -> String {
    [capability_path(base, name).as_str(), "/", suffix].concat()
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

fn field_path(
    base: &SchemaDocument,
    type_name: &str,
    variant_name: &str,
    field_name: &str,
) -> String {
    [
        variant_path(base, type_name, variant_name).as_str(),
        "/field/",
        field_name,
    ]
    .concat()
}

fn error_paths(base: &SchemaDocument, submitted: &SchemaDocument, type_name: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for document in [base, submitted] {
        for capability in &document.capabilities {
            if capability.error == type_name {
                paths.push(capability_suffix_path(
                    base,
                    capability.name.as_str(),
                    "error",
                ));
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
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
    PayloadDocsChanged {
        type_name: String,
        variant_name: String,
    },
    PayloadDeprecationChanged {
        type_name: String,
        variant_name: String,
    },
    PayloadTypeChanged {
        type_name: String,
        variant_name: String,
    },
    FieldAdded {
        type_name: String,
        variant_name: String,
        field_name: String,
        roles: Roles,
    },
    FieldRemoved {
        type_name: String,
        variant_name: String,
        field_name: String,
        roles: Roles,
    },
    FieldDocsChanged {
        type_name: String,
        variant_name: String,
        field_name: String,
    },
    FieldDeprecationChanged {
        type_name: String,
        variant_name: String,
        field_name: String,
    },
    FieldTypeChanged {
        type_name: String,
        variant_name: String,
        field_name: String,
    },
    FieldsReordered {
        type_name: String,
        variant_name: String,
    },
}

#[derive(Debug, Eq, PartialEq)]
enum CapabilityChange {
    CapabilityAdded {
        name: String,
    },
    CapabilityRemoved {
        name: String,
    },
    CapabilitiesReordered,
    CapabilityDocsChanged {
        name: String,
    },
    CapabilityDeprecationChanged {
        name: String,
    },
    CapabilityErrorChanged {
        name: String,
    },
    CapabilityExposureChanged {
        name: String,
        base: ExposureLevel,
        submitted: ExposureLevel,
    },
    CapabilityIdempotencyChanged {
        name: String,
        base: Idempotency,
        submitted: Idempotency,
    },
    InputNameChanged {
        name: String,
    },
    InputLeafChanged {
        name: String,
    },
    OutputLeafChanged {
        name: String,
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
    if base.docs != submitted.docs {
        changes.push(CapabilityChange::CapabilityDocsChanged {
            name: base.name.as_str().to_owned(),
        });
    }
    if base.deprecation != submitted.deprecation {
        changes.push(CapabilityChange::CapabilityDeprecationChanged {
            name: base.name.as_str().to_owned(),
        });
    }
    if base.error != submitted.error {
        changes.push(CapabilityChange::CapabilityErrorChanged {
            name: base.name.as_str().to_owned(),
        });
    }
    if base.max_exposure != submitted.max_exposure {
        changes.push(CapabilityChange::CapabilityExposureChanged {
            name: base.name.as_str().to_owned(),
            base: base.max_exposure,
            submitted: submitted.max_exposure,
        });
    }
    if base.idempotency != submitted.idempotency {
        changes.push(CapabilityChange::CapabilityIdempotencyChanged {
            name: base.name.as_str().to_owned(),
            base: base.idempotency,
            submitted: submitted.idempotency,
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
    match (&base.payload, &submitted.payload) {
        (SchemaPayload::Named(base_fields), SchemaPayload::Named(submitted_fields)) => {
            append_named_field_changes(
                changes,
                type_name,
                base.name.as_str(),
                base_fields,
                submitted_fields,
                roles,
            );
        }
        (
            SchemaPayload::Value {
                docs: base_docs,
                deprecation: base_deprecation,
                ty: base_type,
            },
            SchemaPayload::Value {
                docs: submitted_docs,
                deprecation: submitted_deprecation,
                ty: submitted_type,
            },
        ) => {
            if base_docs != submitted_docs {
                changes.push(TypeChange::PayloadDocsChanged {
                    type_name: type_name.to_owned(),
                    variant_name: base.name.clone(),
                });
            }
            if base_deprecation != submitted_deprecation {
                changes.push(TypeChange::PayloadDeprecationChanged {
                    type_name: type_name.to_owned(),
                    variant_name: base.name.clone(),
                });
            }
            if base_type != submitted_type {
                changes.push(TypeChange::PayloadTypeChanged {
                    type_name: type_name.to_owned(),
                    variant_name: base.name.clone(),
                });
            }
        }
        (base_payload, submitted_payload) if base_payload != submitted_payload => {
            changes.push(TypeChange::VariantPayloadChanged {
                type_name: type_name.to_owned(),
                variant_name: base.name.clone(),
                roles,
            });
        }
        _ => {}
    }
}

fn index_fields(fields: &[SchemaField]) -> BTreeMap<&str, &SchemaField> {
    let mut index = BTreeMap::new();
    for field in fields {
        index.insert(field.name.as_str(), field);
    }
    index
}

fn append_named_field_changes(
    changes: &mut Vec<TypeChange>,
    type_name: &str,
    variant_name: &str,
    base_fields: &[SchemaField],
    submitted_fields: &[SchemaField],
    roles: Roles,
) {
    let base_by_name = index_fields(base_fields);
    let submitted_by_name = index_fields(submitted_fields);

    for base_field in base_fields {
        match submitted_by_name.get(base_field.name.as_str()) {
            None => changes.push(TypeChange::FieldRemoved {
                type_name: type_name.to_owned(),
                variant_name: variant_name.to_owned(),
                field_name: base_field.name.clone(),
                roles,
            }),
            Some(submitted_field) => {
                if base_field.docs != submitted_field.docs {
                    changes.push(TypeChange::FieldDocsChanged {
                        type_name: type_name.to_owned(),
                        variant_name: variant_name.to_owned(),
                        field_name: base_field.name.clone(),
                    });
                }
                if base_field.deprecation != submitted_field.deprecation {
                    changes.push(TypeChange::FieldDeprecationChanged {
                        type_name: type_name.to_owned(),
                        variant_name: variant_name.to_owned(),
                        field_name: base_field.name.clone(),
                    });
                }
                if base_field.ty != submitted_field.ty {
                    changes.push(TypeChange::FieldTypeChanged {
                        type_name: type_name.to_owned(),
                        variant_name: variant_name.to_owned(),
                        field_name: base_field.name.clone(),
                    });
                }
            }
        }
    }

    for submitted_field in submitted_fields {
        if !base_by_name.contains_key(submitted_field.name.as_str()) {
            changes.push(TypeChange::FieldAdded {
                type_name: type_name.to_owned(),
                variant_name: variant_name.to_owned(),
                field_name: submitted_field.name.clone(),
                roles,
            });
        }
    }

    if common_name_sequence_differs(
        base_fields.iter().map(|field| field.name.as_str()),
        submitted_fields.iter().map(|field| field.name.as_str()),
        &base_by_name,
        &submitted_by_name,
    ) {
        changes.push(TypeChange::FieldsReordered {
            type_name: type_name.to_owned(),
            variant_name: variant_name.to_owned(),
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
