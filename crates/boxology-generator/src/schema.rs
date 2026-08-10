use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use boxology_contract_syntax::{
    CanonicalType, CapabilityDeclaration, Contract, DataDeclaration, DataShape, ErrorVariant,
    TypeExpression as ParsedTypeExpression, VariantField, VariantPayload, VariantValue,
    exposure_spelling, idempotency_spelling,
};
use boxology_schema::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SchemaCapability, SchemaDataField,
    SchemaDataShape, SchemaDataType, SchemaDataVariant, SchemaDocument, SchemaField, SchemaPayload,
    SchemaType, SchemaVariant, Shape, TypeExpression,
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

/// Recursively lowers a validated boundary expression into generated descriptor source.
///
/// Local names expand to their declaration shape because runtime descriptors are structural. The
/// controlled grammar admits references only to earlier declarations, so expansion is acyclic.
/// `runtime_prefix` preserves each emitter site's established `TypeDescriptor` spelling.
pub(super) fn type_descriptor_source(
    contract: &Contract,
    expression: &ParsedTypeExpression,
    runtime_prefix: &str,
) -> String {
    match expression {
        ParsedTypeExpression::Leaf(leaf) => format!(
            "{runtime_prefix}TypeDescriptor::{}()",
            descriptor_constructor(*leaf)
        ),
        ParsedTypeExpression::Local(name) => {
            let declaration = contract
                .data
                .iter()
                .find(|declaration| declaration.name == *name)
                .expect("validated local type reference names a declaration");
            data_descriptor_source(contract, declaration, runtime_prefix)
        }
        ParsedTypeExpression::Option(inner) => format!(
            "{runtime_prefix}TypeDescriptor::optional({}).expect(\"generated optional descriptor is valid\")",
            type_descriptor_source(contract, inner, runtime_prefix)
        ),
        ParsedTypeExpression::Vec(inner) => format!(
            "{runtime_prefix}TypeDescriptor::list({}).expect(\"generated list descriptor is valid\")",
            type_descriptor_source(contract, inner, runtime_prefix)
        ),
    }
}

fn data_descriptor_source(
    contract: &Contract,
    declaration: &DataDeclaration,
    runtime_prefix: &str,
) -> String {
    match &declaration.shape {
        DataShape::Struct(fields) => {
            let fields = fields
                .iter()
                .map(|field| {
                    format!(
                        "::boxology_contract::FieldDescriptor::new({name:?}, {descriptor}, {deprecation}),",
                        name = field.name,
                        descriptor = type_descriptor_source(contract, &field.ty, runtime_prefix),
                        deprecation = descriptor_deprecation(&field.deprecation),
                    )
                })
                .collect::<String>();
            format!(
                "{runtime_prefix}TypeDescriptor::structure([{fields}]).expect(\"generated struct descriptor is valid\")"
            )
        }
        DataShape::Enum(variants) => {
            let variants = variants
                .iter()
                .map(|variant| {
                    format!(
                        "::boxology_contract::VariantDescriptor::new({name:?}, ::boxology_contract::VariantPayload::Unit, {deprecation}),",
                        name = variant.name,
                        deprecation = descriptor_deprecation(&variant.deprecation),
                    )
                })
                .collect::<String>();
            format!(
                "{runtime_prefix}TypeDescriptor::enumeration([{variants}]).expect(\"generated enum descriptor is valid\")"
            )
        }
    }
}

fn descriptor_deprecation(note: &Option<String>) -> String {
    match note {
        None => "None".into(),
        Some(note) if note.is_empty() => "Some(::boxology_contract::Deprecation::new(None))".into(),
        Some(note) => format!(
            "Some(::boxology_contract::Deprecation::new(Some({note:?}.into())))",
            note = note,
        ),
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
        data_types: contract.data.iter().map(data_type).collect(),
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
                    payload: payload(&variant.payload),
                })
                .collect(),
        }],
    }
    .canonical_bytes()
}

