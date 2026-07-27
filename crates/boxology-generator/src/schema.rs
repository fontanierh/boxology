use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use boxology_contract_syntax::{CanonicalType, CapabilityDeclaration, Contract};
use boxology_schema::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SchemaCapability, SchemaDocument, SchemaType,
    SchemaVariant, Shape,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const REVISION_DOMAIN: &[u8] = b"boxology.public-contract-revision\0";
const REVISION_VERSION: u32 = 1;

/// Maps a canonical boundary leaf to its `TypeDescriptor` constructor name.
///
/// `String` and `Blob` use the lowercase runtime constructors `string`/`blob`; every scalar
/// leaf already spells its lowercase constructor via `canonical_name()` (e.g. `u32`, `bool`).
pub(super) fn descriptor_constructor(leaf: CanonicalType) -> &'static str {
    match leaf {
        CanonicalType::String => "string",
        CanonicalType::Blob => "blob",
        other => other.canonical_name(),
    }
}

/// Maps one controlled contract onto the shared schema model and emits its canonical bytes.
///
/// The mapping is the whole of this function: `boxology-schema` owns the encoding, so the emit side
/// of the format-1 codec has exactly one implementation (S4 D1).
pub(super) fn document(
    box_id: &str,
    contract: &Contract,
    revision: &[u8; 32],
    semantic_digest: &[u8; 32],
    generator_version: &str,
) -> Vec<u8> {
    let error = &contract.error;
    SchemaDocument {
        box_id: BoxId::new(box_id).expect("generated box identity is valid"),
        capabilities: contract.capabilities.iter().map(capability).collect(),
        provenance: Provenance::new(json!({
            "generator": "boxology-generator",
            "generator_version": generator_version,
            "semantic_digest": hash_spelling(semantic_digest),
        })),
        revision: hash_spelling(revision),
        types: vec![SchemaType {
            name: error.name.clone(),
            docs: error.docs.clone(),
            deprecation: error.deprecation.clone(),
            variants: error
                .variants
                .iter()
                .map(|variant| SchemaVariant {
                    name: variant.name.clone(),
                    docs: variant.docs.clone(),
                    deprecation: variant.deprecation.clone(),
                })
                .collect(),
        }],
    }
    .canonical_bytes()
}

fn capability(capability: &CapabilityDeclaration) -> SchemaCapability {
    SchemaCapability {
        name: CapabilityName::new(capability.name.clone())
            .expect("the shared parser validates capability identity grammar"),
        docs: capability.docs.clone(),
        deprecation: capability.deprecation.clone(),
        error: capability.error.clone(),
        input: InputSlot {
            name: capability.input_name.clone(),
            leaf: leaf(capability.input_type),
        },
        output: OutputSlot {
            leaf: leaf(capability.output_type),
        },
        shape: Shape::Unary,
        // The controlled grammar admits exactly `exposure = external` and no idempotency
        // attribute at all — the declaration's own `exposure`/`idempotency` fields are those two
        // constants — so these are the only values format 1 has ever emitted.
        max_exposure: ExposureLevel::External,
        idempotency: Idempotency::None,
    }
}

/// Maps a parsed boundary leaf onto the schema vocabulary; both enumerations spell the same 13.
fn leaf(leaf: CanonicalType) -> BoundaryLeaf {
    match leaf {
        CanonicalType::Bool => BoundaryLeaf::Bool,
        CanonicalType::U8 => BoundaryLeaf::U8,
        CanonicalType::U16 => BoundaryLeaf::U16,
        CanonicalType::U32 => BoundaryLeaf::U32,
        CanonicalType::U64 => BoundaryLeaf::U64,
        CanonicalType::I8 => BoundaryLeaf::I8,
        CanonicalType::I16 => BoundaryLeaf::I16,
        CanonicalType::I32 => BoundaryLeaf::I32,
        CanonicalType::I64 => BoundaryLeaf::I64,
        CanonicalType::F32 => BoundaryLeaf::F32,
        CanonicalType::F64 => BoundaryLeaf::F64,
        CanonicalType::String => BoundaryLeaf::String,
        CanonicalType::Blob => BoundaryLeaf::Blob,
    }
}

