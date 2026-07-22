use boxology_contract_syntax::{Contract, ErrorDeclaration, ErrorVariant};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const REVISION_DOMAIN: &[u8] = b"boxology.public-contract-revision\0";
const REVISION_VERSION: u32 = 1;

pub(super) fn document(
    box_id: &str,
    contract: &Contract,
    semantic_digest: &[u8; 32],
    generator_version: &str,
) -> Vec<u8> {
    let revision = revision(box_id, contract);
    let capability = &contract.capability;
    let root = object([
        ("box_id", json!(box_id)),
        (
            "capabilities",
            json!([object([
                ("deprecation", deprecation(&capability.deprecation)),
                ("docs", json!(capability.docs)),
                ("error", json!(capability.error)),
                ("id", json!(format!("{box_id}.{}", capability.name))),
                ("idempotency", json!("none")),
                (
                    "input",
                    object([
                        ("name", json!(capability.input_name)),
                        ("type", json!("String"))
                    ]),
                ),
                ("max_exposure", json!("external")),
                ("name", json!(capability.name)),
                ("output", object([("type", json!("String"))])),
                ("shape", json!("unary")),
            ])]),
        ),
        (
            "provenance",
            object([
                ("generator", json!("boxology-generator")),
                ("generator_version", json!(generator_version)),
                ("semantic_digest", json!(hash_spelling(semantic_digest))),
            ]),
        ),
        ("revision", json!(hash_spelling(&revision))),
        ("schema_format", json!(1)),
        ("types", json!([error_type(&contract.error)])),
    ]);
    let mut bytes = serde_json::to_vec_pretty(&root).expect("schema values are serializable");
    bytes.push(b'\n');
    bytes
}

fn error_type(error: &ErrorDeclaration) -> Value {
    object([
        ("deprecation", deprecation(&error.deprecation)),
        ("docs", json!(error.docs)),
        ("kind", json!("error")),
        ("name", json!(error.name)),
        (
            "variants",
            Value::Array(error.variants.iter().map(error_variant).collect()),
        ),
    ])
}

fn error_variant(variant: &ErrorVariant) -> Value {
    object([
        ("deprecation", deprecation(&variant.deprecation)),
        ("docs", json!(variant.docs)),
        ("name", json!(variant.name)),
        ("payload", json!("unit")),
    ])
}

fn object<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect(),
    )
}

fn deprecation(note: &Option<String>) -> Value {
    note.as_ref()
        .map(|note| object([("note", json!(note))]))
        .unwrap_or(Value::Null)
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
    count(&mut out, 1);
    let capability = &contract.capability;
    for value in [
        format!("{box_id}.{}", capability.name),
        capability.name.clone(),
    ] {
        string(&mut out, &value);
    }
    metadata(&mut out, &capability.docs, &capability.deprecation);
    for value in [
        capability.input_name.as_str(),
        "String",
        "String",
        &capability.error,
        "unary",
        "external",
        "none",
    ] {
        string(&mut out, value);
    }
    out
}

fn revision(box_id: &str, contract: &Contract) -> [u8; 32] {
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
