//! Pure in-memory request and diagnostic types shared by Boxology generators.
//!
//! Callers provide every identity, logical path, and byte. This crate consults no filesystem,
//! environment, network, locale, or clock.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod imports;
mod manifest;
mod rust;

pub use imports::{ImportModel, ImportedCapability};
pub use manifest::Manifest;
pub use rust::{
    CapabilityDeclaration, CapabilityMarkerMetadata, ContractDeclaration, ContractDeclarationRole,
    ContractDeclarationShape, ContractDeclarationSyntax, ContractDeprecation, ContractField,
    ContractFields, ContractMemberIdentity, ContractSiteMetadata, ContractVariant,
    ControlledContract, ParsedRustInput, ParsedRustInputs,
};

use boxology_contract::BoxId;
use std::{collections::BTreeMap, fmt};

/// Every live generator diagnostic code in order; retired `BXG0041` is absent.
pub const DIAGNOSTIC_CODES: &[&str] = &DiagnosticCode::catalog();

#[rustfmt::skip]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum DiagnosticCode {
    Bxg0001, Bxg0002, Bxg0003, Bxg0004, Bxg0005, Bxg0006, Bxg0007, Bxg0008,
    Bxg0009, Bxg0010, Bxg0011, Bxg0012, Bxg0013, Bxg0014, Bxg0015, Bxg0016,
    Bxg0017, Bxg0018, Bxg0019, Bxg0020, Bxg0021, Bxg0022, Bxg0023, Bxg0024,
    Bxg0025, Bxg0026, Bxg0027, Bxg0028, Bxg0029, Bxg0030, Bxg0031, Bxg0032,
    Bxg0033, Bxg0034, Bxg0035, Bxg0036, Bxg0037, Bxg0038, Bxg0039, Bxg0040,
    Bxg0042, Bxg0043, Bxg0044, Bxg0045, Bxg0046, Bxg0047, Bxg0048,
}

#[rustfmt::skip]
impl DiagnosticCode {
    const ALL: [Self; 47] = [
        Self::Bxg0001, Self::Bxg0002, Self::Bxg0003, Self::Bxg0004, Self::Bxg0005,
        Self::Bxg0006, Self::Bxg0007, Self::Bxg0008, Self::Bxg0009, Self::Bxg0010,
        Self::Bxg0011, Self::Bxg0012, Self::Bxg0013, Self::Bxg0014, Self::Bxg0015,
        Self::Bxg0016, Self::Bxg0017, Self::Bxg0018, Self::Bxg0019, Self::Bxg0020,
        Self::Bxg0021, Self::Bxg0022, Self::Bxg0023, Self::Bxg0024, Self::Bxg0025,
        Self::Bxg0026, Self::Bxg0027, Self::Bxg0028, Self::Bxg0029, Self::Bxg0030,
        Self::Bxg0031, Self::Bxg0032, Self::Bxg0033, Self::Bxg0034, Self::Bxg0035,
        Self::Bxg0036, Self::Bxg0037, Self::Bxg0038, Self::Bxg0039, Self::Bxg0040,
        Self::Bxg0042, Self::Bxg0043, Self::Bxg0044, Self::Bxg0045, Self::Bxg0046,
        Self::Bxg0047, Self::Bxg0048,
    ];

    const fn as_str(self) -> &'static str {
        match self {
            Self::Bxg0001 => "BXG0001", Self::Bxg0002 => "BXG0002", Self::Bxg0003 => "BXG0003", Self::Bxg0004 => "BXG0004",
            Self::Bxg0005 => "BXG0005", Self::Bxg0006 => "BXG0006", Self::Bxg0007 => "BXG0007", Self::Bxg0008 => "BXG0008",
            Self::Bxg0009 => "BXG0009", Self::Bxg0010 => "BXG0010", Self::Bxg0011 => "BXG0011", Self::Bxg0012 => "BXG0012",
            Self::Bxg0013 => "BXG0013", Self::Bxg0014 => "BXG0014", Self::Bxg0015 => "BXG0015", Self::Bxg0016 => "BXG0016",
            Self::Bxg0017 => "BXG0017", Self::Bxg0018 => "BXG0018", Self::Bxg0019 => "BXG0019", Self::Bxg0020 => "BXG0020",
            Self::Bxg0021 => "BXG0021", Self::Bxg0022 => "BXG0022", Self::Bxg0023 => "BXG0023", Self::Bxg0024 => "BXG0024",
            Self::Bxg0025 => "BXG0025", Self::Bxg0026 => "BXG0026", Self::Bxg0027 => "BXG0027", Self::Bxg0028 => "BXG0028",
            Self::Bxg0029 => "BXG0029", Self::Bxg0030 => "BXG0030", Self::Bxg0031 => "BXG0031", Self::Bxg0032 => "BXG0032",
            Self::Bxg0033 => "BXG0033", Self::Bxg0034 => "BXG0034", Self::Bxg0035 => "BXG0035", Self::Bxg0036 => "BXG0036",
            Self::Bxg0037 => "BXG0037", Self::Bxg0038 => "BXG0038", Self::Bxg0039 => "BXG0039", Self::Bxg0040 => "BXG0040",
            Self::Bxg0042 => "BXG0042", Self::Bxg0043 => "BXG0043", Self::Bxg0044 => "BXG0044", Self::Bxg0045 => "BXG0045",
            Self::Bxg0046 => "BXG0046", Self::Bxg0047 => "BXG0047", Self::Bxg0048 => "BXG0048",
        }
    }

    const fn catalog() -> [&'static str; 47] {
        let (mut catalog, mut index) = ([""; 47], 0);
        while index < Self::ALL.len() {
            catalog[index] = Self::ALL[index].as_str();
            index += 1;
        }
        catalog
    }
}

