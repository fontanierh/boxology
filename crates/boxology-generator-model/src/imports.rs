//! Fail-closed hydration of declared imports into validated in-memory import models.
//!
//! [`ImportModel::parse_all`] parses every declared import's checked-in `schema.json` in request
//! order, rejecting anything malformed with coded, payload-safe diagnostics. Emitting the models
//! into the adapter is deferred.

use super::{
    DeclaredImport, Diagnostic, DiagnosticCode, Diagnostics, GenerationRequest, REQUEST_SPAN,
    RelativePath,
};
use boxology_contract::BoxId;
use boxology_contract_syntax::{
    CanonicalType, DataDeclaration, DataField, DataShape, DataVariant, TypeExpression,
};
use boxology_schema::{
    SchemaDataField, SchemaDataShape, SchemaDataType, SchemaDataVariant, SchemaDocument,
    TypeExpression as SchemaTypeExpression,
};
use serde_json::Value;

const D4: &str = "specs/s2-contract-generator.md D4";
const D3: &str = "specs/s2-contract-generator.md D3";
const OBJECT_RULE: &str = "an imported schema must decode as a JSON object";
const FORMAT_RULE: &str = "an imported schema_format must be the integer 1";
const BOX_ID_RULE: &str = "an imported schema box_id must equal the declared import package";
const SELF_RULE: &str = "a box must not declare an import of itself";
const REVISION_RULE: &str =
    "an imported revision must be \"sha256:\" followed by 64 lowercase hexadecimal digits";
const CAPABILITY_RULE: &str = "an imported schema must be a strict format-1 contract whose unary capabilities use the supported structured boundary subset";

/// One boundary capability offered by an imported package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedCapability {
    name: String,
    input_type: TypeExpression,
    output_type: TypeExpression,
}

impl ImportedCapability {
    /// Returns the capability's box-local name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the capability's input boundary expression.
    pub fn input_type(&self) -> &TypeExpression {
        &self.input_type
    }

    /// Returns the capability's output boundary expression.
    pub fn output_type(&self) -> &TypeExpression {
        &self.output_type
    }
}

/// One validated foreign package hydrated from its checked-in public schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportModel {
    package: BoxId,
    expected_revision: String,
    declarations: Vec<DataDeclaration>,
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

    /// Returns provider-owned structured declarations in schema order.
    pub fn declarations(&self) -> &[DataDeclaration] {
        &self.declarations
    }

    /// Returns the imported capabilities in schema declaration order.
    pub fn capabilities(&self) -> &[ImportedCapability] {
        &self.capabilities
    }
}

#[rustfmt::skip]
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
        emit(diagnostics, path, package, DiagnosticCode::Bxg0042, "schema");
        return None;
    };
    if package == request_box {
        emit(diagnostics, path, package, DiagnosticCode::Bxg0045, "self-import");
    }
    if object.get("schema_format").and_then(Value::as_u64) != Some(1) {
        emit(diagnostics, path, package, DiagnosticCode::Bxg0043, "schema_format");
    }
    if object.get("box_id").and_then(Value::as_str) != Some(package.as_str()) {
        emit(diagnostics, path, package, DiagnosticCode::Bxg0044, "box_id");
    }
    let revision = object.get("revision").and_then(Value::as_str);
    if !revision.is_some_and(is_valid_revision) {
        emit(diagnostics, path, package, DiagnosticCode::Bxg0046, "revision");
    }
    if diagnostics.len() != start {
        return None;
    }
    let Ok(document) = SchemaDocument::parse(bytes) else {
        emit(diagnostics, path, package, DiagnosticCode::Bxg0047, "contract surface");
        return None;
    };
    if document.capabilities.is_empty() {
        emit(diagnostics, path, package, DiagnosticCode::Bxg0047, "contract surface");
        return None;
    }
    Some(ImportModel {
        package: package.clone(),
        expected_revision: revision
            .expect("a clean import carries a revision")
            .to_owned(),
        declarations: document.data_types.into_iter().map(data_declaration).collect(),
        capabilities: document.capabilities.into_iter().map(|capability| ImportedCapability {
            name: capability.name.as_str().to_owned(),
            input_type: type_expression(capability.input.leaf),
            output_type: type_expression(capability.output.leaf),
        }).collect(),
    })
}

fn data_declaration(declaration: SchemaDataType) -> DataDeclaration {
    DataDeclaration {
        docs: declaration.docs,
        deprecation: declaration.deprecation,
        name: declaration.name,
        shape: match declaration.shape {
            SchemaDataShape::Struct(fields) => {
                DataShape::Struct(fields.into_iter().map(data_field).collect())
            }
            SchemaDataShape::Enum(variants) => {
                DataShape::Enum(variants.into_iter().map(data_variant).collect())
            }
        },
    }
}