fn data_type(declaration: &DataDeclaration) -> SchemaDataType {
    let shape = match &declaration.shape {
        DataShape::Struct(fields) => SchemaDataShape::Struct(
            fields
                .iter()
                .map(|field| SchemaDataField {
                    name: field.name.clone(),
                    docs: field.docs.clone(),
                    deprecation: field.deprecation.clone(),
                    ty: type_expression(&field.ty),
                })
                .collect(),
        ),
        DataShape::Enum(variants) => SchemaDataShape::Enum(
            variants
                .iter()
                .map(|variant| SchemaDataVariant {
                    name: variant.name.clone(),
                    docs: variant.docs.clone(),
                    deprecation: variant.deprecation.clone(),
                })
                .collect(),
        ),
    };
    SchemaDataType {
        name: declaration.name.clone(),
        docs: declaration.docs.clone(),
        deprecation: declaration.deprecation.clone(),
        shape,
    }
}

fn type_expression(expression: &ParsedTypeExpression) -> TypeExpression {
    match expression {
        ParsedTypeExpression::Leaf(value) => leaf(*value),
        ParsedTypeExpression::Local(name) => TypeExpression::Local(name.clone()),
        ParsedTypeExpression::Option(inner) => {
            TypeExpression::Option(Box::new(type_expression(inner)))
        }
        ParsedTypeExpression::Vec(inner) => TypeExpression::Vec(Box::new(type_expression(inner))),
    }
}

/// Maps a parsed variant payload onto the schema vocabulary.
fn payload(payload: &VariantPayload) -> SchemaPayload {
    match payload {
        VariantPayload::Unit => SchemaPayload::Unit,
        VariantPayload::Value(VariantValue {
            docs,
            deprecation,
            ty,
        }) => SchemaPayload::Value {
            docs: docs.clone(),
            deprecation: deprecation.clone(),
            ty: leaf(*ty),
        },
        VariantPayload::Named(fields) => SchemaPayload::Named(
            fields
                .iter()
                .map(
                    |VariantField {
                         docs,
                         deprecation,
                         name,
                         ty,
                     }| SchemaField {
                        docs: docs.clone(),
                        deprecation: deprecation.clone(),
                        name: name.clone(),
                        ty: leaf(*ty),
                    },
                )
                .collect(),
        ),
    }
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
            leaf: type_expression(&capability.input_type),
        },
        output: OutputSlot {
            leaf: type_expression(&capability.output_type),
        },
        shape: Shape::Unary,
        max_exposure: capability.exposure,
        idempotency: capability.idempotency,
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

/// Emits one `VariantDescriptor::new(...)` call for a generated error variant.
///
/// One-value payloads lower to `VariantPayload::Value(TypeDescriptor::{leaf}())`. Named payloads
/// remain BXG0048-gated: the gate is the only thing keeping this honest for named shapes. When
/// #104's named slice lifts that gate, this helper must learn named payloads in the same change.
pub(super) fn variant_descriptor_source(variant: &ErrorVariant) -> String {
    let payload = match &variant.payload {
        VariantPayload::Unit => "::boxology_contract::VariantPayload::Unit".to_owned(),
        VariantPayload::Value(value) => format!(
            "::boxology_contract::VariantPayload::Value(::boxology_contract::TypeDescriptor::{}())",
            descriptor_constructor(value.ty)
        ),
        VariantPayload::Named(_) => {
            unreachable!("named payloads remain BXG0048-gated and must not reach emission")
        }
    };
    format!(
        "::boxology_contract::VariantDescriptor::new({name:?}, {payload}, {deprecation}),",
        name = variant.name,
        deprecation = rust_deprecation(&variant.deprecation),
    )
}

