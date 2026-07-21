//! Pure in-memory request and diagnostic types shared by Boxology generators.
//!
//! Callers provide every identity, logical path, and byte. This crate consults no filesystem,
//! environment, network, locale, or clock.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

mod manifest;
mod rust;

pub use manifest::Manifest;
pub use rust::{
    CapabilityDeclaration, CapabilityMarkerMetadata, ContractDeclaration, ContractDeclarationRole,
    ContractDeclarationShape, ContractDeclarationSyntax, ContractDeprecation, ContractField,
    ContractFields, ContractMemberIdentity, ContractSiteMetadata, ContractVariant, ParsedRustInput,
    ParsedRustInputs,
};

use boxology_contract::BoxId;
use std::{collections::BTreeMap, fmt};

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
                    "BXG0001",
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
                        "BXG0002",
                        format!("input[{index}] duplicates input[{first}]"),
                        "input logical paths must be unique",
                    ));
                } else {
                    input_paths.insert(path.clone(), index);
                }
                if requires_utf8(&path) && std::str::from_utf8(&bytes).is_err() {
                    errors.push(request_diagnostic(
                        path.clone(),
                        "BXG0003",
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
                code: "BXG0015",
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
                "BXG0004",
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
                    "BXG0006",
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
                        "BXG0005",
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
    code: &'static str,
    offending: String,
    rule: &'static str,
    rule_source: &'static str,
}
impl Diagnostic {
    copy_getters! {
        #[doc = "Returns the stable `BXG####` code."] code: &'static str = code;
        #[doc = "Returns the source span."] span: Span = span;
    }
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
            self.code,
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
                "BXG0001",
                format!("{kind}[{index}] logical path"),
                "logical paths must be forward-slash relative",
            ));
            None
        }
    }
}

fn request_diagnostic(
    path: RelativePath,
    code: &'static str,
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
}