const RULE_SOURCE: &str = "specs/s2-contract-generator.md D1";
const CRATE_ROOT_SOURCE: &str = "specs/s2-contract-generator.md D1-D2";
const POINT: LineColumn = LineColumn { line: 1, column: 1 };
const REQUEST_SPAN: Span = Span {
    start: POINT,
    end: POINT,
};
macro_rules! ref_getters {
    ($(#[$meta:meta] $name:ident: $return:ty = $field:tt;)*) => {$(
        #[$meta] pub fn $name(&self) -> $return { &self.$field }
    )*};
}
macro_rules! copy_getters {
    ($(#[$meta:meta] $name:ident: $return:ty = $field:ident;)*) => {$(
        #[$meta] pub fn $name(&self) -> $return { self.$field }
    )*};
}

/// An owned, non-absolute UTF-8 logical path with bytewise equality and order.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RelativePath(String);
impl RelativePath {
    fn parse(value: String) -> Result<Self, ()> {
        let bytes = value.as_bytes();
        if value.is_empty()
            || value.starts_with('/')
            || (matches!(bytes.get(1), Some(b':')) && bytes[0].is_ascii_alphabetic())
            || bytes
                .iter()
                .any(|byte| matches!(byte, b'\\' | b'\t' | b'\n' | b'\r' | 0))
        {
            return Err(());
        }
        let mut ordinary = false;
        for component in value.split('/') {
            if component.is_empty() || component == "." || component == ".." && ordinary {
                return Err(());
            }
            ordinary |= component != "..";
        }
        Ok(Self(value))
    }
    /// Returns the exact, unnormalized spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
/// One declared logical input and its uninterpreted bytes.
#[derive(Debug, Eq, PartialEq)]
pub struct InputFile {
    path: RelativePath,
    bytes: Vec<u8>,
}
impl InputFile {
    ref_getters! {
        #[doc = "Returns the input's logical path."] path: &RelativePath = path;
        #[doc = "Returns the exact caller-provided bytes."] bytes: &[u8] = bytes;
    }
}
/// A foreign package and the declared logical path to its checked-in schema.
#[derive(Debug, Eq, PartialEq)]
pub struct DeclaredImport {
    package: BoxId,
    schema_path: RelativePath,
}
impl DeclaredImport {
    ref_getters! {
        #[doc = "Returns the imported package identity."] package: &BoxId = package;
        #[doc = "Returns the schema's exact logical path."] schema_path: &RelativePath = schema_path;
    }
}
/// A fully caller-projected, in-memory generation request.
#[derive(Debug, Eq, PartialEq)]
pub struct GenerationRequest {
    box_id: BoxId,
    crate_root: RelativePath,
    inputs: Vec<InputFile>,
    imports: Vec<DeclaredImport>,
    outputs: Vec<RelativePath>,
}
impl GenerationRequest {
    /// Validates request paths and relationships, preserving valid member order and bytes.
    pub fn new(
        box_id: BoxId,
        raw_crate_root: String,
        raw_inputs: Vec<(String, Vec<u8>)>,
        raw_imports: Vec<(BoxId, String)>,
        raw_outputs: Vec<String>,
    ) -> Result<Self, Diagnostics> {
        let mut errors = Vec::new();
        let crate_root = match RelativePath::parse(raw_crate_root) {
            Ok(path) => Some(path),
            Err(()) => {
                errors.push(request_diagnostic(
                    request_path(),
                    DiagnosticCode::Bxg0001,
                    "crate_root logical path".into(),
                    "logical paths must be forward-slash relative",
                ));
                None
            }
        };
        let mut inputs = Vec::new();
        let mut input_paths = BTreeMap::new();
        for (index, (raw_path, bytes)) in raw_inputs.into_iter().enumerate() {
            if let Some(path) = checked_path(raw_path, "input", index, &mut errors) {
                if let Some(first) = input_paths.get(&path) {
                    errors.push(request_diagnostic(
                        path.clone(),
                        DiagnosticCode::Bxg0002,
                        format!("input[{index}] duplicates input[{first}]"),
                        "input logical paths must be unique",
                    ));
                } else {
                    input_paths.insert(path.clone(), index);
                }
                if requires_utf8(&path) && std::str::from_utf8(&bytes).is_err() {
                    errors.push(request_diagnostic(
                        path.clone(),
                        DiagnosticCode::Bxg0003,
                        format!("input[{index}] bytes"),
                        "Rust, TOML, and JSON inputs must be valid UTF-8",
                    ));
                }
                inputs.push(InputFile { path, bytes });
            }
        }
        if let Some(crate_root) = &crate_root
            && (!input_paths.contains_key(crate_root) || !crate_root.as_str().ends_with(".rs"))
        {
            errors.push(Diagnostic {
                path: crate_root.clone(),
                span: REQUEST_SPAN,
                code: DiagnosticCode::Bxg0015,
                offending: "crate_root input".into(),
                rule: "crate_root must name one declared .rs input",
                rule_source: CRATE_ROOT_SOURCE,
            });
        }
        if !input_paths
            .keys()
            .any(|path| path.as_str() == "boxology.toml")
        {
            errors.push(request_diagnostic(
                request_path(),
                DiagnosticCode::Bxg0004,
                "required input boxology.toml".into(),
                "the request must include boxology.toml",
            ));
        }
        let mut imports = Vec::new();
        let mut import_packages = BTreeMap::new();
        for (index, (package, raw_path)) in raw_imports.into_iter().enumerate() {
            let first = import_packages.get(&package).copied();
            if first.is_none() {
                import_packages.insert(package.clone(), index);
            }
            let schema_path = checked_path(raw_path, "import", index, &mut errors);
            if let Some(first) = first {
                errors.push(request_diagnostic(
                    schema_path.clone().unwrap_or_else(request_path),
                    DiagnosticCode::Bxg0006,
                    format!(
                        "import[{index}] duplicates import[{first}] package {}",
                        package.as_str()
                    ),
                    "declared import package identities must be unique",
                ));
            }
            if let Some(schema_path) = schema_path {
                if !input_paths.contains_key(&schema_path) {
                    errors.push(request_diagnostic(
                        schema_path.clone(),
                        DiagnosticCode::Bxg0005,
                        format!("import[{index}] schema input"),
                        "each declared import schema must be present among the request inputs",
                    ));
                }
                imports.push(DeclaredImport {
                    package,
                    schema_path,
                });
            }
        }
        let mut outputs = Vec::new();
        for (index, raw_path) in raw_outputs.into_iter().enumerate() {
            if let Some(path) = checked_path(raw_path, "output", index, &mut errors) {
                outputs.push(path);
            }
        }
        if !errors.is_empty() {
            errors.sort();
            return Err(Diagnostics(errors));
        }
        Ok(Self {
            box_id,
            crate_root: crate_root.expect("validated crate root exists when diagnostics are empty"),
            inputs,
            imports,
            outputs,
        })
    }

