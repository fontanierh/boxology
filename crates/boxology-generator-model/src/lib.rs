//! Pure in-memory request and diagnostic types shared by Boxology generators.
//!
//! Callers provide every identity, logical path, and byte. This crate consults no filesystem,
//! environment, network, locale, or clock.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::BoxId;
use std::fmt;

const RULE_SOURCE: &str = "specs/s2-contract-generator.md D1";
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
    inputs: Vec<InputFile>,
    imports: Vec<DeclaredImport>,
    outputs: Vec<RelativePath>,
}
impl GenerationRequest {
    /// Validates every logical path and preserves caller-provided member order and bytes.
    pub fn new(
        box_id: BoxId,
        raw_inputs: Vec<(String, Vec<u8>)>,
        raw_imports: Vec<(BoxId, String)>,
        raw_outputs: Vec<String>,
    ) -> Result<Self, Diagnostics> {
        let mut errors = Vec::new();
        let mut inputs = Vec::new();
        for (index, (raw_path, bytes)) in raw_inputs.into_iter().enumerate() {
            if let Some(path) = checked_path(raw_path, "input", index, &mut errors) {
                inputs.push(InputFile { path, bytes });
            }
        }
        let mut imports = Vec::new();
        for (index, (package, raw_path)) in raw_imports.into_iter().enumerate() {
            if let Some(schema_path) = checked_path(raw_path, "import", index, &mut errors) {
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
            inputs,
            imports,
            outputs,
        })
    }

    ref_getters! {
        #[doc = "Returns the manifest-provided box identity."] box_id: &BoxId = box_id;
        #[doc = "Returns inputs in caller-provided order."] inputs: &[InputFile] = inputs;
        #[doc = "Returns declared imports in caller-provided order."] imports: &[DeclaredImport] = imports;
        #[doc = "Returns declared outputs in caller-provided order."] outputs: &[RelativePath] = outputs;
    }
}
/// A one-based source coordinate.
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
            errors.push(Diagnostic {
                path: RelativePath("<request>".into()),
                span: REQUEST_SPAN,
                code: "BXG0001",
                offending: format!("{kind}[{index}] logical path"),
                rule: "logical paths must be forward-slash relative",
                rule_source: RULE_SOURCE,
            });
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn id(value: &str) -> BoxId {
        BoxId::new(value).unwrap()
    }
    fn invalid_request() -> Diagnostics {
        GenerationRequest::new(
            id("demo"),
            vec![
                ("boxology.toml".into(), b"ok\n".to_vec()),
                ("/secret/input".into(), vec![]),
            ],
            vec![(id("foreign"), "schema/..".into())],
            vec!["bad\\output".into()],
        )
        .unwrap_err()
    }

    #[test]
    fn request_preserves_exact_values_and_path_grammar() {
        let request = GenerationRequest::new(
            id("demo"),
            vec![
                ("boxology.toml".into(), b"manifest\n".to_vec()),
                ("../foreign/schema.json".into(), b"{}\n".to_vec()),
            ],
            vec![(id("foreign"), "../foreign/schema.json".into())],
            vec!["generated/schema.json".into()],
        )
        .unwrap();
        assert_eq!(request.box_id().as_str(), "demo");
        assert_eq!(request.inputs()[0].path().as_str(), "boxology.toml");
        assert_eq!(request.inputs()[0].bytes(), b"manifest\n");
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
    fn public_request_seam_is_send_sync_static() {
        fn bounds<T: Send + Sync + 'static>() {}
        bounds::<(RelativePath, InputFile, DeclaredImport, GenerationRequest)>();
        bounds::<(LineColumn, Span, Diagnostic, Diagnostics)>();
    }
}
