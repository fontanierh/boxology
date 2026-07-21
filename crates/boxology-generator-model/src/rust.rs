use super::{Diagnostic, Diagnostics, GenerationRequest, LineColumn, RelativePath, Span};

const RULE: &str = "every declared .rs input must parse as a complete Rust file";
const RULE_SOURCE: &str = "specs/s2-contract-generator.md D2";

/// Every successfully parsed Rust input, sorted by logical-path bytes.
pub struct ParsedRustInputs(Vec<ParsedRustInput>);

/// One logical Rust input and its complete parsed syntax tree.
pub struct ParsedRustInput {
    path: RelativePath,
    syntax: syn::File,
}

impl ParsedRustInputs {
    /// Parses every exact `.rs` request input and aggregates all syntax failures.
    pub fn parse(request: &GenerationRequest) -> Result<Self, Diagnostics> {
        let mut inputs = request
            .inputs()
            .iter()
            .filter(|input| input.path().as_str().ends_with(".rs"))
            .collect::<Vec<_>>();
        inputs.sort_by(|left, right| {
            left.path()
                .as_str()
                .as_bytes()
                .cmp(right.path().as_str().as_bytes())
        });

        let mut parsed = Vec::with_capacity(inputs.len());
        let mut diagnostics = Vec::new();
        for input in inputs {
            let source = std::str::from_utf8(input.bytes())
                .expect("GenerationRequest guarantees Rust inputs are valid UTF-8");
            match syn::parse_file(source) {
                Ok(syntax) => parsed.push(ParsedRustInput {
                    path: input.path().clone(),
                    syntax,
                }),
                Err(error) => append_errors(input.path(), error, &mut diagnostics),
            }
        }
        if diagnostics.is_empty() {
            Ok(Self(parsed))
        } else {
            diagnostics.sort();
            Err(Diagnostics(diagnostics))
        }
    }

    /// Returns parsed inputs in logical-path byte order.
    pub fn as_slice(&self) -> &[ParsedRustInput] {
        &self.0
    }
}

impl ParsedRustInput {
    /// Returns the exact validated logical input path.
    pub fn path(&self) -> &RelativePath {
        &self.path
    }

    /// Returns the complete parsed Rust syntax tree.
    pub fn syntax(&self) -> &syn::File {
        &self.syntax
    }
}