    ref_getters! {
        #[doc = "Returns the manifest-provided box identity."] box_id: &BoxId = box_id;
        #[doc = "Returns the exact validated declared Rust-input root."] crate_root: &RelativePath = crate_root;
        #[doc = "Returns inputs in caller-provided order."] inputs: &[InputFile] = inputs;
        #[doc = "Returns declared imports in caller-provided order."] imports: &[DeclaredImport] = imports;
        #[doc = "Returns declared outputs in caller-provided order."] outputs: &[RelativePath] = outputs;
    }

    /// Requires the declared outputs to equal one generator's complete output set.
    pub fn require_exact_outputs(&self, expected: &[&str]) -> Result<(), Diagnostics> {
        let mut actual = self
            .outputs
            .iter()
            .map(RelativePath::as_str)
            .collect::<Vec<_>>();
        let mut expected = expected.to_vec();
        actual.sort_unstable_by_key(|path| path.as_bytes());
        expected.sort_unstable_by_key(|path| path.as_bytes());
        let has_duplicates = |paths: &[&str]| paths.windows(2).any(|pair| pair[0] == pair[1]);
        if actual == expected && !has_duplicates(&actual) && !has_duplicates(&expected) {
            return Ok(());
        }
        Err(Diagnostics(vec![request_diagnostic(
            request_path(),
            DiagnosticCode::Bxg0039,
            format!("declared outputs {actual:?}"),
            "declared outputs must equal the generator's complete output set without duplicates",
        )]))
    }
}
/// A source coordinate with one-based line and character-column units.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LineColumn {
    line: usize,
    column: usize,
}
impl LineColumn {
    copy_getters! {
        #[doc = "Returns the one-based line."] line: usize = line;
        #[doc = "Returns the one-based column."] column: usize = column;
    }
}
/// A source span with one-based start and end coordinates.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Span {
    start: LineColumn,
    end: LineColumn,
}
impl Span {
    copy_getters! {
        #[doc = "Returns the start coordinate."] start: LineColumn = start;
        #[doc = "Returns the end coordinate."] end: LineColumn = end;
    }
}

/// One stable coded request or model diagnostic.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Diagnostic {
    path: RelativePath,
    span: Span,
    code: DiagnosticCode,
    offending: String,
    rule: &'static str,
    rule_source: &'static str,
}
impl Diagnostic {
    /// Returns the stable `BXG####` code.
    pub fn code(&self) -> &'static str {
        self.code.as_str()
    }
    copy_getters! { #[doc = "Returns the source span."] span: Span = span; }
    ref_getters! {
        #[doc = "Returns the workspace-relative logical path."] path: &RelativePath = path;
        #[doc = "Returns the offending construct description."] offending_construct: &str = offending;
        #[doc = "Returns the violated rule."] rule: &str = rule;
        #[doc = "Returns the normative source of the rule."] rule_source: &str = rule_source;
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}:{}:{}-{}:{} offending={:?} rule={:?} source={:?}",
            self.code(),
            self.path.as_str(),
            self.span.start.line,
            self.span.start.column,
            self.span.end.line,
            self.span.end.column,
            self.offending,
            self.rule,
            self.rule_source
        )
    }
}

/// A nonempty, deterministically sorted diagnostic collection.
#[derive(Debug, Eq, PartialEq)]
pub struct Diagnostics(Vec<Diagnostic>);
impl Diagnostics {
    ref_getters! {
        #[doc = "Returns the sorted diagnostics."] as_slice: &[Diagnostic] = 0;
    }

