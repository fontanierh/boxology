//! Fail-closed hydration of declared imports into validated in-memory import models.
//!
//! [`ImportModel::parse_all`] parses every declared import's checked-in `schema.json` in request
//! order, rejecting anything malformed with coded, payload-safe diagnostics. Emitting the models
//! into the adapter is deferred.

use super::{
    DeclaredImport, Diagnostic, Diagnostics, GenerationRequest, REQUEST_SPAN, RelativePath,
};
use boxology_contract::{BoxId, CapabilityName};
use boxology_contract_syntax::CanonicalType;
use serde_json::Value;
use std::collections::BTreeSet;

const D4: &str = "specs/s2-contract-generator.md D4";
const D3: &str = "specs/s2-contract-generator.md D3";
const OBJECT_RULE: &str = "an imported schema must decode as a JSON object";
const FORMAT_RULE: &str = "an imported schema_format must be the integer 1";
const BOX_ID_RULE: &str = "an imported schema box_id must equal the declared import package";
const SELF_RULE: &str = "a box must not declare an import of itself";
const REVISION_RULE: &str =
    "an imported revision must be \"sha256:\" followed by 64 lowercase hexadecimal digits";
const CAPABILITY_RULE: &str = "each imported capability must declare a unique valid name, its box-qualified id, a unary shape, and known boundary leaves";

#[rustfmt::skip]
const LEAVES: [CanonicalType; 13] = [
    CanonicalType::Bool, CanonicalType::U8, CanonicalType::U16, CanonicalType::U32, CanonicalType::U64,
    CanonicalType::I8, CanonicalType::I16, CanonicalType::I32, CanonicalType::I64,
    CanonicalType::F32, CanonicalType::F64, CanonicalType::String, CanonicalType::Blob,
];

/// One boundary capability offered by an imported package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedCapability {
    name: String,
    input_type: CanonicalType,
    output_type: CanonicalType,
}

impl ImportedCapability {
    /// Returns the capability's box-local name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the capability's input boundary leaf.
    pub fn input_type(&self) -> CanonicalType {
        self.input_type
    }

    /// Returns the capability's output boundary leaf.
    pub fn output_type(&self) -> CanonicalType {
        self.output_type
    }
}

/// One validated foreign package hydrated from its checked-in public schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportModel {
    package: BoxId,
    expected_revision: String,
    capabilities: Vec<ImportedCapability>,
}

impl ImportModel {
    /// Parses every declared import's schema in request order, accumulating all failures.
    ///
    /// # Errors
    /// Returns sorted diagnostics for a schema that is not a JSON object (`BXG0042`), a wrong
    /// `schema_format` (`BXG0043`), a mismatched `box_id` (`BXG0044`), a self-import (`BXG0045`), a
    /// malformed `revision` (`BXG0046`), or an invalid capability entry (`BXG0047`).
    pub fn parse_all(request: &GenerationRequest) -> Result<Vec<Self>, Diagnostics> {
        let mut diagnostics = Vec::new();
        let mut models = Vec::new();
        for import in request.imports() {
            // GenerationRequest::new guarantees the schema input is present; a missing one is an
            // internal invariant violation, so skip it rather than fabricate bytes.
            let Some(input) = request
                .inputs()
                .iter()
                .find(|input| input.path() == import.schema_path())
            else {
                continue;
            };
            if let Some(model) =
                parse_one(request.box_id(), import, input.bytes(), &mut diagnostics)
            {
                models.push(model);
            }
        }
        if diagnostics.is_empty() {
            Ok(models)
        } else {
            diagnostics.sort();
            Err(Diagnostics(diagnostics))
        }
    }

    /// Returns the imported package identity.
    pub fn package(&self) -> &BoxId {
        &self.package
    }

    /// Returns the exact revision the schema declared.
    pub fn expected_revision(&self) -> &str {
        &self.expected_revision
    }

