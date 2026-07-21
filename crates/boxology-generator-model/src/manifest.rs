use super::{Diagnostic, Diagnostics, GenerationRequest, LineColumn, POINT, RelativePath, Span};
use boxology_contract::BoxId;
use std::ops::Range;
#[allow(deprecated)]
use toml_edit::ImDocument;
use toml_edit::Item;

const MANIFEST_PATH: &str = "boxology.toml";
const PACKAGES_SOURCE: &str = "boxology-details/02-packages.md";
const D1_SOURCE: &str = "specs/s2-contract-generator.md D1";
const D4_SOURCE: &str = "specs/s2-contract-generator.md D4";

/// The validated generation-subject identity read from `boxology.toml`.
#[derive(Debug, Eq, PartialEq)]
pub struct Manifest {
    id: BoxId,
}

impl Manifest {
    /// Returns the validated manifest package id.
    pub fn id(&self) -> &BoxId {
        &self.id
    }

    /// Parses the request's manifest and validates its schema, id, and box kind.
    pub fn parse(request: &GenerationRequest) -> Result<Manifest, Diagnostics> {
        let input = request
            .inputs()
            .iter()
            .find(|input| input.path().as_str() == MANIFEST_PATH)
            .expect("GenerationRequest guarantees a boxology.toml input");
        let source = std::str::from_utf8(input.bytes())
            .expect("GenerationRequest guarantees boxology.toml is valid UTF-8");
        #[allow(deprecated)]
        let document = match ImDocument::parse(source) {
            Ok(document) => document,
            Err(error) => {
                return Err(Diagnostics(vec![diagnostic(
                    "BXG0007",
                    source_span(source, error.span()),
                    "manifest TOML syntax".into(),
                    "boxology.toml must be well-formed TOML",
                    PACKAGES_SOURCE,
                )]));
            }
        };

        let schema_item = document.get("schema");
        let Some(schema) = schema_item.and_then(Item::as_integer) else {
            return Err(Diagnostics(vec![diagnostic(
                "BXG0008",
                item_span(source, schema_item),
                "manifest key schema".into(),
                "the manifest must declare an integer schema version",
                PACKAGES_SOURCE,
            )]));
        };
        if schema != 1 {
            return Err(Diagnostics(vec![diagnostic(
                "BXG0009",
                item_span(source, schema_item),
                "manifest key schema".into(),
                "the generator reads manifest schema version 1 and must reject others",
                PACKAGES_SOURCE,
            )]));
        }

        let mut errors = Vec::new();
        let id_item = document.get("id");
        let id = match id_item.and_then(Item::as_str) {
            None => {
                errors.push(diagnostic(
                    "BXG0010",
                    item_span(source, id_item),
                    "manifest key id".into(),
                    "the manifest must declare a string package id",
                    PACKAGES_SOURCE,
                ));
                None
            }
            Some(raw_id) => match BoxId::new(raw_id) {
                Err(_) => {
                    errors.push(diagnostic(
                        "BXG0011",
                        item_span(source, id_item),
                        "manifest key id".into(),
                        "the package id must match [a-z][a-z0-9-]*",
                        D4_SOURCE,
                    ));
                    None
                }
                Ok(id) => {
                    if &id != request.box_id() {
                        errors.push(diagnostic(
                            "BXG0012",
                            item_span(source, id_item),
                            format!(
                                "manifest id {id} differs from request box identity {}",
                                request.box_id()
                            ),
                            "the manifest package id must equal the request box identity",
                            D4_SOURCE,
                        ));
                    }
                    Some(id)
                }
            },
        };

        let kind_item = document.get("kind");
        if kind_item.and_then(Item::as_str) != Some("box") {
            errors.push(diagnostic(
                "BXG0013",
                item_span(source, kind_item),
                "manifest key kind".into(),
                "the generation subject must declare kind = \"box\"",
                D1_SOURCE,
            ));
        }
        if !errors.is_empty() {
            errors.sort();
            return Err(Diagnostics(errors));
        }
        Ok(Manifest {
            id: id.expect("validated manifest id exists when diagnostics are empty"),
        })
    }
}

fn diagnostic(
    code: &'static str,
    span: Span,
    offending: String,
    rule: &'static str,
    rule_source: &'static str,
) -> Diagnostic {
    Diagnostic {
        path: RelativePath(MANIFEST_PATH.into()),
        span,
        code,
        offending,
        rule,
        rule_source,
    }
}

fn item_span(source: &str, item: Option<&Item>) -> Span {
    source_span(source, item.and_then(Item::span))
}

fn source_span(source: &str, range: Option<Range<usize>>) -> Span {
    range.map_or(
        Span {
            start: POINT,
            end: POINT,
        },
        |range| Span {
            start: coordinate(source, range.start),
            end: coordinate(source, range.end),
        },
    )
}