    /// Renders canonical `boxology.generator-diagnostics@1` JSON.
    pub fn render_json(&self) -> String {
        let mut out = String::from(
            "{\n  \"schema\": \"boxology.generator-diagnostics@1\",\n  \"diagnostics\": [\n",
        );
        for (index, diagnostic) in self.0.iter().enumerate() {
            if index > 0 {
                out.push_str(",\n");
            }
            let span = diagnostic.span();
            out.push_str("    {\n      \"code\": ");
            push_json_string(&mut out, diagnostic.code());
            out.push_str(",\n      \"path\": ");
            push_json_string(&mut out, diagnostic.path().as_str());
            out.push_str(",\n      \"span\": {\n        \"start\": {\n          \"line\": ");
            out.push_str(&span.start().line().to_string());
            out.push_str(",\n          \"column\": ");
            out.push_str(&span.start().column().to_string());
            out.push_str("\n        },\n        \"end\": {\n          \"line\": ");
            out.push_str(&span.end().line().to_string());
            out.push_str(",\n          \"column\": ");
            out.push_str(&span.end().column().to_string());
            out.push_str("\n        }\n      },\n      \"offending\": ");
            push_json_string(&mut out, diagnostic.offending_construct());
            out.push_str(",\n      \"rule\": ");
            push_json_string(&mut out, diagnostic.rule());
            out.push_str(",\n      \"rule_source\": ");
            push_json_string(&mut out, diagnostic.rule_source());
            out.push_str("\n    }");
        }
        out.push_str("\n  ]\n}\n");
        out
    }
}

fn push_json_string(out: &mut String, value: &str) {
    out.push_str(&serde_json::to_string(value).expect("a string always serializes as JSON"));
}
impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl fmt::Display for Diagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str("\n")?;
            }
            diagnostic.fmt(formatter)?;
        }
        Ok(())
    }
}

fn checked_path(
    raw_path: String,
    kind: &str,
    index: usize,
    errors: &mut Vec<Diagnostic>,
) -> Option<RelativePath> {
    match RelativePath::parse(raw_path) {
        Ok(path) => Some(path),
        Err(()) => {
            errors.push(request_diagnostic(
                request_path(),
                DiagnosticCode::Bxg0001,
                format!("{kind}[{index}] logical path"),
                "logical paths must be forward-slash relative",
            ));
            None
        }
    }
}

fn request_diagnostic(
    path: RelativePath,
    code: DiagnosticCode,
    offending: String,
    rule: &'static str,
) -> Diagnostic {
    Diagnostic {
        path,
        span: REQUEST_SPAN,
        code,
        offending,
        rule,
        rule_source: RULE_SOURCE,
    }
}

fn request_path() -> RelativePath {
    RelativePath("<request>".into())
}