pub(super) fn descriptor_source(box_id: &str, contract: &Contract, revision: &[u8; 32]) -> String {
    let error = &contract.error;
    let variants = error
        .variants
        .iter()
        .map(variant_descriptor_source)
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
                expression =
                    capability_expression(contract, "box_id.clone()", capability, error_expr),
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
    contract: &Contract,
    box_id_expr: &str,
    capability: &CapabilityDeclaration,
    error_expr: &str,
) -> String {
    format!(
        "::boxology_contract::CapabilityDescriptor::new(::boxology_contract::CapabilityId::new({box_id_expr}, ::boxology_contract::CapabilityName::new({name:?}).expect(\"generated capability name is valid\")), {input_descriptor}, {output_descriptor}, {error_expr}, ::boxology_contract::CapabilityShape::Unary, {exposure}, {idempotency}, {deprecation},)",
        box_id_expr = box_id_expr,
        name = capability.name,
        input_descriptor =
            type_descriptor_source(contract, &capability.input_type, "::boxology_contract::"),
        output_descriptor =
            type_descriptor_source(contract, &capability.output_type, "::boxology_contract::"),
        error_expr = error_expr,
        exposure = exposure_token(capability.exposure),
        idempotency = idempotency_token(capability.idempotency),
        deprecation = rust_deprecation(&capability.deprecation),
    )
}

fn exposure_token(level: ExposureLevel) -> &'static str {
    match level {
        ExposureLevel::CodeOnly => "::boxology_contract::ExposureLevel::CodeOnly",
        ExposureLevel::Internal => "::boxology_contract::ExposureLevel::Internal",
        ExposureLevel::External => "::boxology_contract::ExposureLevel::External",
    }
}