fn append_errors(path: &RelativePath, error: syn::Error, diagnostics: &mut Vec<Diagnostic>) {
    for component in error {
        let upstream = component.span();
        let (start, end) = (upstream.start(), upstream.end());
        diagnostics.push(Diagnostic {
            path: path.clone(),
            span: Span {
                start: LineColumn {
                    line: start.line,
                    column: start.column + 1,
                },
                end: LineColumn {
                    line: end.line,
                    column: end.column + 1,
                },
            },
            code: "BXG0014",
            offending: "Rust source syntax".into(),
            rule: RULE,
            rule_source: RULE_SOURCE,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::BoxId;

    fn request(root: &str, files: &[(&str, &str)]) -> GenerationRequest {
        let mut inputs = vec![(
            "boxology.toml".into(),
            b"schema = 1\nid = \"demo\"\nkind = \"box\"\n".to_vec(),
        )];
        inputs.extend(
            files
                .iter()
                .map(|(path, source)| ((*path).into(), source.as_bytes().to_vec())),
        );
        GenerationRequest::new(
            BoxId::new("demo").unwrap(),
            root.into(),
            inputs,
            vec![],
            vec![],
        )
        .unwrap()
    }

    fn parse_errors(request: &GenerationRequest) -> Diagnostics {
        match ParsedRustInputs::parse(request) {
            Ok(_) => panic!("expected Rust syntax diagnostics"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn span(start: (usize, usize), end: (usize, usize)) -> Span {
        Span {
            start: LineColumn {
                line: start.0,
                column: start.1,
            },
            end: LineColumn {
                line: end.0,
                column: end.1,
            },
        }
    }

    #[test]
    fn valid_inputs_are_byte_sorted_and_retain_inspectable_files() {
        let request = request(
            "a.rs",
            &[("z.rs", "fn z() {}\n"), ("a.rs", "struct A;\nfn a() {}\n")],
        );
        let parsed = ParsedRustInputs::parse(&request).unwrap();
        assert_eq!(
            parsed
                .as_slice()
                .iter()
                .map(|input| (input.path().as_str(), input.syntax().items.len()))
                .collect::<Vec<_>>(),
            [("a.rs", 2), ("z.rs", 1)]
        );
        assert!(matches!(
            parsed.as_slice()[0].syntax().items[0],
            syn::Item::Struct(_)
        ));
    }

    #[test]
    fn unicode_identifier_bom_and_shebang_parse_as_a_complete_file() {
        let request = request(
            "unicode.rs",
            &[(
                "unicode.rs",
                "\u{feff}#!/usr/bin/env rust-script\nfn café() {}\n",
            )],
        );
        let parsed = ParsedRustInputs::parse(&request).unwrap();
        let syntax = parsed.as_slice()[0].syntax();
        assert_eq!(
            syntax.shebang.as_deref(),
            Some("#!/usr/bin/env rust-script")
        );
        assert_eq!(syntax.items.len(), 1);
    }

    #[test]
    fn non_rust_suffix_is_ignored_while_a_valid_root_exists() {
        let request = request("root.rs", &[("notes.rs.bak", "@\n"), ("root.rs", "")]);
        let parsed = ParsedRustInputs::parse(&request).unwrap();
        assert_eq!(
            parsed
                .as_slice()
                .iter()
                .map(|input| input.path().as_str())
                .collect::<Vec<_>>(),
            ["root.rs"]
        );
    }

    #[test]
    fn multifile_failures_are_complete_sorted_exact_safe_and_repeatable() {
        let request = request(
            "a.rs",
            &[
                ("b.rs", "fn café() { @ }\n"),
                ("a.rs", "fn good() {}\nfn bad() { @ }\n"),
            ],
        );
        let (first, second) = (parse_errors(&request), parse_errors(&request));
        assert_eq!(first, second);
        let diagnostics = first.as_slice();
        assert_eq!(diagnostics.len(), 2);
        for (diagnostic, path, expected_span) in [
            (&diagnostics[0], "a.rs", span((2, 12), (2, 13))),
            (&diagnostics[1], "b.rs", span((1, 13), (1, 14))),
        ] {
            assert_eq!(diagnostic.code(), "BXG0014");
            assert_eq!(diagnostic.path().as_str(), path);
            assert_eq!(diagnostic.span(), expected_span);
            assert_eq!(diagnostic.offending_construct(), "Rust source syntax");
            assert_eq!(diagnostic.rule(), RULE);
            assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
            assert_eq!(
                diagnostic.to_string(),
                format!(
                    "BXG0014 {path}:{}:{}-{}:{} offending=\"Rust source syntax\" rule=\"{RULE}\" source=\"{RULE_SOURCE}\"",
                    expected_span.start().line(),
                    expected_span.start().column(),
                    expected_span.end().line(),
                    expected_span.end().column()
                )
            );
            assert!(!diagnostic.to_string().contains(['\r', '\n']));
        }
        let display = first.to_string();
        for payload in ["good", "bad", "café", "@", "expected expression"] {
            assert!(!display.contains(payload));
        }
    }

    #[test]
    fn combined_syn_error_components_are_all_aggregated() {
        let mut error = syn::Error::new(proc_macro2::Span::call_site(), "first payload");
        error.combine(syn::Error::new(
            proc_macro2::Span::call_site(),
            "second payload",
        ));
        let mut diagnostics = Vec::new();
        append_errors(&RelativePath("combined.rs".into()), error, &mut diagnostics);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(|error| error.code() == "BXG0014"));
        assert!(!format!("{:?}", diagnostics).contains("payload"));
    }
}