fn coordinate(source: &str, offset: usize) -> LineColumn {
    let prefix = source
        .get(..offset)
        .expect("TOML spans end at UTF-8 character boundaries");
    LineColumn {
        line: prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(id: &str, manifest: &str) -> GenerationRequest {
        GenerationRequest::new(
            BoxId::new(id).unwrap(),
            "root.rs".into(),
            vec![
                (MANIFEST_PATH.into(), manifest.as_bytes().to_vec()),
                ("root.rs".into(), vec![]),
            ],
            vec![],
            vec![],
        )
        .unwrap()
    }
    fn errors(id: &str, manifest: &str) -> Diagnostics {
        Manifest::parse(&request(id, manifest)).unwrap_err()
    }
    fn at(start: (usize, usize), end: (usize, usize)) -> Span {
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
    fn assert_one(diagnostics: &Diagnostics, code: &str, span: Span, rule_source: &str) {
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one diagnostic, got {diagnostics:?}");
        };
        assert_eq!(diagnostic.code(), code);
        assert_eq!(diagnostic.path().as_str(), MANIFEST_PATH);
        assert_eq!(diagnostic.span(), span);
        assert_eq!(diagnostic.rule_source(), rule_source);
        assert!(!diagnostic.offending_construct().is_empty());
        assert!(!diagnostic.rule().is_empty());
        assert!(!diagnostic.to_string().contains(['\n', '\r']));
    }
    #[test]
    fn parses_documented_extra_keys_and_tables_without_owning_them() {
        let manifest = r#"schema = 1
id = "demo"
kind = "box"
display_name = "Demo"
owned = ["src/**"]

[quality]
commands = ["cargo test"]

[[crates]]
path = "crates/demo"
package = "demo"
role = "box-implementation"
"#;
        assert_eq!(
            Manifest::parse(&request("demo", manifest)).unwrap().id(),
            &BoxId::new("demo").unwrap()
        );
    }

    #[test]
    fn minimal_manifest_parses_deterministically() {
        let source = "schema = 1\nid = \"demo\"\nkind = \"box\"\n";
        let first = Manifest::parse(&request("demo", source)).unwrap();
        let second = Manifest::parse(&request("demo", source)).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn bxg0007_reports_the_parser_span_and_stops_field_checks() {
        let diagnostics = errors("demo", "schema = 1\nid = \"demo");
        assert_one(
            &diagnostics,
            "BXG0007",
            at((2, 11), (2, 11)),
            PACKAGES_SOURCE,
        );
    }

    #[test]
    fn bxg0008_reports_missing_and_non_integer_schema() {
        for (manifest, span) in [
            ("id = \"demo\"\nkind = \"box\"\n", at((1, 1), (1, 1))),
            (
                "schema = \"1\"\nid = \"demo\"\nkind = \"box\"\n",
                at((1, 10), (1, 13)),
            ),
        ] {
            assert_one(&errors("demo", manifest), "BXG0008", span, PACKAGES_SOURCE);
        }
    }

    #[test]
    fn bxg0009_rejects_unknown_schema_before_id_and_kind_checks() {
        let diagnostics = errors(
            "demo",
            "schema = 2\nid = \"Bad_Id\"\nkind = \"composition\"\n",
        );
        assert_one(
            &diagnostics,
            "BXG0009",
            at((1, 10), (1, 11)),
            PACKAGES_SOURCE,
        );
    }

    #[test]
    fn bxg0010_reports_missing_and_non_string_id() {
        for (manifest, span) in [
            ("schema = 1\nkind = \"box\"\n", at((1, 1), (1, 1))),
            ("schema = 1\nid = 7\nkind = \"box\"\n", at((2, 6), (2, 7))),
        ] {
            assert_one(&errors("demo", manifest), "BXG0010", span, PACKAGES_SOURCE);
        }
    }

    #[test]
    fn bxg0011_rejects_invalid_ids_without_leaking_or_mismatch_noise() {
        let invalid = errors("demo", "schema = 1\nid = \"Bad_Id\"\nkind = \"box\"\n");
        assert_one(&invalid, "BXG0011", at((2, 6), (2, 14)), D4_SOURCE);
        assert!(!invalid.to_string().contains("Bad_Id"));

        let control = errors("demo", "schema = 1\nid = \"bad\\u000A\"\nkind = \"box\"\n");
        assert_one(&control, "BXG0011", at((2, 6), (2, 17)), D4_SOURCE);
        assert!(!control.to_string().contains("bad"));

        let unicode = errors("demo", "schema = 1\nid = \"é\"\nkind = \"box\"\n");
        assert_one(&unicode, "BXG0011", at((2, 6), (2, 9)), D4_SOURCE);
    }

    #[test]
    fn bxg0012_reports_only_safe_valid_id_mismatch_values() {
        let diagnostics = errors("demo", "schema = 1\nid = \"other\"\nkind = \"box\"\n");
        assert_one(&diagnostics, "BXG0012", at((2, 6), (2, 13)), D4_SOURCE);
        assert!(
            diagnostics
                .to_string()
                .contains("manifest id other differs from request box identity demo")
        );
    }

    #[test]
    fn bxg0013_reports_missing_and_non_box_kind() {
        for (manifest, span) in [
            ("schema = 1\nid = \"demo\"\n", at((1, 1), (1, 1))),
            (
                "schema = 1\nid = \"demo\"\nkind = \"composition\"\n",
                at((3, 8), (3, 21)),
            ),
        ] {
            assert_one(&errors("demo", manifest), "BXG0013", span, D1_SOURCE);
        }
    }

    #[test]
    fn field_diagnostics_are_complete_sorted_and_deterministic() {
        let source = "schema = 1\nid = \"Bad_Id\"\nkind = \"composition\"\n";
        let (first, second) = (errors("demo", source), errors("demo", source));
        assert_eq!(first, second);
        assert_eq!(
            first
                .as_slice()
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            ["BXG0011", "BXG0013"]
        );
        assert!(first.as_slice().windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
