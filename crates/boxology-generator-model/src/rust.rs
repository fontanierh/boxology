use super::{Diagnostic, Diagnostics, GenerationRequest, LineColumn, RelativePath, Span};
use syn::ext::IdentExt;

const RULE: &str = "every declared .rs input must parse as a complete Rust file";
const RULE_SOURCE: &str = "specs/s2-contract-generator.md D2";
const PATH_RULE: &str = "#[path] module overrides are not supported in v0";
const MISSING_RULE: &str =
    "outline module lookup must find x.rs or x/mod.rs among declared Rust inputs";
const AMBIGUOUS_RULE: &str =
    "outline module lookup must not find both x.rs and x/mod.rs among declared Rust inputs";

/// Every successfully parsed Rust input, sorted by logical-path bytes.
pub struct ParsedRustInputs {
    inputs: Vec<ParsedRustInput>,
    crate_root: usize,
}

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
        let crate_root = inputs
            .iter()
            .position(|input| input.path() == request.crate_root())
            .expect("GenerationRequest guarantees the crate root is a declared Rust input");

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
            Ok(Self {
                inputs: parsed,
                crate_root,
            })
        } else {
            diagnostics.sort();
            Err(Diagnostics(diagnostics))
        }
    }

    /// Returns parsed inputs in logical-path byte order.
    pub fn as_slice(&self) -> &[ParsedRustInput] {
        &self.inputs
    }

    /// Validates default module lookup and returns unique reachable files in logical-path byte order.
    pub fn resolve_reachable_inputs(&self) -> Result<Vec<&ParsedRustInput>, Diagnostics> {
        let root = &self.inputs[self.crate_root];
        let module_dir = root
            .path
            .as_str()
            .rsplit_once('/')
            .map_or("", |pair| pair.0);
        let mut reachable = vec![false; self.inputs.len()];
        reachable[self.crate_root] = true;
        let mut diagnostics = Vec::new();
        self.visit_items(
            self.crate_root,
            &root.syntax.items,
            module_dir,
            &mut reachable,
            &mut diagnostics,
        );
        if diagnostics.is_empty() {
            Ok(self
                .inputs
                .iter()
                .zip(reachable)
                .filter_map(|(input, reachable)| reachable.then_some(input))
                .collect())
        } else {
            diagnostics.sort();
            Err(Diagnostics(diagnostics))
        }
    }

    fn visit_items(
        &self,
        source: usize,
        items: &[syn::Item],
        module_dir: &str,
        reachable: &mut [bool],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for module in items.iter().filter_map(|item| match item {
            syn::Item::Mod(module) => Some(module),
            _ => None,
        }) {
            let mut has_path = false;
            for attribute in &module.attrs {
                let Some(identifier) = attribute.path().get_ident() else {
                    continue;
                };
                if identifier.unraw() == "path" {
                    has_path = true;
                    diagnostics.push(module_diagnostic(
                        &self.inputs[source].path,
                        identifier.span(),
                        "BXG0016",
                        "module path override",
                        PATH_RULE,
                    ));
                }
            }
            if has_path {
                continue;
            }

            let name = module.ident.unraw().to_string();
            let child_dir = if module_dir.is_empty() {
                name
            } else {
                format!("{module_dir}/{name}")
            };
            if let Some((_, items)) = &module.content {
                self.visit_items(source, items, &child_dir, reachable, diagnostics);
                continue;
            }

            let direct = self.find(&format!("{child_dir}.rs"));
            let nested = self.find(&format!("{child_dir}/mod.rs"));
            let target = match (direct, nested) {
                (None, None) => {
                    diagnostics.push(module_diagnostic(
                        &self.inputs[source].path,
                        module.ident.span(),
                        "BXG0017",
                        "missing outline module input",
                        MISSING_RULE,
                    ));
                    continue;
                }
                (Some(_), Some(_)) => {
                    diagnostics.push(module_diagnostic(
                        &self.inputs[source].path,
                        module.ident.span(),
                        "BXG0018",
                        "ambiguous outline module inputs",
                        AMBIGUOUS_RULE,
                    ));
                    continue;
                }
                (Some(target), None) | (None, Some(target)) => target,
            };
            if !reachable[target] {
                reachable[target] = true;
                self.visit_items(
                    target,
                    &self.inputs[target].syntax.items,
                    &child_dir,
                    reachable,
                    diagnostics,
                );
            }
        }
    }

    fn find(&self, path: &str) -> Option<usize> {
        self.inputs
            .binary_search_by(|input| input.path.as_str().as_bytes().cmp(path.as_bytes()))
            .ok()
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
        diagnostics.push(Diagnostic {
            path: path.clone(),
            span: source_span(component.span()),
            code: "BXG0014",
            offending: "Rust source syntax".into(),
            rule: RULE,
            rule_source: RULE_SOURCE,
        });
    }
}

fn module_diagnostic(
    path: &RelativePath,
    span: proc_macro2::Span,
    code: &'static str,
    offending: &'static str,
    rule: &'static str,
) -> Diagnostic {
    Diagnostic {
        path: path.clone(),
        span: source_span(span),
        code,
        offending: offending.into(),
        rule,
        rule_source: RULE_SOURCE,
    }
}

