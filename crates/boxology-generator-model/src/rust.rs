use super::{Diagnostic, Diagnostics, GenerationRequest, LineColumn, RelativePath, Span};
use std::collections::BTreeSet;
use syn::ext::IdentExt;

const RULE: &str = "every declared .rs input must parse as a complete Rust file";
const RULE_SOURCE: &str = "specs/s2-contract-generator.md D2";
const PATH_RULE: &str = "#[path] module overrides are not supported in v0";
const MISSING_RULE: &str =
    "outline module lookup must find x.rs or x/mod.rs among declared Rust inputs";
const AMBIGUOUS_RULE: &str =
    "outline module lookup must not find both x.rs and x/mod.rs among declared Rust inputs";
const UNREACHABLE_RULE: &str =
    "Boxology-annotated items must be reachable from the declared crate root";
const CONDITIONAL_RULE: &str = "cfg and cfg_attr are forbidden on exported items, their fields or variants, surrounding impls, and ancestor module declarations";
const COLLISION_RULE: &str = "contract type names must be unique in the flat lifted namespace";
const COLLISION_RULE_SOURCE: &str = "specs/s2-contract-generator.md D2-D4";
const ATTRIBUTE_RULE: &str = "contract declarations, their fields, and variants may use only doc, direct boxology attributes, deprecated, and derive";
const DERIVE_RULE: &str =
    "contract declarations, their fields, and variants may derive only Debug, Clone, and PartialEq";

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

/// A reachable module-scope contract struct or enum found by provisional declaration discovery.
pub struct ContractDeclaration<'a> {
    source: &'a RelativePath,
    identifier_span: Span,
    module_path: Vec<String>,
    lifted_name: String,
    syntax: ContractDeclarationSyntax<'a>,
}