fn requires_utf8(path: &RelativePath) -> bool {
    [".rs", ".toml", ".json"]
        .iter()
        .any(|extension| path.as_str().ends_with(extension))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use syn::visit::Visit;
    const ROOT: &str = "root.rs";

    fn id(value: &str) -> BoxId {
        BoxId::new(value).unwrap()
    }
    fn invalid_request() -> Diagnostics {
        GenerationRequest::new(
            id("demo"),
            ROOT.into(),
            vec![
                ("boxology.toml".into(), b"ok\n".to_vec()),
                (ROOT.into(), vec![]),
                ("/secret/input".into(), vec![]),
            ],
            vec![(id("foreign"), "schema/..".into())],
            vec!["bad\\output".into()],
        )
        .unwrap_err()
    }
    fn assert_single(diagnostics: Diagnostics, code: &str, path: &str) {
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one diagnostic, got {diagnostics:?}");
        };
        assert_eq!(diagnostic.code(), code);
        assert_eq!(diagnostic.path().as_str(), path);
        assert_eq!(diagnostic.span(), REQUEST_SPAN);
        assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
        assert!(!diagnostic.offending_construct().is_empty());
        assert!(!diagnostic.rule().is_empty());
        assert!(!diagnostic.to_string().contains(['\n', '\r']));
    }
    fn mixed_relational_errors() -> Diagnostics {
        GenerationRequest::new(
            id("demo"),
            "missing.rs".into(),
            vec![
                ("z.rs".into(), vec![0xff]),
                ("a.json".into(), vec![0xff]),
                ("a.json".into(), b"{}".to_vec()),
            ],
            vec![
                (id("foreign"), "m.json".into()),
                (id("foreign"), "n.json".into()),
            ],
            vec![],
        )
        .unwrap_err()
    }

    #[test]
    fn request_preserves_exact_values_and_path_grammar() {
        let request = GenerationRequest::new(
            id("demo"),
            "source/custom-entry.rs".into(),
            vec![
                ("boxology.toml".into(), b"manifest\n".to_vec()),
                ("source/custom-entry.rs".into(), b"fn entry() {}\n".to_vec()),
                ("../foreign/schema.json".into(), b"{}\n".to_vec()),
                ("assets/pixel.bin".into(), vec![0xff]),
            ],
            vec![(id("foreign"), "../foreign/schema.json".into())],
            vec!["generated/schema.json".into()],
        )
        .unwrap();
        assert_eq!(request.box_id().as_str(), "demo");
        assert_eq!(request.crate_root().as_str(), "source/custom-entry.rs");
        assert_eq!(request.inputs()[0].path().as_str(), "boxology.toml");
        assert_eq!(request.inputs()[0].bytes(), b"manifest\n");
        assert_eq!(request.inputs()[3].bytes(), [0xff]);
        assert_eq!(request.imports()[0].package().as_str(), "foreign");
        assert_eq!(
            request.imports()[0].schema_path().as_str(),
            "../foreign/schema.json"
        );
        assert_eq!(request.outputs()[0].as_str(), "generated/schema.json");
        for valid in ["a", "a/b", "../schema.json", "../../x", "é"] {
            assert_eq!(RelativePath::parse(valid.into()).unwrap().as_str(), valid);
        }
        for invalid in [
            "", "/a", "C:/a", "a//b", "a/.", "a/..", "a\\b", "a\tb", "a\nb", "a\rb", "a\0b",
        ] {
            assert!(RelativePath::parse(invalid.into()).is_err());
        }
    }

    #[test]
    fn invalid_crate_root_is_one_exact_payload_safe_path_diagnostic() {
        let diagnostics = GenerationRequest::new(
            id("demo"),
            "/secret/root.rs".into(),
            vec![("boxology.toml".into(), b"manifest".to_vec())],
            vec![],
            vec![],
        )
        .unwrap_err();
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one diagnostic, got {diagnostics:?}");
        };
        assert_eq!(diagnostic.code(), "BXG0001");
        assert_eq!(diagnostic.path().as_str(), "<request>");
        assert_eq!(diagnostic.span(), REQUEST_SPAN);
        assert_eq!(diagnostic.offending_construct(), "crate_root logical path");
        assert_eq!(
            diagnostic.rule(),
            "logical paths must be forward-slash relative"
        );
        assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
        assert_eq!(
            diagnostic.to_string(),
            "BXG0001 <request>:1:1-1:1 offending=\"crate_root logical path\" rule=\"logical paths must be forward-slash relative\" source=\"specs/s2-contract-generator.md D1\""
        );
        assert!(!diagnostics.to_string().contains("/secret"));
    }

    #[test]
    fn crate_root_must_be_a_declared_exact_rs_input() {
        for (root, inputs) in [
            (
                "missing.rs",
                vec![("boxology.toml".into(), b"manifest".to_vec())],
            ),
            (
                "source/custom-entry.RS",
                vec![
                    ("boxology.toml".into(), b"manifest".to_vec()),
                    ("source/custom-entry.RS".into(), vec![]),
                ],
            ),
        ] {
            let diagnostics =
                GenerationRequest::new(id("demo"), root.into(), inputs, vec![], vec![])
                    .unwrap_err();
            let [diagnostic] = diagnostics.as_slice() else {
                panic!("expected one diagnostic, got {diagnostics:?}");
            };
            assert_eq!(diagnostic.code(), "BXG0015");
            assert_eq!(diagnostic.path().as_str(), root);
            assert_eq!(diagnostic.span(), REQUEST_SPAN);
            assert_eq!(diagnostic.offending_construct(), "crate_root input");
            assert_eq!(
                diagnostic.rule(),
                "crate_root must name one declared .rs input"
            );
            assert_eq!(diagnostic.rule_source(), CRATE_ROOT_SOURCE);
            assert_eq!(
                diagnostic.to_string(),
                format!(
                    "BXG0015 {root}:1:1-1:1 offending=\"crate_root input\" rule=\"crate_root must name one declared .rs input\" source=\"specs/s2-contract-generator.md D1-D2\""
                )
            );
        }
    }

    #[test]
    fn invalid_paths_are_coded_sorted_safe_and_have_exact_spans() {
        let (first, second) = (invalid_request(), invalid_request());
        assert_eq!(first, second);
        assert_eq!(first.as_slice().len(), 3);
        assert!(first.as_slice().windows(2).all(|pair| pair[0] <= pair[1]));
        for diagnostic in &first {
            assert_eq!(diagnostic.code(), "BXG0001");
            assert_eq!(diagnostic.path().as_str(), "<request>");
            assert_eq!(diagnostic.span(), REQUEST_SPAN);
            assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
            assert!(!diagnostic.offending_construct().is_empty() && !diagnostic.rule().is_empty());
            assert!(!diagnostic.to_string().contains(['\n', '\r']));
        }
        assert_eq!((POINT.line(), POINT.column()), (1, 1));
        assert!(!first.to_string().contains("/secret"));
    }

    #[test]
    fn duplicate_input_path_is_coded_at_the_duplicate_path() {
        let diagnostics = GenerationRequest::new(
            id("demo"),
            "src/lib.rs".into(),
            vec![
                ("boxology.toml".into(), b"manifest".to_vec()),
                ("src/lib.rs".into(), b"first".to_vec()),
                ("src/lib.rs".into(), b"second".to_vec()),
            ],
            vec![],
            vec![],
        )
        .unwrap_err();
        assert_single(diagnostics, "BXG0002", "src/lib.rs");
    }

    #[test]
    fn text_input_extensions_require_utf8_without_leaking_bytes() {
        for path in ["src/lib.rs", "box.toml", "schema.json"] {
            let mut inputs = vec![
                ("boxology.toml".into(), b"manifest".to_vec()),
                (path.into(), vec![0xff]),
            ];
            let root = if path.ends_with(".rs") {
                path
            } else {
                inputs.push((ROOT.into(), vec![]));
                ROOT
            };
            let diagnostics =
                GenerationRequest::new(id("demo"), root.into(), inputs, vec![], vec![])
                    .unwrap_err();
            assert!(!diagnostics.to_string().contains('�'));
            assert_single(diagnostics, "BXG0003", path);
        }
    }

    #[test]
    fn missing_manifest_is_coded_at_the_request() {
        let diagnostics = GenerationRequest::new(
            id("demo"),
            "src/lib.rs".into(),
            vec![("src/lib.rs".into(), b"source".to_vec())],
            vec![],
            vec![],
        )
        .unwrap_err();
        assert_single(diagnostics, "BXG0004", "<request>");
    }

    #[test]
    fn missing_declared_schema_is_coded_at_its_logical_path() {
        let diagnostics = GenerationRequest::new(
            id("demo"),
            ROOT.into(),
            vec![
                ("boxology.toml".into(), b"manifest".to_vec()),
                (ROOT.into(), vec![]),
            ],
            vec![(id("foreign"), "foreign/schema.json".into())],
            vec![],
        )
        .unwrap_err();
        assert_single(diagnostics, "BXG0005", "foreign/schema.json");
    }

    #[test]
    fn duplicate_import_package_is_coded_at_the_second_schema_path() {
        let diagnostics = GenerationRequest::new(
            id("demo"),
            ROOT.into(),
            vec![
                ("boxology.toml".into(), b"manifest".to_vec()),
                (ROOT.into(), vec![]),
                ("one.json".into(), b"{}".to_vec()),
                ("two.json".into(), b"{}".to_vec()),
            ],
            vec![
                (id("foreign"), "one.json".into()),
                (id("foreign"), "two.json".into()),
            ],
            vec![],
        )
        .unwrap_err();
        assert_single(diagnostics, "BXG0006", "two.json");
    }

    #[test]
    fn relational_diagnostics_are_complete_and_deterministically_sorted() {
        let (first, second) = (mixed_relational_errors(), mixed_relational_errors());
        assert_eq!(first, second);
        let actual = first
            .as_slice()
            .iter()
            .map(|diagnostic| (diagnostic.path().as_str(), diagnostic.code()))
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            [
                ("<request>", "BXG0004"),
                ("a.json", "BXG0002"),
                ("a.json", "BXG0003"),
                ("m.json", "BXG0005"),
                ("missing.rs", "BXG0015"),
                ("n.json", "BXG0005"),
                ("n.json", "BXG0006"),
                ("z.rs", "BXG0003"),
            ]
        );
        assert!(first.as_slice().windows(2).all(|pair| pair[0] <= pair[1]));
        for diagnostic in &first {
            assert_eq!(diagnostic.span(), REQUEST_SPAN);
            assert!(!diagnostic.to_string().contains(['\n', '\r']));
        }
    }

    #[test]
    fn public_request_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(RelativePath, InputFile, DeclaredImport, GenerationRequest)>();
        bounds::<Manifest>();
        bounds::<(LineColumn, Span, Diagnostic, Diagnostics)>();
    }

    #[test]
    #[rustfmt::skip]
    fn catalog_and_json_mirror_are_exact_and_hostile_safe() {
        let canonical = |codes: &[&str]| {
            codes.len() == 47 && codes.iter().enumerate().all(|(index, code)| {
                let number = if index < 40 { index + 1 } else { index + 2 };
                *code == format!("BXG{number:04}")
            })
        };
        assert!(canonical(DIAGNOSTIC_CODES));
        let mut duplicate = DIAGNOSTIC_CODES.to_vec();
        duplicate[1] = duplicate[0];
        let mut misspelled = DIAGNOSTIC_CODES.to_vec();
        misspelled[0] = "BXG01";
        let mut missing = DIAGNOSTIC_CODES.to_vec();
        missing.remove(0);
        assert!([duplicate, misspelled, missing].iter().all(|codes| !canonical(codes)));
        let diagnostics = Diagnostics(vec![Diagnostic { path: RelativePath("hostile/\"line.json".into()), span: Span { start: LineColumn { line: 2, column: 3 }, end: LineColumn { line: 4, column: 5 } }, code: DiagnosticCode::Bxg0001, offending: "bad\n\"construct\\".into(), rule: "rule\t\"quoted\"", rule_source: "source\\path" }]);
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0001");
        assert_eq!(diagnostics.as_slice()[0].span().start().line(), 2);
        assert_eq!(
            diagnostics.render_json(),
            "{\n  \"schema\": \"boxology.generator-diagnostics@1\",\n  \"diagnostics\": [\n    {\n      \"code\": \"BXG0001\",\n      \"path\": \"hostile/\\\"line.json\",\n      \"span\": {\n        \"start\": {\n          \"line\": 2,\n          \"column\": 3\n        },\n        \"end\": {\n          \"line\": 4,\n          \"column\": 5\n        }\n      },\n      \"offending\": \"bad\\n\\\"construct\\\\\",\n      \"rule\": \"rule\\t\\\"quoted\\\"\",\n      \"rule_source\": \"source\\\\path\"\n    }\n  ]\n}\n"
        );
        let mirror: serde_json::Value = serde_json::from_str(&diagnostics.render_json()).unwrap();
        assert_eq!(mirror["diagnostics"][0]["offending"], "bad\n\"construct\\");
    }

    #[derive(Default)]
    struct AllocationAudit {
        codes: BTreeSet<&'static str>,
        errors: Vec<String>,
        helpers: BTreeSet<String>,
        forwarded: Option<bool>,
        placement: bool,
        named_closure: bool,
    }

    impl AllocationAudit {
        fn code(&mut self, expression: &syn::Expr) -> Option<DiagnosticCode> {
            let syn::Expr::Path(path) = expression else {
                self.errors.push("dynamic diagnostic code".into());
                return None;
            };
            let segments = &path.path.segments;
            if segments.len() != 2 || segments[0].ident != "DiagnosticCode" {
                self.errors.push("dynamic diagnostic code".into());
                return None;
            }
            let variant = segments[1].ident.to_string();
            let code = DiagnosticCode::ALL
                .iter()
                .find(|code| format!("{code:?}") == variant)
                .copied();
            if code.is_none() {
                self.errors
                    .push(format!("unknown diagnostic code {variant}"));
            }
            code
        }

        fn record(&mut self, expression: &syn::Expr) {
            if let Some(code) = self.code(expression) {
                self.codes.insert(code.as_str());
            }
        }

        fn record_call(&mut self, helper: &str, expression: &syn::Expr) {
            if let Some(code) = self.code(expression) {
                self.codes.insert(code.as_str());
                if !helper_domain(helper, code) {
                    self.errors
                        .push(format!("wrong diagnostic domain for {helper}"));
                }
            }
        }
    }

    #[rustfmt::skip]
    fn helper_index(name: &str) -> Option<usize> {
        match name { "diagnostic" => Some(0), "request_diagnostic" => Some(1), "capability_identity_error" | "module_diagnostic" => Some(2), "emit" | "add_metadata_error" => Some(3), _ => None }
    }

    #[rustfmt::skip]
    fn helper_domain(name: &str, code: DiagnosticCode) -> bool {
        let number = code.as_str()[3..].parse::<u8>().unwrap();
        match name { "request_diagnostic" => (1..=6).contains(&number) || number == 39, "diagnostic" => (7..=13).contains(&number),
            "module_diagnostic" => matches!(number, 16..=20 | 22 | 23 | 36 | 37), "add_metadata_error" => matches!(number, 32 | 33),
            "capability_identity_error" => matches!(number, 34 | 35), "emit" => (42..=47).contains(&number), _ => false }
    }

    #[rustfmt::skip]
    fn typed_code_parameter(item: &syn::ItemFn, index: usize) -> bool {
        matches!(item.sig.inputs.get(index), Some(syn::FnArg::Typed(argument)) if matches!(argument.pat.as_ref(), syn::Pat::Ident(pattern) if pattern.ident == "code" && pattern.by_ref.is_none() && pattern.mutability.is_none() && pattern.subpat.is_none()) && matches!(argument.ty.as_ref(), syn::Type::Path(path) if path.path.is_ident("DiagnosticCode")))
    }

    #[rustfmt::skip]
    fn is_test(attributes: &[syn::Attribute]) -> bool {
        attributes.iter().any(|attribute| attribute.path().is_ident("cfg") && attribute.parse_args::<syn::Ident>().is_ok_and(|ident| ident == "test"))
    }

    #[rustfmt::skip]
    impl<'ast> Visit<'ast> for AllocationAudit {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if !is_test(&item.attrs) {
                syn::visit::visit_item_mod(self, item);
            }
        }
        fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
            if item.ident != "DiagnosticCode" {
                syn::visit::visit_item_enum(self, item);
            }
        }

        fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
            let kind = match item.self_ty.as_ref() { syn::Type::Path(path) => path.path.segments.last().map(|segment| &segment.ident), _ => None };
            if kind.is_some_and(|ident| ident == "DiagnosticCode") { return; }
            let previous = std::mem::replace(&mut self.placement, kind.is_some_and(|ident| ident == "PlacementVisitor"));
            syn::visit::visit_item_impl(self, item);
            self.placement = previous;
        }

        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            if is_test(&item.attrs) {
                return;
            }
            let name = item.sig.ident.to_string();
            let returns_diagnostic = matches!(&item.sig.output, syn::ReturnType::Type(_, ty)
                if matches!(ty.as_ref(), syn::Type::Path(path) if path.path.is_ident("Diagnostic")));
            if returns_diagnostic && !matches!(name.as_str(), "diagnostic" | "request_diagnostic" | "capability_identity_error" | "contract_role_diagnostic" | "deprecation_diagnostic" | "module_diagnostic") {
                self.errors.push(format!("unexpected diagnostic constructor {name}"));
            }
            let Some(index) = helper_index(&name) else {
                return syn::visit::visit_item_fn(self, item);
            };
            if !self.helpers.insert(name.clone()) || !typed_code_parameter(item, index) {
                self.errors.push(format!("invalid helper {name}"));
                return;
            }
            self.forwarded = Some(false);
            self.visit_block(&item.block);
            if self.forwarded.take() != Some(true) {
                self.errors
                    .push(format!("helper {name} does not forward code"));
            }
        }

        fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
            if self.forwarded.is_some() && pattern.ident == "code" {
                self.errors.push("helper shadows code".into());
            }
            syn::visit::visit_pat_ident(self, pattern);
        }

        fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
            if self.forwarded.is_some()
                && matches!(assign.left.as_ref(), syn::Expr::Path(path) if path.path.is_ident("code"))
            {
                self.errors.push("helper mutates code".into());
            }
            syn::visit::visit_expr_assign(self, assign);
        }

        fn visit_local(&mut self, local: &'ast syn::Local) {
            let named = local.init.as_ref().is_some_and(|init| matches!(init.expr.as_ref(), syn::Expr::Closure(_)));
            if named && matches!(&local.pat, syn::Pat::Ident(pattern) if helper_index(&pattern.ident.to_string()).is_some()) {
                self.errors.push("helper shadow closure".into());
            }
            let previous = self.named_closure;
            self.named_closure |= named;
            syn::visit::visit_local(self, local);
            self.named_closure = previous;
        }

        fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
            let binding = match tree { syn::UseTree::Name(name) => Some(&name.ident), syn::UseTree::Rename(name) => Some(&name.rename), _ => None };
            if matches!(tree, syn::UseTree::Glob(_)) || binding.is_some_and(|name| helper_index(&name.to_string()).is_some()) {
                self.errors.push("helper shadow import".into());
            }
            syn::visit::visit_use_tree(self, tree);
        }

        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            let syn::Expr::Path(function) = call.func.as_ref() else {
                syn::visit::visit_expr_call(self, call);
                return;
            };
            let Some(name) = function
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
            else {
                syn::visit::visit_expr_call(self, call);
                return;
            };
            if let Some(index) = helper_index(&name) {
                if self.named_closure {
                    self.errors.push("named closure returns diagnostic".into());
                }
                if function.path.leading_colon.is_some() || function.path.segments.len() != 1 {
                    self.errors.push(format!("qualified helper call {name}"));
                }
                if let Some(code) = call.args.get(index) {
                    self.record_call(&name, code);
                } else {
                    self.errors.push(format!("uncoded {name} call"));
                }
            }
            syn::visit::visit_expr_call(self, call);
        }

        fn visit_expr_struct(&mut self, structure: &'ast syn::ExprStruct) {
            let seam = structure.path.segments.last().is_some_and(|segment| {
                segment.ident == "Diagnostic" || segment.ident == "PlacementVisitor"
            });
            if seam {
                match structure.fields.iter().find(
                    |field| matches!(&field.member, syn::Member::Named(ident) if ident == "code"),
                ) {
                    Some(field) => {
                        if self.forwarded.is_some() {
                            let exact = matches!(&field.expr, syn::Expr::Path(path) if path.path.is_ident("code"));
                            self.forwarded = Some(exact);
                            if !exact {
                                self.errors.push("helper does not forward code".into());
                            }
                        } else if !(self.placement && matches!(&field.expr, syn::Expr::Field(access)
                            if matches!(access.base.as_ref(), syn::Expr::Path(path) if path.path.is_ident("self"))
                                && matches!(&access.member, syn::Member::Named(ident) if ident == "code"))) {
                            self.record(&field.expr);
                        }
                        if self.named_closure && structure.path.is_ident("Diagnostic") {
                            self.errors.push("named closure constructs diagnostic".into());
                        }
                    }
                    None => self.errors.push("uncoded diagnostic allocation".into()),
                }
            }
            syn::visit::visit_expr_struct(self, structure);
        }

        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if mac.path.is_ident("vec")
                && let Ok(expressions) = mac.parse_body_with(
                    syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
                )
            {
                for expression in &expressions {
                    self.visit_expr(expression);
                }
            }
        }
    }

    #[rustfmt::skip]
    fn sources() -> [&'static str; 6] {
        [include_str!("lib.rs"), include_str!("manifest.rs"), include_str!("imports.rs"), include_str!("rust.rs"), include_str!("../../boxology-generator/src/lib.rs"), include_str!("../../boxology-generator/src/schema.rs")]
    }

    #[rustfmt::skip]
    fn audit<'a>(
        sources: impl IntoIterator<Item = &'a str>,
    ) -> Result<BTreeSet<&'static str>, Vec<String>> {
        let mut audit = AllocationAudit::default();
        for source in sources {
            audit.visit_file(&syn::parse_file(source).unwrap());
        }
        let expected = ["add_metadata_error", "capability_identity_error", "diagnostic", "emit", "module_diagnostic", "request_diagnostic"]
            .into_iter().map(str::to_owned).collect();
        if audit.helpers != expected {
            audit.errors.push("helper inventory mismatch".into());
        }
        if audit.errors.is_empty() {
            Ok(audit.codes)
        } else {
            Err(audit.errors)
        }
    }

    #[rustfmt::skip]
    fn audit_sources(extra: &str) -> Result<BTreeSet<&'static str>, Vec<String>> { audit(sources().into_iter().chain([extra])) }

    #[test]
    #[rustfmt::skip]
    fn exact_six_source_allocation_audit_kills_registry_mutants() {
        assert_eq!(audit_sources("").unwrap(), DIAGNOSTIC_CODES.iter().copied().collect());
        for mutant in [
            "fn mutant(){request_diagnostic(request_path(), DiagnosticCode::Bxg0041, String::new(), \"r\");}",
            "fn mutant(){request_diagnostic(request_path(), DiagnosticCode::Bxg9999, String::new(), \"r\");}",
            "fn mutant(){request_diagnostic(request_path(), \"dynamic\", String::new(), \"r\");}",
            "fn mutant(){request_diagnostic(request_path(), DiagnosticCode::Bxg0048, String::new(), \"r\");}",
            "fn mutant(){crate::request_diagnostic(request_path(), DiagnosticCode::Bxg0001, String::new(), \"r\");}",
            "fn request_diagnostic(){}",
            "fn mutant(){let request_diagnostic=|path:RelativePath,_code:DiagnosticCode,offending:String,rule:&'static str|Diagnostic{path,span:REQUEST_SPAN,code:DiagnosticCode::Bxg0048,offending,rule,rule_source:RULE_SOURCE};let _=request_diagnostic(request_path(),DiagnosticCode::Bxg0001,String::new(),\"r\");}",
            "fn alternate(path:RelativePath)->Diagnostic{Diagnostic{path,span:REQUEST_SPAN,code:DiagnosticCode::Bxg0001,offending:String::new(),rule:\"r\",rule_source:RULE_SOURCE}}",
            "fn mutant(path: RelativePath){let _=Diagnostic{path,span:REQUEST_SPAN,offending:String::new(),rule:\"r\",rule_source:\"s\"};}",
        ] {
            assert!(audit_sources(mutant).is_err(), "surviving mutant: {mutant}");
        }
        let broken = include_str!("lib.rs").replace("span: REQUEST_SPAN,\n        code,\n        offending,", "span: REQUEST_SPAN,\n        code: DiagnosticCode::Bxg0048,\n        offending,");
        let mut replaced = sources();
        replaced[0] = &broken;
        assert!(audit(replaced).is_err(), "wrong helper forwarding survived");
        let mutated = include_str!("lib.rs").replace("fn request_diagnostic(\n    path: RelativePath,\n    code: DiagnosticCode,", "fn request_diagnostic(\n    path: RelativePath,\n    mut code: DiagnosticCode,")
            .replace(") -> Diagnostic {\n    Diagnostic {\n        path,\n        span: REQUEST_SPAN,", ") -> Diagnostic {\n    code = DiagnosticCode::Bxg0048;\n    Diagnostic {\n        path,\n        span: REQUEST_SPAN,");
        replaced[0] = &mutated;
        assert!(audit(replaced).is_err(), "mutated helper code survived");
    }
}