    /// Returns the imported capabilities in schema declaration order.
    pub fn capabilities(&self) -> &[ImportedCapability] {
        &self.capabilities
    }
}

fn parse_one(
    request_box: &BoxId,
    import: &DeclaredImport,
    bytes: &[u8],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ImportModel> {
    let path = import.schema_path();
    let package = import.package();
    let start = diagnostics.len();
    let Ok(Value::Object(object)) = serde_json::from_slice::<Value>(bytes) else {
        emit(diagnostics, path, package, "BXG0042", "schema");
        return None;
    };
    if package == request_box {
        emit(diagnostics, path, package, "BXG0045", "self-import");
    }
    if object.get("schema_format").and_then(Value::as_u64) != Some(1) {
        emit(diagnostics, path, package, "BXG0043", "schema_format");
    }
    if object.get("box_id").and_then(Value::as_str) != Some(package.as_str()) {
        emit(diagnostics, path, package, "BXG0044", "box_id");
    }
    let revision = object.get("revision").and_then(Value::as_str);
    if !revision.is_some_and(is_valid_revision) {
        emit(diagnostics, path, package, "BXG0046", "revision");
    }
    let capabilities = parse_capabilities(package, &object, path, diagnostics);
    (diagnostics.len() == start).then(|| ImportModel {
        package: package.clone(),
        expected_revision: revision
            .expect("a clean import carries a revision")
            .to_owned(),
        capabilities,
    })
}

fn parse_capabilities(
    package: &BoxId,
    object: &serde_json::Map<String, Value>,
    path: &RelativePath,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ImportedCapability> {
    let mut capabilities = Vec::new();
    let entries = match object.get("capabilities") {
        Some(Value::Array(entries)) if !entries.is_empty() => entries,
        _ => {
            emit(diagnostics, path, package, "BXG0047", "capabilities");
            return capabilities;
        }
    };
    let mut seen = BTreeSet::new();
    for entry in entries {
        let Value::Object(entry) = entry else {
            emit(diagnostics, path, package, "BXG0047", "capability entry");
            continue;
        };
        let Some(name) = entry
            .get("name")
            .and_then(Value::as_str)
            .and_then(|name| CapabilityName::new(name).ok())
        else {
            emit(diagnostics, path, package, "BXG0047", "capability name");
            continue;
        };
        if !seen.insert(name.as_str().to_owned()) {
            emit(diagnostics, path, package, "BXG0047", "duplicate name");
            continue;
        }
        let expected_id = format!("{}.{}", package.as_str(), name.as_str());
        let id_ok = entry.get("id").and_then(Value::as_str) == Some(expected_id.as_str());
        let shape_ok = entry.get("shape").and_then(Value::as_str) == Some("unary");
        let input = leaf_type(entry.get("input"));
        let output = leaf_type(entry.get("output"));
        if !id_ok {
            emit(diagnostics, path, package, "BXG0047", "capability id");
        }
        if !shape_ok {
            emit(diagnostics, path, package, "BXG0047", "capability shape");
        }
        if input.is_none() {
            emit(diagnostics, path, package, "BXG0047", "input type");
        }
        if output.is_none() {
            emit(diagnostics, path, package, "BXG0047", "output type");
        }
        if let (true, true, Some(input_type), Some(output_type)) = (id_ok, shape_ok, input, output)
        {
            capabilities.push(ImportedCapability {
                name: name.as_str().to_owned(),
                input_type,
                output_type,
            });
        }
    }
    capabilities
}

/// Maps a slot's schema `type` spelling back to its canonical boundary leaf.
fn leaf_type(slot: Option<&Value>) -> Option<CanonicalType> {
    let name = slot?.get("type")?.as_str()?;
    LEAVES
        .into_iter()
        .find(|leaf| leaf.canonical_name() == name)
}

fn is_valid_revision(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    })
}