fn idempotency_token(value: Idempotency) -> &'static str {
    match value {
        Idempotency::None => "::boxology_contract::Idempotency::None",
        Idempotency::Inherent => "::boxology_contract::Idempotency::Inherent",
    }
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
    count(&mut out, contract.data.len() + 1);
    for declaration in &contract.data {
        out.push(match declaration.shape {
            DataShape::Struct(_) => 0x03,
            DataShape::Enum(_) => 0x04,
        });
        string(&mut out, &declaration.name);
        metadata(&mut out, &declaration.docs, &declaration.deprecation);
        match &declaration.shape {
            DataShape::Struct(fields) => {
                count(&mut out, fields.len());
                for field in fields {
                    string(&mut out, &field.name);
                    metadata(&mut out, &field.docs, &field.deprecation);
                    string(&mut out, &field.ty.canonical_spelling());
                }
            }
            DataShape::Enum(variants) => {
                count(&mut out, variants.len());
                for variant in variants {
                    string(&mut out, &variant.name);
                    metadata(&mut out, &variant.docs, &variant.deprecation);
                }
            }
        }
    }
    out.push(1); // error declaration
    let error = &contract.error;
    string(&mut out, &error.name);
    metadata(&mut out, &error.docs, &error.deprecation);
    count(&mut out, error.variants.len());
    for variant in &error.variants {
        string(&mut out, &variant.name);
        metadata(&mut out, &variant.docs, &variant.deprecation);
        match &variant.payload {
            VariantPayload::Unit => out.push(0x00),
            VariantPayload::Value(VariantValue {
                docs,
                deprecation,
                ty,
            }) => {
                out.push(0x01);
                metadata(&mut out, docs, deprecation);
                string(&mut out, ty.canonical_name());
            }
            VariantPayload::Named(fields) => {
                out.push(0x02);
                count(&mut out, fields.len());
                for field in fields {
                    metadata(&mut out, &field.docs, &field.deprecation);
                    string(&mut out, &field.name);
                    string(&mut out, field.ty.canonical_name());
                }
            }
        }
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
        let input_type = capability.input_type.canonical_spelling();
        let output_type = capability.output_type.canonical_spelling();
        for value in [
            capability.input_name.as_str(),
            input_type.as_str(),
            output_type.as_str(),
            capability.error.as_str(),
            "unary",
            exposure_spelling(capability.exposure),
            idempotency_spelling(capability.idempotency),
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
    use boxology_contract_syntax::{
        ErrorDeclaration, ErrorVariant, VariantField, VariantPayload, VariantValue,
    };

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

    fn structured() -> Contract {
        boxology_contract_syntax::parse(
            r#"
            #[doc = "mood docs"] pub enum Mood {
                #[deprecated(note = "quiet")] Calm, Busy
            }
            #[deprecated(note = "legacy")] pub struct Profile {
                #[doc = "name docs"] pub name: String,
                pub scores: Vec<u32>,
                pub mood: Option<Mood>,
                #[deprecated] pub history: Option<Vec<Mood>>
            }
            #[error] pub enum Fault { Bad }
            #[capability(exposure = external)]
            pub async fn save(input: Profile) -> Result<Option<Vec<Profile>>, Fault>;
            "#
            .parse()
            .unwrap(),
        )
        .unwrap()
    }

    fn struct_fields(contract: &mut Contract) -> &mut Vec<boxology_contract_syntax::DataField> {
        let DataShape::Struct(fields) = &mut contract.data[1].shape else {
            panic!("struct")
        };
        fields
    }

    fn enum_variants(contract: &mut Contract) -> &mut Vec<boxology_contract_syntax::DataVariant> {
        let DataShape::Enum(variants) = &mut contract.data[0].shape else {
            panic!("enum")
        };
        variants
    }

    #[test]
    fn structured_schema_mapping_projection_and_mutations_are_exact() {
        let contract = structured();
        let bytes = document("profiles", &contract, &[7; 32], &[8; 32], "1.2.3");
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["schema_format"], 1);
        assert_eq!(value["capabilities"][0]["input"]["type"], "Profile");
        assert_eq!(
            value["capabilities"][0]["output"]["type"],
            "Option<Vec<Profile>>"
        );
        assert_eq!(
            value["types"],
            json!([
                {"kind":"enum","name":"Mood","docs":["mood docs"],"deprecation":null,"variants":[
                    {"name":"Calm","docs":[],"deprecation":{"note":"quiet"}},
                    {"name":"Busy","docs":[],"deprecation":null}
                ]},
                {"kind":"struct","name":"Profile","docs":[],"deprecation":{"note":"legacy"},"fields":[
                    {"name":"name","docs":["name docs"],"deprecation":null,"type":"String"},
                    {"name":"scores","docs":[],"deprecation":null,"type":"Vec<u32>"},
                    {"name":"mood","docs":[],"deprecation":null,"type":"Option<Mood>"},
                    {"name":"history","docs":[],"deprecation":{"note":""},"type":"Option<Vec<Mood>>"}
                ]},
                {"kind":"error","name":"Fault","docs":[],"deprecation":null,"variants":[
                    {"name":"Bad","docs":[],"deprecation":null,"payload":"unit"}
                ]}
            ])
        );
        let baseline_projection = projection("profiles", &contract);
        assert_eq!(
            baseline_projection
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            "626f786f6c6f67792e7075626c69632d636f6e74726163742d7265766973696f6e0000000001000000000000000870726f66696c657300000000000000030400000000000000044d6f6f64000000000000000100000000000000096d6f6f6420646f6373000000000000000002000000000000000443616c6d0000000000000000010000000000000005717569657400000000000000044275737900000000000000000003000000000000000750726f66696c6500000000000000000100000000000000066c6567616379000000000000000400000000000000046e616d65000000000000000100000000000000096e616d6520646f6373000000000000000006537472696e67000000000000000673636f72657300000000000000000000000000000000085665633c7533323e00000000000000046d6f6f64000000000000000000000000000000000c4f7074696f6e3c4d6f6f643e0000000000000007686973746f7279000000000000000001000000000000000000000000000000114f7074696f6e3c5665633c4d6f6f643e3e0100000000000000054661756c7400000000000000000000000000000000010000000000000003426164000000000000000000000000000000000001000000000000000d70726f66696c65732e736176650000000000000004736176650000000000000000000000000000000005696e707574000000000000000750726f66696c6500000000000000144f7074696f6e3c5665633c50726f66696c653e3e00000000000000054661756c740000000000000005756e617279000000000000000865787465726e616c00000000000000046e6f6e65"
        );
        assert_eq!(
            hash_spelling(&revision("profiles", &contract)),
            "sha256:78f1a8009cff2b47cd17309c138c1ad08a43f90fd9c0905a97e9c38d289f89ad"
        );

        let mutations: &[fn(&mut Contract)] = &[
            |c| c.data.swap(0, 1),
            |c| {
                c.data.push(DataDeclaration {
                    docs: Vec::new(),
                    deprecation: None,
                    name: "Archive".into(),
                    shape: DataShape::Struct(Vec::new()),
                });
            },
            |c| c.data[0].shape = DataShape::Struct(Vec::new()),
            |c| c.data[0].name = "Feeling".into(),
            |c| c.data[0].docs.push("more".into()),
            |c| c.data[0].deprecation = Some("old".into()),
            |c| enum_variants(c).swap(0, 1),
            |c| {
                enum_variants(c).pop().unwrap();
            },
            |c| enum_variants(c)[0].name = "Still".into(),
            |c| enum_variants(c)[0].docs.push("calm".into()),
            |c| enum_variants(c)[0].deprecation = None,
            |c| struct_fields(c).swap(0, 1),
            |c| {
                struct_fields(c).pop().unwrap();
            },
            |c| struct_fields(c)[0].name = "label".into(),
            |c| struct_fields(c)[0].docs.push("more".into()),
            |c| struct_fields(c)[0].deprecation = Some("old".into()),
            |c| struct_fields(c)[0].ty = CanonicalType::Bool.into(),
            |c| c.capabilities[0].input_type = CanonicalType::U32.into(),
            |c| c.capabilities[0].output_type = CanonicalType::Bool.into(),
        ];
        let baseline_revision = revision("profiles", &contract);
        for (index, mutate) in mutations.iter().enumerate() {
            let mut changed = contract.clone();
            mutate(&mut changed);
            assert_ne!(
                projection("profiles", &changed),
                baseline_projection,
                "mutation {index}"
            );
            assert_ne!(
                revision("profiles", &changed),
                baseline_revision,
                "mutation {index}"
            );
        }
    }

    fn unary(name: &str, exposure: ExposureLevel, idempotency: Idempotency) -> Contract {
        Contract {
            data: vec![],
            error: ErrorDeclaration {
                docs: Vec::new(),
                deprecation: None,
                name: "E".to_owned(),
                variants: vec![ErrorVariant {
                    docs: Vec::new(),
                    deprecation: None,
                    name: "V".to_owned(),
                    payload: VariantPayload::Unit,
                }],
            },
            capabilities: vec![CapabilityDeclaration {
                docs: Vec::new(),
                deprecation: None,
                name: name.to_owned(),
                input_name: "n".to_owned(),
                input_type: CanonicalType::String.into(),
                output_type: CanonicalType::String.into(),
                error: "E".to_owned(),
                exposure,
                idempotency,
            }],
        }
    }

    /// Document, descriptor source, and public-revision projection all follow the model.
    #[test]
    fn model_driven_exposure_and_idempotency_emission() {
        #[rustfmt::skip]
        let cases = [
            (ExposureLevel::CodeOnly, Idempotency::None, "CodeOnly", "None"),
            (ExposureLevel::CodeOnly, Idempotency::Inherent, "CodeOnly", "Inherent"),
            (ExposureLevel::Internal, Idempotency::None, "Internal", "None"),
            (ExposureLevel::Internal, Idempotency::Inherent, "Internal", "Inherent"),
            (ExposureLevel::External, Idempotency::None, "External", "None"),
            (ExposureLevel::External, Idempotency::Inherent, "External", "Inherent"),
        ];
        let base = unary("g", ExposureLevel::External, Idempotency::None);
        let base_revision = revision("box", &base);
        for (exposure, idempotency, exposure_token, idempotency_token) in cases {
            let contract = unary("g", exposure, idempotency);
            let mapped = capability(&contract.capabilities[0]);
            assert_eq!(mapped.max_exposure, exposure);
            assert_eq!(mapped.idempotency, idempotency);
            let source = descriptor_source("box", &contract, &[0; 32]);
            assert!(
                source.contains(&format!("ExposureLevel::{exposure_token}")),
                "{source}"
            );
            assert!(
                source.contains(&format!("Idempotency::{idempotency_token}")),
                "{source}"
            );
            if (exposure, idempotency) != (ExposureLevel::External, Idempotency::None) {
                assert_ne!(base_revision, revision("box", &contract));
            }
        }
        let mut renamed = base.clone();
        renamed.capabilities[0].name = "rescued".to_owned();
        assert_ne!(base_revision, revision("box", &renamed));
        const PINNED: &str = "626f786f6c6f67792e7075626c69632d636f6e74726163742d7265766973696f6e00000000010000000000000003626f7800000000000000010100000000000000014500000000000000000000000000000000010000000000000001560000000000000000000000000000000000010000000000000005626f782e6700000000000000016700000000000000000000000000000000016e0000000000000006537472696e670000000000000006537472696e670000000000000001450000000000000005756e6172790000000000000008696e7465726e616c0000000000000008696e686572656e74";
        assert_eq!(
            projection(
                "box",
                &unary("g", ExposureLevel::Internal, Idempotency::Inherent)
            )
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
            PINNED
        );
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
            data: vec![],
            error: ErrorDeclaration {
                docs: vec!["Why storing fails.".to_owned()],
                deprecation: Some("use StoreFault".to_owned()),
                name: "StoreError".to_owned(),
                variants: vec![
                    ErrorVariant {
                        docs: vec!["No such key.".to_owned()],
                        deprecation: None,
                        name: "Missing".to_owned(),
                        payload: VariantPayload::Unit,
                    },
                    ErrorVariant {
                        docs: Vec::new(),
                        deprecation: Some(String::new()),
                        name: "Denied".to_owned(),
                        payload: VariantPayload::Unit,
                    },
                ],
            },
            capabilities: vec![CapabilityDeclaration {
                docs: vec!["Stores a value.".to_owned(), "Second line.".to_owned()],
                deprecation: Some("use insert".to_owned()),
                name: "put".to_owned(),
                input_name: "value".to_owned(),
                input_type: CanonicalType::U32.into(),
                output_type: CanonicalType::Bool.into(),
                error: "StoreError".to_owned(),
                exposure: ExposureLevel::External,
                idempotency: Idempotency::None,
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
            data_types: Vec::new(),
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
                        payload: SchemaPayload::Unit,
                    },
                    SchemaVariant {
                        name: "Denied".to_owned(),
                        docs: Vec::new(),
                        deprecation: Some(String::new()),
                        payload: SchemaPayload::Unit,
                    },
                ],
            }],
        };
        assert_eq!(
            document("store", &contract, &[4; 32], &[9; 32], "1.2.3"),
            expected.canonical_bytes()
        );
    }

    /// Pins the payload mapping for all three shapes, including the empty-named ≠ unit distinction
    /// and declaration-order field preservation.
    #[test]
    fn document_maps_every_payload_shape() {
        let contract = Contract {
            data: vec![],
            error: ErrorDeclaration {
                docs: Vec::new(),
                deprecation: None,
                name: "PayloadError".to_owned(),
                variants: vec![
                    ErrorVariant {
                        docs: Vec::new(),
                        deprecation: None,
                        name: "Unit".to_owned(),
                        payload: VariantPayload::Unit,
                    },
                    ErrorVariant {
                        docs: vec!["value variant".to_owned()],
                        deprecation: None,
                        name: "Value".to_owned(),
                        payload: VariantPayload::Value(VariantValue {
                            docs: vec!["value payload".to_owned()],
                            deprecation: Some("use detail".to_owned()),
                            ty: CanonicalType::U32,
                        }),
                    },
                    ErrorVariant {
                        docs: Vec::new(),
                        deprecation: Some("retired".to_owned()),
                        name: "Named".to_owned(),
                        payload: VariantPayload::Named(vec![
                            VariantField {
                                docs: vec!["message field".to_owned()],
                                deprecation: None,
                                name: "message".to_owned(),
                                ty: CanonicalType::String,
                            },
                            VariantField {
                                docs: Vec::new(),
                                deprecation: Some("use text".to_owned()),
                                name: "code".to_owned(),
                                ty: CanonicalType::I64,
                            },
                        ]),
                    },
                    ErrorVariant {
                        docs: Vec::new(),
                        deprecation: None,
                        name: "EmptyNamed".to_owned(),
                        payload: VariantPayload::Named(Vec::new()),
                    },
                ],
            },
            capabilities: vec![CapabilityDeclaration {
                docs: Vec::new(),
                deprecation: None,
                name: "inspect".to_owned(),
                input_name: "name".to_owned(),
                input_type: CanonicalType::String.into(),
                output_type: CanonicalType::String.into(),
                error: "PayloadError".to_owned(),
                exposure: ExposureLevel::External,
                idempotency: Idempotency::None,
            }],
        };
        let expected = SchemaDocument {
            box_id: BoxId::new("payloads").unwrap(),
            capabilities: vec![SchemaCapability {
                name: CapabilityName::new("inspect").unwrap(),
                docs: Vec::new(),
                deprecation: None,
                error: "PayloadError".to_owned(),
                input: InputSlot {
                    name: "name".to_owned(),
                    leaf: BoundaryLeaf::String,
                },
                output: OutputSlot {
                    leaf: BoundaryLeaf::String,
                },
                shape: Shape::Unary,
                max_exposure: ExposureLevel::External,
                idempotency: Idempotency::None,
            }],
            data_types: Vec::new(),
            provenance: Provenance::new(json!({
                "generator": "boxology-generator",
                "generator_version": "0.0.0",
                "semantic_digest": hash_spelling(&[1; 32]),
            })),
            revision: hash_spelling(&[2; 32]),
            types: vec![SchemaType {
                name: "PayloadError".to_owned(),
                docs: Vec::new(),
                deprecation: None,
                variants: vec![
                    SchemaVariant {
                        name: "Unit".to_owned(),
                        docs: Vec::new(),
                        deprecation: None,
                        payload: SchemaPayload::Unit,
                    },
                    SchemaVariant {
                        name: "Value".to_owned(),
                        docs: vec!["value variant".to_owned()],
                        deprecation: None,
                        payload: SchemaPayload::Value {
                            docs: vec!["value payload".to_owned()],
                            deprecation: Some("use detail".to_owned()),
                            ty: BoundaryLeaf::U32,
                        },
                    },
                    SchemaVariant {
                        name: "Named".to_owned(),
                        docs: Vec::new(),
                        deprecation: Some("retired".to_owned()),
                        payload: SchemaPayload::Named(vec![
                            SchemaField {
                                docs: vec!["message field".to_owned()],
                                deprecation: None,
                                name: "message".to_owned(),
                                ty: BoundaryLeaf::String,
                            },
                            SchemaField {
                                docs: Vec::new(),
                                deprecation: Some("use text".to_owned()),
                                name: "code".to_owned(),
                                ty: BoundaryLeaf::I64,
                            },
                        ]),
                    },
                    SchemaVariant {
                        name: "EmptyNamed".to_owned(),
                        docs: Vec::new(),
                        deprecation: None,
                        payload: SchemaPayload::Named(Vec::new()),
                    },
                ],
            }],
        };
        assert_ne!(SchemaPayload::Unit, SchemaPayload::Named(Vec::new()));
        assert_eq!(
            document("payloads", &contract, &[2; 32], &[1; 32], "0.0.0"),
            expected.canonical_bytes()
        );
    }
}