fn source_span(upstream: proc_macro2::Span) -> Span {
    let (start, end) = (upstream.start(), upstream.end());
    Span {
        start: LineColumn {
            line: start.line,
            column: start.column + 1,
        },
        end: LineColumn {
            line: end.line,
            column: end.column + 1,
        },
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

    fn resolution_errors(request: &GenerationRequest) -> Diagnostics {
        let parsed = ParsedRustInputs::parse(request).unwrap();
        match parsed.resolve_reachable_inputs() {
            Ok(_) => panic!("expected Rust module diagnostics"),
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
    fn resolves_structural_modules_from_a_nonstandard_root() {
        let request = request(
            "source/custom-entry.rs",
            &[
                ("source/unreachable.rs", "mod also_unreachable;\n"),
                ("source/type.rs", ""),
                ("source/flat/child.rs", ""),
                (
                    "source/custom-entry.rs",
                    "mod flat;\nmod directory;\nmod inline { mod deeper { mod leaf; } }\nmod r#type;\n",
                ),
                ("source/directory/mod.rs", "mod child;\n"),
                ("source/inline/deeper/leaf.rs", ""),
                ("source/flat.rs", "mod child;\n"),
                ("source/directory/child.rs", ""),
            ],
        );
        let parsed = ParsedRustInputs::parse(&request).unwrap();
        assert!(
            parsed
                .as_slice()
                .iter()
                .any(|input| input.path().as_str() == "source/unreachable.rs")
        );
        let paths = parsed
            .resolve_reachable_inputs()
            .unwrap()
            .iter()
            .map(|input| input.path().as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            [
                "source/custom-entry.rs",
                "source/directory/child.rs",
                "source/directory/mod.rs",
                "source/flat.rs",
                "source/flat/child.rs",
                "source/inline/deeper/leaf.rs",
                "source/type.rs",
            ]
        );
        assert!(
            paths
                .windows(2)
                .all(|pair| pair[0].as_bytes() < pair[1].as_bytes())
        );
    }

    #[test]
    fn module_resolution_diagnostics_are_exact_and_safe() {
        let cases = [
            (
                &[(
                    "root.rs",
                    "#[path = \"never-print-this.rs\"] mod redirected;\n",
                )][..],
                "BXG0016",
                span((1, 3), (1, 7)),
                "module path override",
                PATH_RULE,
            ),
            (
                &[("root.rs", "mod missing;\n")][..],
                "BXG0017",
                span((1, 5), (1, 12)),
                "missing outline module input",
                MISSING_RULE,
            ),
            (
                &[
                    ("duplicate/mod.rs", ""),
                    ("root.rs", "mod duplicate;\n"),
                    ("duplicate.rs", ""),
                ][..],
                "BXG0018",
                span((1, 5), (1, 14)),
                "ambiguous outline module inputs",
                AMBIGUOUS_RULE,
            ),
        ];
        for (files, code, expected_span, offending, rule) in cases {
            let diagnostics = resolution_errors(&request("root.rs", files));
            assert_eq!(diagnostics.as_slice().len(), 1);
            let diagnostic = &diagnostics.as_slice()[0];
            assert_eq!(diagnostic.code(), code);
            assert_eq!(diagnostic.path().as_str(), "root.rs");
            assert_eq!(diagnostic.span(), expected_span);
            assert_eq!(diagnostic.offending_construct(), offending);
            assert_eq!(diagnostic.rule(), rule);
            assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
            assert_eq!(
                diagnostic.to_string(),
                format!(
                    "{code} root.rs:{}:{}-{}:{} offending={offending:?} rule={rule:?} source={RULE_SOURCE:?}",
                    expected_span.start().line(),
                    expected_span.start().column(),
                    expected_span.end().line(),
                    expected_span.end().column()
                )
            );
            for payload in [
                "never-print-this.rs",
                "redirected",
                "mod missing",
                "duplicate",
            ] {
                assert!(!diagnostic.to_string().contains(payload));
            }
        }
    }

    #[test]
    fn resolution_errors_are_complete_sorted_repeatable_and_branch_local() {
        let request = request(
            "root.rs",
            &[
                ("redirected_payload/mod.rs", "mod hidden_payload;\n"),
                ("ambiguous_payload.rs", "mod hidden_payload;\n"),
                ("a_continuing.rs", "mod descendant_payload;\n"),
                (
                    "root.rs",
                    "mod absent_payload;\n#[r#path = \"raw-never-print-this.rs\"]\nmod redirected_payload;\nmod ambiguous_payload;\nmod a_continuing;\n",
                ),
                ("ambiguous_payload/mod.rs", "mod hidden_payload;\n"),
                ("redirected_payload.rs", "mod hidden_payload;\n"),
            ],
        );
        let (first, second) = (resolution_errors(&request), resolution_errors(&request));
        assert_eq!(first, second);
        assert_eq!(
            first
                .as_slice()
                .iter()
                .map(|diagnostic| (
                    diagnostic.path().as_str(),
                    diagnostic.span().start().line(),
                    diagnostic.code()
                ))
                .collect::<Vec<_>>(),
            [
                ("a_continuing.rs", 1, "BXG0017"),
                ("root.rs", 1, "BXG0017"),
                ("root.rs", 2, "BXG0016"),
                ("root.rs", 4, "BXG0018"),
            ]
        );
        assert_eq!(first.as_slice()[2].span(), span((2, 3), (2, 9)));
        let rendered = first.to_string();
        for payload in [
            "never-print-this.rs",
            "raw-never-print-this.rs",
            "absent_payload",
            "redirected_payload",
            "ambiguous_payload",
            "descendant_payload",
            "hidden_payload",
        ] {
            assert!(!rendered.contains(payload));
        }
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