/// Pushes one payload-safe import diagnostic; `rule` and source follow the `code`.
fn emit(
    diagnostics: &mut Vec<Diagnostic>,
    path: &RelativePath,
    package: &BoxId,
    code: &'static str,
    detail: &str,
) {
    let (rule, rule_source) = match code {
        "BXG0042" => (OBJECT_RULE, D4),
        "BXG0043" => (FORMAT_RULE, D4),
        "BXG0044" => (BOX_ID_RULE, D4),
        "BXG0045" => (SELF_RULE, D3),
        "BXG0046" => (REVISION_RULE, D4),
        _ => (CAPABILITY_RULE, D4),
    };
    diagnostics.push(Diagnostic {
        path: path.clone(),
        span: REQUEST_SPAN,
        code,
        offending: format!("import {package} {detail}"),
        rule,
        rule_source,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const REVISION: &str =
        "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";

    fn id(value: &str) -> BoxId {
        BoxId::new(value).unwrap()
    }

    fn valid_schema(box_id: &str, capability: &str) -> String {
        format!(
            "{{ \"box_id\": \"{box_id}\", \"capabilities\": [ {{ \"id\": \"{box_id}.{capability}\", \
             \"input\": {{ \"name\": \"name\", \"type\": \"String\" }}, \"name\": \"{capability}\", \
             \"output\": {{ \"type\": \"String\" }}, \"shape\": \"unary\" }} ], \
             \"revision\": \"{REVISION}\", \"schema_format\": 1 }}"
        )
    }

    /// Builds a request for box `box_id` importing each `(package, schema_path, bytes)`.
    fn request(box_id: &str, imports: &[(&str, &str, &str)]) -> GenerationRequest {
        let mut inputs = vec![
            ("boxology.toml".to_owned(), b"manifest".to_vec()),
            ("src/lib.rs".to_owned(), b"fn entry() {}\n".to_vec()),
        ];
        let mut declared = Vec::new();
        for (package, schema_path, bytes) in imports {
            inputs.push(((*schema_path).to_owned(), bytes.as_bytes().to_vec()));
            declared.push((id(package), (*schema_path).to_owned()));
        }
        GenerationRequest::new(id(box_id), "src/lib.rs".into(), inputs, declared, vec![]).unwrap()
    }

    /// Parses a single-import request and asserts the one diagnostic's code, path, and Display line.
    fn assert_line(box_id: &str, schema: &str, code: &str, line: &str) {
        let request = request(box_id, &[("hello", "imports/hello.json", schema)]);
        let diagnostics = ImportModel::parse_all(&request).unwrap_err();
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one diagnostic, got {diagnostics:?}");
        };
        assert_eq!(diagnostic.code(), code);
        assert_eq!(diagnostic.path().as_str(), "imports/hello.json");
        assert_eq!(diagnostic.to_string(), line);
    }

    #[test]
    fn valid_import_hydrates_one_model_with_ordered_capabilities() {
        let schema = valid_schema("hello", "greet");
        let request = request("greeter", &[("hello", "imports/hello.json", &schema)]);
        let models = ImportModel::parse_all(&request).unwrap();
        let [model] = models.as_slice() else {
            panic!("expected one model, got {models:?}");
        };
        assert_eq!(model.package().as_str(), "hello");
        assert_eq!(model.expected_revision(), REVISION);
        let [capability] = model.capabilities() else {
            panic!("expected one capability");
        };
        assert_eq!(capability.name(), "greet");
        assert_eq!(capability.input_type(), CanonicalType::String);
        assert_eq!(capability.output_type(), CanonicalType::String);
    }

    #[test]
    fn two_imports_hydrate_in_request_order() {
        let alpha = valid_schema("alpha", "one");
        let beta = valid_schema("beta", "two");
        let imports = [
            ("beta", "imports/beta.json", beta.as_str()),
            ("alpha", "imports/alpha.json", alpha.as_str()),
        ];
        let models = ImportModel::parse_all(&request("greeter", &imports)).unwrap();
        let packages: Vec<_> = models.iter().map(|m| m.package().as_str()).collect();
        assert_eq!(packages, ["beta", "alpha"]);
    }

    #[test]
    fn non_object_schema_is_bxg0042() {
        let line = "BXG0042 imports/hello.json:1:1-1:1 offending=\"import hello schema\" rule=\"an imported schema must decode as a JSON object\" source=\"specs/s2-contract-generator.md D4\"";
        assert_line("greeter", "[]", "BXG0042", line);
    }

    #[test]
    fn wrong_schema_format_is_bxg0043() {
        let schema =
            valid_schema("hello", "greet").replace("\"schema_format\": 1", "\"schema_format\": 2");
        let line = "BXG0043 imports/hello.json:1:1-1:1 offending=\"import hello schema_format\" rule=\"an imported schema_format must be the integer 1\" source=\"specs/s2-contract-generator.md D4\"";
        assert_line("greeter", &schema, "BXG0043", line);
    }

    #[test]
    fn mismatched_box_id_is_bxg0044() {
        let schema = valid_schema("hello", "greet")
            .replace("\"box_id\": \"hello\"", "\"box_id\": \"other\"");
        let line = "BXG0044 imports/hello.json:1:1-1:1 offending=\"import hello box_id\" rule=\"an imported schema box_id must equal the declared import package\" source=\"specs/s2-contract-generator.md D4\"";
        assert_line("greeter", &schema, "BXG0044", line);
    }

    #[test]
    fn self_import_is_bxg0045() {
        let schema = valid_schema("hello", "greet");
        let line = "BXG0045 imports/hello.json:1:1-1:1 offending=\"import hello self-import\" rule=\"a box must not declare an import of itself\" source=\"specs/s2-contract-generator.md D3\"";
        assert_line("hello", &schema, "BXG0045", line);
    }

    #[test]
    fn malformed_revision_is_bxg0046() {
        let schema = valid_schema("hello", "greet").replace(REVISION, "sha256:not-hex");
        let line = "BXG0046 imports/hello.json:1:1-1:1 offending=\"import hello revision\" rule=\"an imported revision must be \\\"sha256:\\\" followed by 64 lowercase hexadecimal digits\" source=\"specs/s2-contract-generator.md D4\"";
        assert_line("greeter", &schema, "BXG0046", line);
    }

    #[test]
    fn invalid_capability_is_bxg0047() {
        let schema =
            valid_schema("hello", "greet").replace("\"shape\": \"unary\"", "\"shape\": \"stream\"");
        let line = "BXG0047 imports/hello.json:1:1-1:1 offending=\"import hello capability shape\" rule=\"each imported capability must declare a unique valid name, its box-qualified id, a unary shape, and known boundary leaves\" source=\"specs/s2-contract-generator.md D4\"";
        assert_line("greeter", &schema, "BXG0047", line);
    }

    /// Wraps `caps` as a `hello` schema's `capabilities` value, keeping the other fields valid.
    fn schema_with_capabilities(caps: &str) -> String {
        format!(
            "{{ \"box_id\": \"hello\", \"capabilities\": {caps}, \"revision\": \"{REVISION}\", \
             \"schema_format\": 1 }}"
        )
    }

    /// Spells one otherwise-valid `hello.{name}` capability entry as its JSON object.
    fn valid_capability(name: &str) -> String {
        format!(
            "{{ \"id\": \"hello.{name}\", \"input\": {{ \"name\": \"name\", \"type\": \"String\" }}, \
             \"name\": \"{name}\", \"output\": {{ \"type\": \"String\" }}, \"shape\": \"unary\" }}"
        )
    }

    /// Parses a single-import `hello` schema and asserts its one BXG0047 carries `detail`.
    fn assert_bxg0047(schema: &str, detail: &str) {
        let request = request("greeter", &[("hello", "imports/hello.json", schema)]);
        let diagnostics = ImportModel::parse_all(&request).unwrap_err();
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one diagnostic, got {diagnostics:?}");
        };
        assert_eq!(diagnostic.code(), "BXG0047");
        assert_eq!(
            diagnostic.offending_construct(),
            format!("import hello {detail}")
        );
    }

    #[test]
    fn empty_capabilities_array_is_bxg0047() {
        assert_bxg0047(&schema_with_capabilities("[]"), "capabilities");
    }

    #[test]
    fn non_object_capability_entry_is_bxg0047() {
        assert_bxg0047(&schema_with_capabilities("[ 1 ]"), "capability entry");
    }

    #[test]
    fn missing_capability_name_is_bxg0047() {
        let entry = valid_capability("greet").replace(" \"name\": \"greet\",", "");
        assert_bxg0047(
            &schema_with_capabilities(&format!("[ {entry} ]")),
            "capability name",
        );
    }

    #[test]
    fn duplicate_capability_name_is_bxg0047() {
        let caps = format!(
            "[ {}, {} ]",
            valid_capability("greet"),
            valid_capability("greet")
        );
        assert_bxg0047(&schema_with_capabilities(&caps), "duplicate name");
    }

    #[test]
    fn mismatched_capability_id_is_bxg0047() {
        let entry = valid_capability("greet").replace("\"hello.greet\"", "\"hello.wrong\"");
        assert_bxg0047(
            &schema_with_capabilities(&format!("[ {entry} ]")),
            "capability id",
        );
    }

    #[test]
    fn unknown_input_leaf_is_bxg0047() {
        let entry = valid_capability("greet").replace(
            "\"name\": \"name\", \"type\": \"String\"",
            "\"name\": \"name\", \"type\": \"u128\"",
        );
        assert_bxg0047(
            &schema_with_capabilities(&format!("[ {entry} ]")),
            "input type",
        );
    }

    #[test]
    fn unknown_output_leaf_is_bxg0047() {
        let entry = valid_capability("greet").replace(
            "\"output\": { \"type\": \"String\" }",
            "\"output\": { \"type\": \"usize\" }",
        );
        assert_bxg0047(
            &schema_with_capabilities(&format!("[ {entry} ]")),
            "output type",
        );
    }

    #[test]
    fn full_emitter_schema_with_provenance_and_types_hydrates() {
        // The full emitter schema carries provenance, a types map, and the complete per-capability
        // field set (deprecation/docs/error/idempotency/max_exposure) the parser ignores. It must
        // still hydrate exactly one model, proving the parser stays emitter-compatible.
        let schema = format!(
            "{{ \"box_id\": \"hello\", \"capabilities\": [ {{ \"deprecation\": null, \
             \"docs\": [], \"error\": \"GreetError\", \"id\": \"hello.greet\", \
             \"idempotency\": \"none\", \"input\": {{ \"name\": \"name\", \"type\": \"String\" }}, \
             \"max_exposure\": \"external\", \"name\": \"greet\", \
             \"output\": {{ \"type\": \"String\" }}, \"shape\": \"unary\" }} ], \
             \"provenance\": {{ \"generator\": \"boxology-generator\", \
             \"generator_version\": \"0.0.0\", \"semantic_digest\": \"sha256:00\" }}, \
             \"revision\": \"{REVISION}\", \"schema_format\": 1, \
             \"types\": {{ \"GreetError\": {{ \"kind\": \"error\" }} }} }}"
        );
        let request = request("greeter", &[("hello", "imports/hello.json", &schema)]);
        let models = ImportModel::parse_all(&request).unwrap();
        let [model] = models.as_slice() else {
            panic!("expected one model, got {models:?}");
        };
        assert_eq!(model.package().as_str(), "hello");
        let [capability] = model.capabilities() else {
            panic!("expected one capability");
        };
        assert_eq!(capability.name(), "greet");
        assert_eq!(capability.input_type(), CanonicalType::String);
        assert_eq!(capability.output_type(), CanonicalType::String);
    }
}