/// The parsed syntax belonging to a discovered contract declaration.
#[derive(Clone, Copy)]
pub enum ContractDeclarationSyntax<'a> {
    /// A contract struct declaration.
    Struct(&'a syn::ItemStruct),
    /// A contract enum declaration, including a provisionally recognized error enum.
    Enum(&'a syn::ItemEnum),
}

impl ContractDeclaration<'_> {
    /// Returns the declaration's exact logical source path.
    pub fn source(&self) -> &RelativePath {
        self.source
    }

    /// Returns the declaration identifier's one-based source span.
    pub fn identifier_span(&self) -> Span {
        self.identifier_span
    }

    /// Returns the canonical unraw module components, empty at the crate root.
    pub fn module_path(&self) -> &[String] {
        &self.module_path
    }

    /// Returns the owned declaration name in its unraw spelling.
    pub fn lifted_name(&self) -> &str {
        &self.lifted_name
    }

    /// Returns the declaration's parsed struct-or-enum syntax.
    pub fn syntax(&self) -> ContractDeclarationSyntax<'_> {
        self.syntax
    }
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

    /// Provisionally discovers reachable contract structs and enums and rejects lifted-name collisions.
    ///
    /// This phase intentionally ignores deferred placements and is not complete authoring-grammar
    /// validation.
    pub fn discover_contract_declarations(
        &self,
    ) -> Result<Vec<ContractDeclaration<'_>>, Diagnostics> {
        self.resolve_reachable_inputs()?;
        let root = &self.inputs[self.crate_root];
        let module_dir = root
            .path
            .as_str()
            .rsplit_once('/')
            .map_or("", |pair| pair.0);
        let mut visited = vec![false; self.inputs.len()];
        visited[self.crate_root] = true;
        let mut declarations = Vec::new();
        self.collect_contract_declarations(
            self.crate_root,
            &root.syntax.items,
            module_dir,
            &mut Vec::new(),
            &mut visited,
            &mut declarations,
        );
        declarations.sort_by(|left, right| {
            left.module_path
                .cmp(&right.module_path)
                .then_with(|| {
                    left.source
                        .as_str()
                        .as_bytes()
                        .cmp(right.source.as_str().as_bytes())
                })
                .then(left.identifier_span.cmp(&right.identifier_span))
        });

        let mut names = BTreeSet::new();
        let mut diagnostics = Vec::new();
        for declaration in &declarations {
            if !names.insert(declaration.lifted_name.clone()) {
                diagnostics.push(Diagnostic {
                    path: declaration.source.clone(),
                    span: declaration.identifier_span,
                    code: "BXG0021",
                    offending: "colliding lifted contract type name".into(),
                    rule: COLLISION_RULE,
                    rule_source: COLLISION_RULE_SOURCE,
                });
            }
        }
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        for declaration in &declarations {
            validate_contract_attributes(declaration, &mut diagnostics);
        }
        diagnostics.sort();
        diagnostics.dedup();
        diagnostics
            .is_empty()
            .then_some(declarations)
            .ok_or(Diagnostics(diagnostics))
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
        if !diagnostics.is_empty() {
            diagnostics.sort();
            return Err(Diagnostics(diagnostics));
        }

        for (source, input) in self.inputs.iter().enumerate() {
            if !reachable[source] {
                self.inspect_unreachable(source, &input.syntax.items, &mut diagnostics);
            }
        }
        self.validate_items(
            self.crate_root,
            &root.syntax.items,
            module_dir,
            &mut Vec::new(),
            &mut diagnostics,
        );
        diagnostics.sort();
        diagnostics.dedup();
        if !diagnostics.is_empty() {
            return Err(Diagnostics(diagnostics));
        }
        Ok(self
            .inputs
            .iter()
            .zip(reachable)
            .filter_map(|(input, reachable)| reachable.then_some(input))
            .collect())
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

    fn collect_contract_declarations<'a>(
        &'a self,
        source: usize,
        items: &'a [syn::Item],
        module_dir: &str,
        module_path: &mut Vec<String>,
        visited: &mut [bool],
        declarations: &mut Vec<ContractDeclaration<'a>>,
    ) {
        for item in items {
            let declaration = match item {
                syn::Item::Struct(item) if has_boxology(&item.attrs, "contract") => {
                    Some((&item.ident, ContractDeclarationSyntax::Struct(item)))
                }
                syn::Item::Enum(item) if has_boxology(&item.attrs, "contract") => {
                    Some((&item.ident, ContractDeclarationSyntax::Enum(item)))
                }
                _ => None,
            };
            if let Some((identifier, syntax)) = declaration {
                declarations.push(ContractDeclaration {
                    source: &self.inputs[source].path,
                    identifier_span: source_span(identifier.span()),
                    module_path: module_path.clone(),
                    lifted_name: identifier.unraw().to_string(),
                    syntax,
                });
            }

            let syn::Item::Mod(module) = item else {
                continue;
            };
            let name = module.ident.unraw().to_string();
            let child_dir = if module_dir.is_empty() {
                name.clone()
            } else {
                format!("{module_dir}/{name}")
            };
            module_path.push(name);
            if let Some((_, items)) = &module.content {
                self.collect_contract_declarations(
                    source,
                    items,
                    &child_dir,
                    module_path,
                    visited,
                    declarations,
                );
            } else {
                let target = self
                    .find(&format!("{child_dir}.rs"))
                    .or_else(|| self.find(&format!("{child_dir}/mod.rs")))
                    .expect("structural validation guarantees one outline target");
                if !visited[target] {
                    visited[target] = true;
                    self.collect_contract_declarations(
                        target,
                        &self.inputs[target].syntax.items,
                        &child_dir,
                        module_path,
                        visited,
                        declarations,
                    );
                }
            }
            module_path.pop();
        }
    }

    fn inspect_unreachable(
        &self,
        source: usize,
        items: &[syn::Item],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for item in items {
            add_dead(&self.inputs[source].path, item_attrs(item), diagnostics);
            if let syn::Item::Mod(module) = item
                && let Some((_, items)) = &module.content
            {
                self.inspect_unreachable(source, items, diagnostics);
            }
            if let syn::Item::Impl(implementation) = item {
                for item in &implementation.items {
                    add_dead(&self.inputs[source].path, impl_attrs(item), diagnostics);
                }
            }
        }
    }

    fn validate_items<'a>(
        &'a self,
        source: usize,
        items: &'a [syn::Item],
        module_dir: &str,
        ancestors: &mut Vec<(usize, &'a syn::ItemMod)>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for item in items {
            let attributes = item_attrs(item);
            if is_export(attributes) {
                self.validate_context(source, attributes, ancestors, diagnostics);
                if has_boxology(attributes, "contract") {
                    match item {
                        syn::Item::Struct(item) => {
                            for field in &item.fields {
                                self.add_conditionals(source, &field.attrs, diagnostics);
                            }
                        }
                        syn::Item::Enum(item) => {
                            for variant in &item.variants {
                                self.add_conditionals(source, &variant.attrs, diagnostics);
                                for field in &variant.fields {
                                    self.add_conditionals(source, &field.attrs, diagnostics);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            if let syn::Item::Impl(implementation) = item {
                for item in &implementation.items {
                    let attributes = impl_attrs(item);
                    if is_export(attributes) {
                        self.validate_context(source, attributes, ancestors, diagnostics);
                        self.add_conditionals(source, &implementation.attrs, diagnostics);
                    }
                }
            }
            let syn::Item::Mod(module) = item else {
                continue;
            };
            let name = module.ident.unraw().to_string();
            let child_dir = if module_dir.is_empty() {
                name
            } else {
                format!("{module_dir}/{name}")
            };
            ancestors.push((source, module));
            if let Some((_, items)) = &module.content {
                self.validate_items(source, items, &child_dir, ancestors, diagnostics);
            } else {
                let target = self
                    .find(&format!("{child_dir}.rs"))
                    .or_else(|| self.find(&format!("{child_dir}/mod.rs")))
                    .expect("structural validation guarantees one outline target");
                self.validate_items(
                    target,
                    &self.inputs[target].syntax.items,
                    &child_dir,
                    ancestors,
                    diagnostics,
                );
            }
            ancestors.pop();
        }
    }

    fn validate_context(
        &self,
        source: usize,
        attributes: &[syn::Attribute],
        ancestors: &[(usize, &syn::ItemMod)],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        self.add_conditionals(source, attributes, diagnostics);
        for &(source, module) in ancestors {
            self.add_conditionals(source, &module.attrs, diagnostics);
        }
    }

    fn add_conditionals(
        &self,
        source: usize,
        attributes: &[syn::Attribute],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        attributes
            .iter()
            .filter_map(|attribute| conditional(attribute).map(|name| (attribute, name)))
            .for_each(|(attribute, offending)| {
                diagnostics.push(module_diagnostic(
                    &self.inputs[source].path,
                    attribute_span(attribute, false),
                    "BXG0020",
                    offending,
                    CONDITIONAL_RULE,
                ));
            });
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

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(syn::ItemConst { attrs, .. })
        | syn::Item::Enum(syn::ItemEnum { attrs, .. })
        | syn::Item::ExternCrate(syn::ItemExternCrate { attrs, .. })
        | syn::Item::Fn(syn::ItemFn { attrs, .. })
        | syn::Item::ForeignMod(syn::ItemForeignMod { attrs, .. })
        | syn::Item::Impl(syn::ItemImpl { attrs, .. })
        | syn::Item::Macro(syn::ItemMacro { attrs, .. })
        | syn::Item::Mod(syn::ItemMod { attrs, .. })
        | syn::Item::Static(syn::ItemStatic { attrs, .. })
        | syn::Item::Struct(syn::ItemStruct { attrs, .. })
        | syn::Item::Trait(syn::ItemTrait { attrs, .. })
        | syn::Item::TraitAlias(syn::ItemTraitAlias { attrs, .. })
        | syn::Item::Type(syn::ItemType { attrs, .. })
        | syn::Item::Union(syn::ItemUnion { attrs, .. })
        | syn::Item::Use(syn::ItemUse { attrs, .. }) => attrs,
        _ => &[],
    }
}

fn impl_attrs(item: &syn::ImplItem) -> &[syn::Attribute] {
    match item {
        syn::ImplItem::Const(syn::ImplItemConst { attrs, .. })
        | syn::ImplItem::Fn(syn::ImplItemFn { attrs, .. })
        | syn::ImplItem::Type(syn::ImplItemType { attrs, .. })
        | syn::ImplItem::Macro(syn::ImplItemMacro { attrs, .. }) => attrs,
        _ => &[],
    }
}

fn boxology_leaf(attribute: &syn::Attribute) -> Option<&syn::Ident> {
    let path = attribute.path();
    if !matches!(&attribute.style, syn::AttrStyle::Outer)
        || path.segments.len() != 2
        || path
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
        || path.segments[0].ident.unraw() != "boxology"
    {
        return None;
    }
    Some(&path.segments[1].ident)
}

fn add_dead(path: &RelativePath, attributes: &[syn::Attribute], diagnostics: &mut Vec<Diagnostic>) {
    if let Some(attribute) = attributes
        .iter()
        .find(|attribute| boxology_leaf(attribute).is_some())
    {
        diagnostics.push(module_diagnostic(
            path,
            attribute_span(attribute, true),
            "BXG0019",
            "Boxology-annotated item",
            UNREACHABLE_RULE,
        ));
    }
}

fn has_boxology(attributes: &[syn::Attribute], leaf: &str) -> bool {
    attributes.iter().any(|attribute| {
        boxology_leaf(attribute).is_some_and(|identifier| identifier.unraw() == leaf)
    })
}

fn is_export(attributes: &[syn::Attribute]) -> bool {
    has_boxology(attributes, "contract") || has_boxology(attributes, "capability")
}

fn conditional(attribute: &syn::Attribute) -> Option<&'static str> {
    let path = attribute.path();
    if path.segments.len() != 1 || !matches!(path.segments[0].arguments, syn::PathArguments::None) {
        return None;
    }
    let identifier = path.segments[0].ident.unraw();
    (identifier == "cfg")
        .then_some("cfg attribute")
        .or_else(|| (identifier == "cfg_attr").then_some("cfg_attr attribute"))
}

fn attribute_span(attribute: &syn::Attribute, close_path: bool) -> proc_macro2::Span {
    let path = attribute.path();
    let start = path
        .leading_colon
        .as_ref()
        .map_or_else(|| path.segments[0].ident.span(), |colon| colon.spans[0]);
    let end = if close_path && matches!(&attribute.meta, syn::Meta::Path(_)) {
        attribute.bracket_token.span.close()
    } else {
        path.segments.last().unwrap().ident.span()
    };
    start.join(end).expect("attribute path spans one source")
}

fn validate_contract_attributes(
    declaration: &ContractDeclaration<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = declaration.source;
    match declaration.syntax {
        ContractDeclarationSyntax::Struct(item) => {
            validate_attributes(path, &item.attrs, diagnostics);
            for field in &item.fields {
                validate_attributes(path, &field.attrs, diagnostics);
            }
        }
        ContractDeclarationSyntax::Enum(item) => {
            validate_attributes(path, &item.attrs, diagnostics);
            for variant in &item.variants {
                validate_attributes(path, &variant.attrs, diagnostics);
                for field in &variant.fields {
                    validate_attributes(path, &field.attrs, diagnostics);
                }
            }
        }
    }
}

fn validate_attributes(
    path: &RelativePath,
    attributes: &[syn::Attribute],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for attribute in attributes {
        if boxology_leaf(attribute).is_some() {
            continue;
        }
        let name = attribute
            .path()
            .get_ident()
            .filter(|_| matches!(attribute.style, syn::AttrStyle::Outer))
            .map(|identifier| identifier.unraw().to_string());
        match name.as_deref() {
            Some("doc" | "deprecated") => {}
            Some("derive") => validate_derives(path, attribute, diagnostics),
            _ => diagnostics.push(module_diagnostic(
                path,
                attribute_span(attribute, false),
                "BXG0022",
                "non-allowlisted contract attribute",
                ATTRIBUTE_RULE,
            )),
        }
    }
}

fn validate_derives(
    path: &RelativePath,
    attribute: &syn::Attribute,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Ok(derives) = attribute.parse_args_with(
        syn::punctuated::Punctuated::<ParsedDerive, syn::Token![,]>::parse_terminated,
    ) else {
        diagnostics.push(module_diagnostic(
            path,
            attribute_span(attribute, false),
            "BXG0023",
            "non-allowlisted contract derive",
            DERIVE_RULE,
        ));
        return;
    };
    for derive in derives {
        let allowed = derive
            .path
            .get_ident()
            .map(syn::Ident::unraw)
            .is_some_and(|name| name == "Debug" || name == "Clone" || name == "PartialEq");
        if !allowed {
            diagnostics.push(module_diagnostic(
                path,
                derive.span,
                "BXG0023",
                "non-allowlisted contract derive",
                DERIVE_RULE,
            ));
        }
    }
}

struct ParsedDerive {
    path: syn::Path,
    span: proc_macro2::Span,
}

impl syn::parse::Parse for ParsedDerive {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let start = input.cursor();
        let mut path: syn::Path = input.parse()?;
        if input.peek(syn::token::Paren) {
            path.segments.last_mut().unwrap().arguments =
                syn::PathArguments::Parenthesized(input.parse()?);
        }
        let finish = input.cursor();
        let (first, mut cursor) = start.token_tree().expect("a parsed path has tokens");
        let mut last = first.span();
        while cursor != finish {
            let (token, next) = cursor
                .token_tree()
                .expect("finish follows parsed path tokens");
            last = token.span();
            cursor = next;
        }
        Ok(Self {
            path,
            span: first
                .span()
                .join(last)
                .expect("derive path spans one source"),
        })
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

    fn discovery_errors(request: &GenerationRequest) -> Diagnostics {
        let parsed = ParsedRustInputs::parse(request).unwrap();
        match parsed.discover_contract_declarations() {
            Ok(_) => panic!("expected contract declaration diagnostics"),
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
    fn direct_boxology_paths_are_exact_and_owned_by_the_first_attribute() {
        let expected = |line, end| {
            format!(
                "BXG0019 dead.rs:{line}:3-{line}:{end} offending={:?} rule={UNREACHABLE_RULE:?} source={RULE_SOURCE:?}",
                "Boxology-annotated item"
            )
        };
        let one = resolution_errors(&request(
            "root.rs",
            &[
                ("root.rs", ""),
                ("dead.rs", "#[boxology::contract] struct Dead;"),
            ],
        ));
        assert_eq!(one.to_string(), expected(1, 22));
        let source = "#[r#boxology::r#contract] struct Raw;\n#[::r#boxology::r#capability] fn leading() {}\n#[boxology::contract]\n#[::boxology::capability]\nfn twice() {}\n#[alias::contract] fn alias() {}\n#[crate::boxology::contract] fn prefixed() {}\n#[boxology] fn short() {}\n#[boxology::contract::nested] fn long() {}\n";
        let errors =
            resolution_errors(&request("root.rs", &[("root.rs", ""), ("dead.rs", source)]));
        let rendered = [(1, 26), (2, 30), (3, 22)]
            .map(|(line, end)| expected(line, end))
            .join("\n");
        assert_eq!(errors.to_string(), rendered);
    }

    #[test]
    fn conditional_export_item_spellings_have_exact_path_spans() {
        for (attribute, end, offending) in [
            ("#[cfg(payload)]", 6, "cfg attribute"),
            ("#[cfg_attr(payload, ignored)]", 11, "cfg_attr attribute"),
            ("#[r#cfg(payload)]", 8, "cfg attribute"),
            ("#[::r#cfg_attr(x, y)]", 15, "cfg_attr attribute"),
        ] {
            let source = format!("{attribute}\n#[boxology::contract]\nstruct Export;");
            let errors = resolution_errors(&request("root.rs", &[("root.rs", &source)]));
            assert_eq!(
                errors.to_string(),
                format!(
                    "BXG0020 root.rs:1:3-1:{end} offending={offending:?} rule={CONDITIONAL_RULE:?} source={RULE_SOURCE:?}"
                )
            );
        }
    }

    #[test]
    fn declaration_errors_are_complete_sorted_deduplicated_and_safe() {
        let request = request(
            "root.rs",
            &[
                ("a-dead.rs", "#[boxology::contract(payload)] struct Secret;"),
                (
                    "inline/child.rs",
                    "#[cfg(deep_payload)]\nmod deep {\n#[boxology::contract]\nstruct One;\n#[boxology::capability]\nfn two() {}\n}",
                ),
                (
                    "root.rs",
                    "#[boxology::contract]\nmod inline {\n#![cfg(root_payload)]\n#[cfg_attr(child_payload, ignored)]\nmod child;\n}\n#[boxology::contract]\nstruct S {\n#[cfg(field_payload)]\nfield: u8,\n}\n#[boxology::contract]\nenum E {\n#[cfg(variant_payload)]\nA,\nB {\n#[cfg_attr(field_payload, ignored)]\nfield: u8,\n}\n}\nstruct Plain;\nimpl Plain {\n#![cfg(impl_payload)]\n#[cfg_attr(method_payload, ignored)]\n#[boxology::capability]\nfn cap(&self) {}\n#[cfg(unrelated_payload)]\nfn helper(&self) {}\n}",
                ),
                (
                    "z-dead.rs",
                    "struct Z;\nimpl Z {\n#[boxology::contract]\n#[::boxology::capability]\nfn twice() {}\n}",
                ),
            ],
        );
        let (first, second) = (resolution_errors(&request), resolution_errors(&request));
        assert_eq!(first, second);
        let sites = [
            ("a-dead.rs", 1, 21),
            ("inline/child.rs", 1, 6),
            ("root.rs", 3, 7),
            ("root.rs", 4, 11),
            ("root.rs", 9, 6),
            ("root.rs", 14, 6),
            ("root.rs", 17, 11),
            ("root.rs", 23, 7),
            ("root.rs", 24, 11),
            ("z-dead.rs", 3, 22),
        ];
        let expected = sites.map(|(path, line, end)| {
            let start = if end == 7 { 4 } else { 3 };
            let (code, offending, rule) = match end {
                6 | 7 => ("BXG0020", "cfg attribute", CONDITIONAL_RULE),
                11 => ("BXG0020", "cfg_attr attribute", CONDITIONAL_RULE),
                _ => ("BXG0019", "Boxology-annotated item", UNREACHABLE_RULE),
            };
            format!("{code} {path}:{line}:{start}-{line}:{end} offending={offending:?} rule={rule:?} source={RULE_SOURCE:?}")
        }).join("\n");
        assert_eq!(first.to_string(), expected);
        let rendered = first.to_string();
        for payload in ["payload", "Secret", "One", "cap", "ignored"] {
            assert!(!rendered.contains(payload));
        }
    }

    #[test]
    fn conditionals_outside_export_shape_are_allowed() {
        let source = "#![cfg(file_payload)]\n#[cfg(internal_payload)] fn internal() {}\nstruct Plain { #[cfg(field_payload)] field: u8 }\nenum PlainEnum { #[cfg(variant_payload)] A, B { #[cfg_attr(field_payload, ignored)] field: u8 } }\n#[cfg(sibling_payload)] mod sibling {}\nimpl Plain { #[cfg(helper_payload)] fn helper(&self) {} #[boxology::capability] fn exported(&self) {} }";
        let valid = request("root.rs", &[("root.rs", source)]);
        let parsed = ParsedRustInputs::parse(&valid).unwrap();
        assert!(parsed.resolve_reachable_inputs().is_ok());
    }

    #[test]
    fn provisional_discovery_is_canonical_and_ignores_deferred_placements_and_non_contract_leaves()
    {
        let files = [
            (
                "src/root.rs",
                concat!(
                    "#[boxology::contract]\nstruct Foo;\nmod alpha;\nmod r#inline {\n",
                    "#[boxology::contract(error)]\nenum Fault { HiddenVariant }\n",
                    "#[boxology::contract]\nstruct r#foo;\nstruct Plain;\n",
                    "fn body() { #[boxology::contract] struct Local; }\n",
                    "#[boxology::contract] fn misplaced() {}\n",
                    "#[boxology::contract] type Alias = u8;\n",
                    "#[boxology::contract] union Deferred { value: u8 }\n",
                    "macro_rules! hidden { () => { #[boxology::contract] struct Macro; } }\n",
                    "struct Host;\nimpl Host { #[boxology::capability] fn cap(&self) {} }\n}\n",
                ),
            ),
            (
                "src/alpha.rs",
                "#[boxology::contract]\nenum Ordinary { Hidden }\nmod deep;\n",
            ),
            (
                "src/alpha/deep/mod.rs",
                "#[boxology::contract]\nstruct Deep;\n",
            ),
        ];
        let project = |files: &[(&str, &str)]| {
            let request = request("src/root.rs", files);
            let parsed = ParsedRustInputs::parse(&request).unwrap();
            parsed
                .discover_contract_declarations()
                .unwrap()
                .into_iter()
                .map(|declaration| {
                    let (kind, syntax_name) = match declaration.syntax() {
                        ContractDeclarationSyntax::Struct(item) => ("struct", item.ident.unraw()),
                        ContractDeclarationSyntax::Enum(item) => ("enum", item.ident.unraw()),
                    };
                    assert_eq!(syntax_name, declaration.lifted_name());
                    let span = declaration.identifier_span();
                    format!(
                        "{kind}|[{}]|{}|{}|{}:{}-{}:{}",
                        declaration.module_path().join("::"),
                        declaration.lifted_name(),
                        declaration.source().as_str(),
                        span.start().line(),
                        span.start().column(),
                        span.end().line(),
                        span.end().column(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let first = project(&files);
        let expected = [
            "struct|[]|Foo|src/root.rs|2:8-2:11",
            "enum|[alpha]|Ordinary|src/alpha.rs|2:6-2:14",
            "struct|[alpha::deep]|Deep|src/alpha/deep/mod.rs|2:8-2:12",
            "enum|[inline]|Fault|src/root.rs|6:6-6:11",
            "struct|[inline]|foo|src/root.rs|8:8-8:13",
        ];
        assert_eq!(first, expected);
        assert_eq!(first, project(&files.into_iter().rev().collect::<Vec<_>>()));
    }

    #[test]
    fn raw_struct_and_enum_collisions_are_complete_repeatable_exact_and_payload_safe() {
        let files = [
            (
                "root.rs",
                "#[boxology::contract(root_payload)]\nstruct Foo { winner_field: u8 }\nmod a;\nmod z;\n",
            ),
            (
                "a.rs",
                "#[boxology::contract(loser_payload)]\nstruct r#Foo { loser_field: u8 }\n",
            ),
            (
                "z.rs",
                "#[boxology::contract(error)]\nenum Foo { SecretVariant }\n",
            ),
        ];
        let first = discovery_errors(&request("root.rs", &files));
        let reversed = files.into_iter().rev().collect::<Vec<_>>();
        assert_eq!(first, discovery_errors(&request("root.rs", &reversed)));
        assert_eq!(first.as_slice().len(), 2);
        for (diagnostic, (path, expected_span)) in first.as_slice().iter().zip([
            ("a.rs", span((2, 8), (2, 13))),
            ("z.rs", span((2, 6), (2, 9))),
        ]) {
            assert_eq!(
                (diagnostic.code(), diagnostic.path().as_str()),
                ("BXG0021", path)
            );
            assert_eq!(diagnostic.span(), expected_span);
            assert_eq!(
                diagnostic.offending_construct(),
                "colliding lifted contract type name"
            );
            assert_eq!(diagnostic.rule(), COLLISION_RULE);
            assert_eq!(diagnostic.rule_source(), COLLISION_RULE_SOURCE);
        }
        let rendered = first.to_string();
        for sentinel in ["Foo", "payload", "field", "SecretVariant", "root.rs"] {
            assert!(!rendered.contains(sentinel));
        }
    }

    #[test]
    fn contract_attribute_allowlist_accepts_all_supported_sites_and_forms() {
        let source = concat!(
            "/// declaration docs\n#[doc = \"more docs\"]\n#[deprecated]\n",
            "#[deprecated(note = \"later\")]\n#[derive(Debug, r#Clone)]\n",
            "#[derive(PartialEq)]\n#[boxology::contract(payload)]\n",
            "struct Named { #[doc = \"field\"] #[derive(Clone)] named: u8 }\n",
            "#[::boxology::contract]\n",
            "struct Tuple(#[::boxology::field(anything)] #[deprecated(note = \"later\")] u8);\n",
            "#[derive()]\n#[boxology::contract(error)]\nenum Event {\n",
            "#[doc = \"variant\"] Unit,\n",
            "Tuple(#[derive(Debug, PartialEq)] u8),\n",
            "Named { #[boxology::field] value: u8 },\n}\n",
        );
        let parsed = ParsedRustInputs::parse(&request("root.rs", &[("root.rs", source)])).unwrap();
        let declarations = parsed.discover_contract_declarations().unwrap();
        assert_eq!(
            declarations
                .iter()
                .map(ContractDeclaration::lifted_name)
                .collect::<Vec<_>>(),
            ["Named", "Tuple", "Event"]
        );
    }

    #[test]
    fn rejected_attributes_are_owned_by_each_direct_contract_site() {
        let source = concat!(
            "#[PrivateDeclAttr(secret)]\n#[boxology::contract]\nstruct S {\n",
            "#[PrivateFieldAttr(secret)]\nvalue: u8,\n}\n",
            "#[boxology::contract]\nenum E {\n",
            "#[PrivateVariantAttr(secret)]\nA,\nB {\n",
            "#[::PrivateVariantFieldAttr(secret)]\nvalue: u8,\n}\n}\n",
        );
        let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
        let expected = [
            span((1, 3), (1, 18)),
            span((4, 3), (4, 19)),
            span((9, 3), (9, 21)),
            span((12, 3), (12, 28)),
        ];
        assert_eq!(diagnostics.as_slice().len(), expected.len());
        for (diagnostic, expected) in diagnostics.as_slice().iter().zip(expected) {
            assert_eq!(diagnostic.code(), "BXG0022");
            assert_eq!(diagnostic.span(), expected);
            assert_eq!(
                diagnostic.offending_construct(),
                "non-allowlisted contract attribute"
            );
            assert_eq!(diagnostic.rule(), ATTRIBUTE_RULE);
            assert_eq!(diagnostic.rule_source(), RULE_SOURCE);
        }
        let output = format!("{diagnostics}\n{diagnostics:?}");
        for private in [
            "PrivateDeclAttr",
            "PrivateFieldAttr",
            "PrivateVariantAttr",
            "PrivateVariantFieldAttr",
            "secret",
        ] {
            assert!(!output.contains(private));
        }
    }

    #[test]
    fn derive_allowlist_is_exact_complete_and_payload_safe() {
        let source = concat!(
            "#[boxology::contract]\n#[derive(Debug, r#Clone, PartialEq)]\n#[derive()]\n",
            "#[derive(Debug, serde::Serialize, Clone)]\n#[derive(::PartialEq)]\n",
            "#[derive(Debug<u8>)]\n#[derive(Debug(u8))]\n#[derive(Fn() -> SecretReturn)]\n",
            "#[derive(Copy)]\n#[derive]\n#[derive = \"PrivateValue\"]\nstruct Bad;\n",
        );
        let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
        let expected = [
            span((4, 17), (4, 33)),
            span((5, 10), (5, 21)),
            span((6, 10), (6, 19)),
            span((7, 10), (7, 19)),
            span((8, 10), (8, 30)),
            span((9, 10), (9, 14)),
            span((10, 3), (10, 9)),
            span((11, 3), (11, 9)),
        ];
        assert_eq!(diagnostics.as_slice().len(), expected.len());
        for (diagnostic, expected) in diagnostics.as_slice().iter().zip(expected) {
            assert_eq!(diagnostic.code(), "BXG0023");
            assert_eq!(diagnostic.span(), expected);
            assert_eq!(
                diagnostic.offending_construct(),
                "non-allowlisted contract derive"
            );
            assert_eq!(diagnostic.rule(), DERIVE_RULE);
        }
        let output = format!("{diagnostics}\n{diagnostics:?}");
        for private in [
            "serde",
            "Serialize",
            "SecretReturn",
            "Copy",
            "PrivateValue",
            "u8",
        ] {
            assert!(!output.contains(private));
        }
    }

    #[test]
    fn earlier_phases_suppress_allowlist_and_findings_are_input_order_invariant() {
        let collision = discovery_errors(&request(
            "root.rs",
            &[(
                "root.rs",
                "#[boxology::contract]\n#[PrivateOne]\nstruct Foo;\n#[boxology::contract]\n#[PrivateTwo]\nenum Foo { A }",
            )],
        ));
        assert_eq!(collision.as_slice().len(), 1);
        assert_eq!(collision.as_slice()[0].code(), "BXG0021");
        for (source, code) in [
            (
                "#[boxology::contract]\n#[Private]\nstruct Unique;\nmod missing;",
                "BXG0017",
            ),
            (
                "#[cfg(secret)]\n#[boxology::contract]\n#[Private]\nstruct Unique;",
                "BXG0020",
            ),
        ] {
            let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
            assert_eq!(diagnostics.as_slice()[0].code(), code);
            assert!(
                diagnostics
                    .as_slice()
                    .iter()
                    .all(|diagnostic| !matches!(diagnostic.code(), "BXG0022" | "BXG0023"))
            );
        }

        let files = [
            ("root.rs", "mod z; mod a;"),
            ("z.rs", "#[boxology::contract]\n#[Private]\nstruct Z;"),
            ("a.rs", "#[boxology::contract]\n#[derive(Copy)]\nstruct A;"),
        ];
        let first = discovery_errors(&request("root.rs", &files));
        let reversed = files.into_iter().rev().collect::<Vec<_>>();
        assert_eq!(first, discovery_errors(&request("root.rs", &reversed)));
        assert_eq!(
            first
                .as_slice()
                .iter()
                .map(|diagnostic| (diagnostic.path().as_str(), diagnostic.code()))
                .collect::<Vec<_>>(),
            [("a.rs", "BXG0023"), ("z.rs", "BXG0022")]
        );
    }

    #[test]
    fn earlier_structural_and_conditional_phases_suppress_collisions() {
        for (source, code) in [
            (
                "#[boxology::contract] struct Foo;\n#[boxology::contract] enum Foo { A }\nmod missing;",
                "BXG0017",
            ),
            (
                "#[cfg(secret)]\n#[boxology::contract] struct Foo;\n#[boxology::contract] enum Foo { A }",
                "BXG0020",
            ),
        ] {
            let diagnostics = discovery_errors(&request("root.rs", &[("root.rs", source)]));
            assert_eq!(diagnostics.as_slice().len(), 1);
            assert_eq!(diagnostics.as_slice()[0].code(), code);
            assert!(!diagnostics.to_string().contains("BXG0021"));
        }
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
                ("dead.rs", "#[boxology::contract] struct Dead;\n"),
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
            "root.rs",
            &[
                ("b.rs", "fn café() { @ }\n"),
                ("a.rs", "fn good() {}\nfn bad() { @ }\n"),
                ("root.rs", "fn root() {}\n"),
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