pub(super) fn descriptor_source(box_id: &str, contract: &Contract, revision: &[u8; 32]) -> String {
    let error = &contract.error;
    let variants = error
        .variants
        .iter()
        .map(|variant| {
            format!(
                "::boxology_contract::VariantDescriptor::new({name:?}, ::boxology_contract::VariantPayload::Unit, {deprecation}),",
                name = variant.name,
                deprecation = rust_deprecation(&variant.deprecation),
            )
        })
        .collect::<String>();
    // Every capability binds `box_id.clone()` and MUST run before `ContractDescriptor::new`
    // moves `box_id`. At a single capability the binding name, error move, and array are the
    // exact tokens emitted before this generalization, so the frozen Hello output is unchanged.
    let single = contract.capabilities.len() == 1;
    let bindings = contract
        .capabilities
        .iter()
        .enumerate()
        .map(|(index, capability)| {
            let binding = if single {
                "capability".to_owned()
            } else {
                format!("capability_{index}")
            };
            let error_expr = if single { "error" } else { "error.clone()" };
            format!(
                "let {binding} = {expression};",
                binding = binding,
                expression = capability_expression("box_id.clone()", capability, error_expr),
            )
        })
        .collect::<String>();
    let array = if single {
        "[capability]".to_owned()
    } else {
        format!(
            "[{}]",
            (0..contract.capabilities.len())
                .map(|index| format!("capability_{index}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let revision = hash_spelling(revision);
    format!(
        r#"
        #[doc(hidden)]
        static __BOXOLOGY_CONTRACT_DESCRIPTOR: ::std::sync::LazyLock<::boxology_contract::ContractDescriptor> = ::std::sync::LazyLock::new(|| {{
            let box_id = ::boxology_contract::BoxId::new({box_id:?})
                .expect("generated box identity is valid");
            let error = ::boxology_contract::TypeDescriptor::enumeration([
                {variants}
            ])
            .expect("generated error descriptor is valid");
            {bindings}
            ::boxology_contract::ContractDescriptor::new(
                box_id,
                {array},
                ::boxology_contract::ContractRevision::new({revision:?})
                    .expect("generated contract revision is non-empty"),
            )
            .expect("generated contract descriptor is valid")
        }});

        /// Returns the canonical generated contract descriptor.
        pub fn contract_descriptor() -> &'static ::boxology_contract::ContractDescriptor {{
            &__BOXOLOGY_CONTRACT_DESCRIPTOR
        }}
        "#,
        box_id = box_id,
        variants = variants,
        bindings = bindings,
        array = array,
        revision = revision,
    )
}

/// Emits one `CapabilityDescriptor::new(...)` expression with the exact tokens the single-capability
/// descriptor emitted before generalization. `box_id_expr` names the moved-or-cloned box identity and
/// `error_expr` names the error descriptor (a move at one capability, a clone when several share it).
fn capability_expression(
    box_id_expr: &str,
    capability: &CapabilityDeclaration,
    error_expr: &str,
) -> String {
    format!(
        "::boxology_contract::CapabilityDescriptor::new(::boxology_contract::CapabilityId::new({box_id_expr}, ::boxology_contract::CapabilityName::new({name:?}).expect(\"generated capability name is valid\")), ::boxology_contract::TypeDescriptor::{input_constructor}(), ::boxology_contract::TypeDescriptor::{output_constructor}(), {error_expr}, ::boxology_contract::CapabilityShape::Unary, ::boxology_contract::ExposureLevel::External, ::boxology_contract::Idempotency::None, {deprecation},)",
        box_id_expr = box_id_expr,
        name = capability.name,
        input_constructor = descriptor_constructor(capability.input_type),
        output_constructor = descriptor_constructor(capability.output_type),
        error_expr = error_expr,
        deprecation = rust_deprecation(&capability.deprecation),
    )
}

fn rust_deprecation(note: &Option<String>) -> String {
    match note {
        None => "None".into(),
        Some(note) if note.is_empty() => "Some(::boxology_contract::Deprecation::new(None))".into(),
        Some(note) => format!(
            "Some(::boxology_contract::Deprecation::new(Some({note:?}.into())))",
            note = note,
        ),
    }
}

fn hash_spelling(hash: &[u8; 32]) -> String {
    format!(
        "sha256:{}",
        hash.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

pub(super) fn projection(box_id: &str, contract: &Contract) -> Vec<u8> {
    let mut out = REVISION_DOMAIN.to_vec();
    out.extend_from_slice(&REVISION_VERSION.to_be_bytes());
    string(&mut out, box_id);
    count(&mut out, 1);
    out.push(1); // error declaration
    let error = &contract.error;
    string(&mut out, &error.name);
    metadata(&mut out, &error.docs, &error.deprecation);
    count(&mut out, error.variants.len());
    for variant in &error.variants {
        string(&mut out, &variant.name);
        metadata(&mut out, &variant.docs, &variant.deprecation);
        out.push(0); // unit payload
    }
    count(&mut out, contract.capabilities.len());
    for capability in &contract.capabilities {
        for value in [
            format!("{box_id}.{}", capability.name),
            capability.name.clone(),
        ] {
            string(&mut out, &value);
        }
        metadata(&mut out, &capability.docs, &capability.deprecation);
        for value in [
            capability.input_name.as_str(),
            capability.input_type.canonical_name(),
            capability.output_type.canonical_name(),
            &capability.error,
            "unary",
            "external",
            "none",
        ] {
            string(&mut out, value);
        }
    }
    out
}

pub(super) fn revision(box_id: &str, contract: &Contract) -> [u8; 32] {
    Sha256::digest(projection(box_id, contract)).into()
}

fn metadata(out: &mut Vec<u8>, docs: &[String], deprecation: &Option<String>) {
    count(out, docs.len());
    for doc in docs {
        string(out, doc);
    }
    match deprecation {
        None => out.push(0),
        Some(note) => {
            out.push(1);
            string(out, note);
        }
    }
}

fn string(out: &mut Vec<u8>, value: &str) {
    count(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

fn count(out: &mut Vec<u8>, value: usize) {
    out.extend_from_slice(&(value as u64).to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract_syntax::{ErrorDeclaration, ErrorVariant};

    /// Pins all thirteen arms of the parsed-leaf to schema-leaf translation by naming the pairs.
    /// Only five leaves reach a document in any other test, so a transposed arm on the other eight
    /// — `u8` mapped to `bool`, say — would ship silently as a cross-version wire incompatibility.
    /// Pairs, not spelling equality: the two `canonical_name`s name different domains, the exact
    /// Rust identifier and the wire vocabulary, and `descriptor_constructor` above already shows
    /// them diverging, so a future wire respelling must not turn this test red.
    #[test]
    fn every_boundary_leaf_maps_to_its_named_schema_leaf() {
        use BoundaryLeaf as Wire;
        use CanonicalType as Parsed;
        #[rustfmt::skip]
        let pairs = [
            (Parsed::Bool, Wire::Bool), (Parsed::U8, Wire::U8), (Parsed::U16, Wire::U16),
            (Parsed::U32, Wire::U32), (Parsed::U64, Wire::U64), (Parsed::I8, Wire::I8),
            (Parsed::I16, Wire::I16), (Parsed::I32, Wire::I32), (Parsed::I64, Wire::I64),
            (Parsed::F32, Wire::F32), (Parsed::F64, Wire::F64), (Parsed::String, Wire::String),
            (Parsed::Blob, Wire::Blob),
        ];
        assert_eq!(pairs.len(), 13);
        for (parsed, wire) in pairs {
            assert_eq!(leaf(parsed), wire);
        }
    }

    /// `capability` hardcodes `external`/`none` instead of reading the declaration's own
    /// `exposure`/`idempotency`, and may only do so while those are the sole values the controlled
    /// grammar admits. This pins that premise from the parser's side, so widening the grammar
    /// turns a test red here rather than silently flattening a new value onto `external`.
    #[test]
    fn the_grammar_admits_only_the_exposure_and_idempotency_the_mapping_hardcodes() {
        let source = |marker| {
            format!(
                "#[error] pub enum E {{ V }} #[capability({marker})] \
                 pub async fn g(n:String)->Result<String,E>;"
            )
        };
        let contract: Contract = syn::parse_str(&source("exposure=external")).unwrap();
        let declared = &contract.capabilities[0];
        assert_eq!(declared.exposure, "external");
        assert_eq!(declared.idempotency, "none");
        let mapped = capability(declared);
        assert_eq!(mapped.max_exposure, ExposureLevel::External);
        assert_eq!(mapped.idempotency, Idempotency::None);
        for widened in [
            "exposure=internal",
            "exposure=code_only",
            "idempotency=inherent",
        ] {
            let rejected = syn::parse_str::<Contract>(&source(widened)).unwrap_err();
            assert_eq!(rejected.to_string(), "exposure must be external");
        }
    }

    /// Pins the seam: `document` is a mapping onto `boxology-schema` and nothing else, so a later
    /// re-optimization cannot quietly fork the emit side of the codec back into two authorities.
    /// The contract below deliberately carries what the Hello golden does not — docs, a deprecated
    /// capability, a deprecated type, a bare `#[deprecated]` variant, non-`String` boundary leaves,
    /// and the only `generator_version` other than `"0.0.0"` anywhere. Both sides run through
    /// `canonical_bytes`, so it pins the `Contract` to `SchemaDocument` mapping and can never
    /// catch a serializer bug.
    #[test]
    fn document_bytes_are_the_model_bytes() {
        let contract = Contract {
            error: ErrorDeclaration {
                docs: vec!["Why storing fails.".to_owned()],
                deprecation: Some("use StoreFault".to_owned()),
                name: "StoreError".to_owned(),
                variants: vec![
                    ErrorVariant {
                        docs: vec!["No such key.".to_owned()],
                        deprecation: None,
                        name: "Missing".to_owned(),
                    },
                    ErrorVariant {
                        docs: Vec::new(),
                        deprecation: Some(String::new()),
                        name: "Denied".to_owned(),
                    },
                ],
            },
            capabilities: vec![CapabilityDeclaration {
                docs: vec!["Stores a value.".to_owned(), "Second line.".to_owned()],
                deprecation: Some("use insert".to_owned()),
                name: "put".to_owned(),
                input_name: "value".to_owned(),
                input_type: CanonicalType::U32,
                output_type: CanonicalType::Bool,
                error: "StoreError".to_owned(),
                exposure: "external",
                idempotency: "none",
            }],
        };
        let expected = SchemaDocument {
            box_id: BoxId::new("store").unwrap(),
            capabilities: vec![SchemaCapability {
                name: CapabilityName::new("put").unwrap(),
                docs: vec!["Stores a value.".to_owned(), "Second line.".to_owned()],
                deprecation: Some("use insert".to_owned()),
                error: "StoreError".to_owned(),
                input: InputSlot {
                    name: "value".to_owned(),
                    leaf: BoundaryLeaf::U32,
                },
                output: OutputSlot {
                    leaf: BoundaryLeaf::Bool,
                },
                shape: Shape::Unary,
                max_exposure: ExposureLevel::External,
                idempotency: Idempotency::None,
            }],
            provenance: Provenance::new(json!({
                "generator": "boxology-generator",
                "generator_version": "1.2.3",
                "semantic_digest": hash_spelling(&[9; 32]),
            })),
            revision: hash_spelling(&[4; 32]),
            types: vec![SchemaType {
                name: "StoreError".to_owned(),
                docs: vec!["Why storing fails.".to_owned()],
                deprecation: Some("use StoreFault".to_owned()),
                variants: vec![
                    SchemaVariant {
                        name: "Missing".to_owned(),
                        docs: vec!["No such key.".to_owned()],
                        deprecation: None,
                    },
                    SchemaVariant {
                        name: "Denied".to_owned(),
                        docs: Vec::new(),
                        deprecation: Some(String::new()),
                    },
                ],
            }],
        };
        assert_eq!(
            document("store", &contract, &[4; 32], &[9; 32], "1.2.3"),
            expected.canonical_bytes()
        );
    }
}