fn data_field(field: SchemaDataField) -> DataField {
    DataField {
        docs: field.docs,
        deprecation: field.deprecation,
        name: field.name,
        ty: type_expression(field.ty),
    }
}

fn data_variant(variant: SchemaDataVariant) -> DataVariant {
    DataVariant {
        docs: variant.docs,
        deprecation: variant.deprecation,
        name: variant.name,
    }
}

fn type_expression(expression: SchemaTypeExpression) -> TypeExpression {
    match expression {
        SchemaTypeExpression::Bool => CanonicalType::Bool.into(),
        SchemaTypeExpression::U8 => CanonicalType::U8.into(),
        SchemaTypeExpression::U16 => CanonicalType::U16.into(),
        SchemaTypeExpression::U32 => CanonicalType::U32.into(),
        SchemaTypeExpression::U64 => CanonicalType::U64.into(),
        SchemaTypeExpression::I8 => CanonicalType::I8.into(),
        SchemaTypeExpression::I16 => CanonicalType::I16.into(),
        SchemaTypeExpression::I32 => CanonicalType::I32.into(),
        SchemaTypeExpression::I64 => CanonicalType::I64.into(),
        SchemaTypeExpression::F32 => CanonicalType::F32.into(),
        SchemaTypeExpression::F64 => CanonicalType::F64.into(),
        SchemaTypeExpression::String => CanonicalType::String.into(),
        SchemaTypeExpression::Blob => CanonicalType::Blob.into(),
        SchemaTypeExpression::Local(name) => TypeExpression::Local(name),
        SchemaTypeExpression::Option(inner) => {
            TypeExpression::Option(Box::new(type_expression(*inner)))
        }
        SchemaTypeExpression::Vec(inner) => TypeExpression::Vec(Box::new(type_expression(*inner))),
    }
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
#[rustfmt::skip]
fn emit(
    diagnostics: &mut Vec<Diagnostic>,
    path: &RelativePath,
    package: &BoxId,
    code: DiagnosticCode,
    detail: &str,
) {
    let (rule, rule_source) = match code {
        DiagnosticCode::Bxg0042 => (OBJECT_RULE, D4),
        DiagnosticCode::Bxg0043 => (FORMAT_RULE, D4),
        DiagnosticCode::Bxg0044 => (BOX_ID_RULE, D4),
        DiagnosticCode::Bxg0045 => (SELF_RULE, D3),
        DiagnosticCode::Bxg0046 => (REVISION_RULE, D4),
        DiagnosticCode::Bxg0047 => (CAPABILITY_RULE, D4),
        DiagnosticCode::Bxg0001 | DiagnosticCode::Bxg0002 | DiagnosticCode::Bxg0003 | DiagnosticCode::Bxg0004 | DiagnosticCode::Bxg0005 | DiagnosticCode::Bxg0006 | DiagnosticCode::Bxg0007 | DiagnosticCode::Bxg0008 | DiagnosticCode::Bxg0009
        | DiagnosticCode::Bxg0010 | DiagnosticCode::Bxg0011 | DiagnosticCode::Bxg0012 | DiagnosticCode::Bxg0013 | DiagnosticCode::Bxg0014 | DiagnosticCode::Bxg0015 | DiagnosticCode::Bxg0016 | DiagnosticCode::Bxg0017 | DiagnosticCode::Bxg0018 | DiagnosticCode::Bxg0019
        | DiagnosticCode::Bxg0020 | DiagnosticCode::Bxg0021 | DiagnosticCode::Bxg0022 | DiagnosticCode::Bxg0023 | DiagnosticCode::Bxg0024 | DiagnosticCode::Bxg0025 | DiagnosticCode::Bxg0026 | DiagnosticCode::Bxg0027 | DiagnosticCode::Bxg0028 | DiagnosticCode::Bxg0029
        | DiagnosticCode::Bxg0030 | DiagnosticCode::Bxg0031 | DiagnosticCode::Bxg0032 | DiagnosticCode::Bxg0033 | DiagnosticCode::Bxg0034 | DiagnosticCode::Bxg0035 | DiagnosticCode::Bxg0036 | DiagnosticCode::Bxg0037 | DiagnosticCode::Bxg0038 | DiagnosticCode::Bxg0039 | DiagnosticCode::Bxg0040 | DiagnosticCode::Bxg0048 => {
            unreachable!("import diagnostics accept only BXG0042-BXG0047")
        }
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

    #[test]
    fn revision_matches_the_checked_in_hello_schema() {
        let schema =
            std::str::from_utf8(include_bytes!("../../fixtures/hello/generated/schema.json"))
                .expect("checked-in hello schema is UTF-8");
        assert_eq!(schema.matches(REVISION).count(), 1);
    }

    fn id(value: &str) -> BoxId {
        BoxId::new(value).unwrap()
    }

    fn valid_schema(box_id: &str, capability: &str) -> String {
        format!(
            "{{ \"box_id\": \"{box_id}\", \"capabilities\": [ {{ \"deprecation\": null, \
             \"docs\": [], \"error\": \"ImportError\", \"id\": \"{box_id}.{capability}\", \
             \"idempotency\": \"none\", \"input\": {{ \"name\": \"name\", \"type\": \"String\" }}, \
             \"max_exposure\": \"external\", \"name\": \"{capability}\", \
             \"output\": {{ \"type\": \"String\" }}, \"shape\": \"unary\" }} ], \
             \"provenance\": {{}}, \"revision\": \"{REVISION}\", \"schema_format\": 1, \
             \"types\": [ {{ \"deprecation\": null, \"docs\": [], \"kind\": \"error\", \
             \"name\": \"ImportError\", \"variants\": [ {{ \"deprecation\": null, \"docs\": [], \
             \"name\": \"Failed\", \"payload\": \"unit\" }} ] }} ] }}"
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
        assert_eq!(
            capability.input_type(),
            &TypeExpression::Leaf(CanonicalType::String)
        );
        assert_eq!(
            capability.output_type(),
            &TypeExpression::Leaf(CanonicalType::String)
        );
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
        let line = "BXG0047 imports/hello.json:1:1-1:1 offending=\"import hello contract surface\" rule=\"an imported schema must be a strict format-1 contract whose unary capabilities use the supported structured boundary subset\" source=\"specs/s2-contract-generator.md D4\"";
        assert_line("greeter", &schema, "BXG0047", line);
    }

    /// Wraps `caps` as a `hello` schema's `capabilities` value, keeping the other fields valid.
    fn schema_with_capabilities(caps: &str) -> String {
        format!(
            "{{ \"box_id\": \"hello\", \"capabilities\": {caps}, \"provenance\": {{}}, \
             \"revision\": \"{REVISION}\", \"schema_format\": 1, \"types\": [ {{ \
             \"deprecation\": null, \"docs\": [], \"kind\": \"error\", \"name\": \"ImportError\", \
             \"variants\": [ {{ \"deprecation\": null, \"docs\": [], \"name\": \"Failed\", \
             \"payload\": \"unit\" }} ] }} ] }}"
        )
    }

    /// Spells one otherwise-valid `hello.{name}` capability entry as its JSON object.
    fn valid_capability(name: &str) -> String {
        format!(
            "{{ \"deprecation\": null, \"docs\": [], \"error\": \"ImportError\", \
             \"id\": \"hello.{name}\", \"idempotency\": \"none\", \
             \"input\": {{ \"name\": \"name\", \"type\": \"String\" }}, \
             \"max_exposure\": \"external\", \"name\": \"{name}\", \
             \"output\": {{ \"type\": \"String\" }}, \"shape\": \"unary\" }}"
        )
    }

    /// Parses a single-import `hello` schema and asserts its one BXG0047 carries `detail`.
    fn assert_bxg0047(schema: &str) {
        let request = request("greeter", &[("hello", "imports/hello.json", schema)]);
        let diagnostics = ImportModel::parse_all(&request).unwrap_err();
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one diagnostic, got {diagnostics:?}");
        };
        assert_eq!(diagnostic.code(), "BXG0047");
        assert_eq!(
            diagnostic.offending_construct(),
            "import hello contract surface"
        );
    }

    #[test]
    fn empty_capabilities_array_is_bxg0047() {
        assert_bxg0047(&schema_with_capabilities("[]"));
    }

    #[test]
    fn non_object_capability_entry_is_bxg0047() {
        assert_bxg0047(&schema_with_capabilities("[ 1 ]"));
    }

    #[test]
    fn missing_capability_name_is_bxg0047() {
        let entry = valid_capability("greet").replace(" \"name\": \"greet\",", "");
        assert_bxg0047(&schema_with_capabilities(&format!("[ {entry} ]")));
    }

    #[test]
    fn duplicate_capability_name_is_bxg0047() {
        let caps = format!(
            "[ {}, {} ]",
            valid_capability("greet"),
            valid_capability("greet")
        );
        assert_bxg0047(&schema_with_capabilities(&caps));
    }

    #[test]
    fn mismatched_capability_id_is_bxg0047() {
        let entry = valid_capability("greet").replace("\"hello.greet\"", "\"hello.wrong\"");
        assert_bxg0047(&schema_with_capabilities(&format!("[ {entry} ]")));
    }

    #[test]
    fn unknown_input_leaf_is_bxg0047() {
        let entry = valid_capability("greet").replace(
            "\"name\": \"name\", \"type\": \"String\"",
            "\"name\": \"name\", \"type\": \"u128\"",
        );
        assert_bxg0047(&schema_with_capabilities(&format!("[ {entry} ]")));
    }

    #[test]
    fn unknown_output_leaf_is_bxg0047() {
        let entry = valid_capability("greet").replace(
            "\"output\": { \"type\": \"String\" }",
            "\"output\": { \"type\": \"usize\" }",
        );
        assert_bxg0047(&schema_with_capabilities(&format!("[ {entry} ]")));
    }

    #[test]
    fn full_emitter_schema_with_provenance_and_types_hydrates() {
        let schema =
            std::str::from_utf8(include_bytes!("../../fixtures/hello/generated/schema.json"))
                .unwrap();
        let request = request("greeter", &[("hello", "imports/hello.json", schema)]);
        let models = ImportModel::parse_all(&request).unwrap();
        let [model] = models.as_slice() else {
            panic!("expected one model, got {models:?}");
        };
        assert_eq!(model.package().as_str(), "hello");
        let [capability] = model.capabilities() else {
            panic!("expected one capability");
        };
        assert_eq!(capability.name(), "greet");
        assert_eq!(
            capability.input_type(),
            &TypeExpression::Leaf(CanonicalType::String)
        );
        assert_eq!(
            capability.output_type(),
            &TypeExpression::Leaf(CanonicalType::String)
        );
    }

    fn classifier_schema() -> &'static str {
        std::str::from_utf8(include_bytes!(
            "../../boxology-classifier/generated/schema.json"
        ))
        .unwrap()
    }

    #[test]
    fn structured_classifier_schema_hydrates_ordered_provider_model() {
        let request = request(
            "checker",
            &[("classifier", "imports/classifier.json", classifier_schema())],
        );
        let models = ImportModel::parse_all(&request).unwrap();
        let [model] = models.as_slice() else {
            panic!("expected classifier import")
        };
        assert_eq!(
            model
                .declarations()
                .iter()
                .map(|declaration| declaration.name.as_str())
                .collect::<Vec<_>>(),
            [
                "CompatibilityClass",
                "ClassifyRequest",
                "ClassifyFinding",
                "ClassifyReport",
                "ClassifyFailureStage",
                "ClassifyFailure",
                "ClassifyOutcome",
            ]
        );
        let DataShape::Struct(request_fields) = &model.declarations()[1].shape else {
            panic!("ClassifyRequest must be a struct")
        };
        assert_eq!(request_fields[0].name, "base");
        assert_eq!(
            request_fields[0].ty,
            TypeExpression::Option(Box::new(TypeExpression::Vec(Box::new(
                CanonicalType::U8.into(),
            ))))
        );
        assert_eq!(request_fields[1].name, "submitted");
        assert_eq!(
            request_fields[1].ty,
            TypeExpression::Vec(Box::new(CanonicalType::U8.into()))
        );
        let [capability] = model.capabilities() else {
            panic!("expected classify capability")
        };
        assert_eq!(
            capability.input_type(),
            &TypeExpression::Local("ClassifyRequest".into())
        );
        assert_eq!(
            capability.output_type(),
            &TypeExpression::Local("ClassifyOutcome".into())
        );
    }

    #[test]
    fn malformed_unknown_and_unsupported_structured_imports_are_payload_safe_bxg0047() {
        const SENTINEL: &str = "SecretSentinel";
        let schemas = [
            classifier_schema().replacen("\"kind\": \"enum\"", "\"kind\": \"union\"", 1),
            classifier_schema().replacen(
                "\"type\": \"ClassifyRequest\"",
                "\"type\": \"SecretSentinel\"",
                1,
            ),
            classifier_schema().replacen(
                "\"type\": \"Option<Vec<u8>>\"",
                "\"type\": \"Map<String,u8>\"",
                1,
            ),
        ];
        for schema in schemas {
            let request = request(
                "checker",
                &[("classifier", "imports/classifier.json", &schema)],
            );
            let diagnostics = ImportModel::parse_all(&request).unwrap_err();
            let [diagnostic] = diagnostics.as_slice() else {
                panic!("expected one structured import diagnostic")
            };
            assert_eq!(diagnostic.code(), "BXG0047");
            assert!(!diagnostic.to_string().contains(SENTINEL));
        }
    }
}
