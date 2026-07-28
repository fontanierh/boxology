use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use syn::parse::Parser;
use syn::visit::Visit;
const EXPECTED: &str = "BXW0042 BXW0043 BXW0044 BXW0045 BXW0046 BXW0047 BXW0048 BXW0049 BXW0050 BXW0051 BXW0052 BXW0053 BXW0054 BXW0055 BXW0056 BXW0057 BXW0058 BXW0059 BXW0060";
const DANGEROUS: &str = "mod include include_str concat stringify cfg cfg_attr test";
const MACROS: &str =
    "macro_rules ref_getters assert assert_eq assert_ne format matches panic vec write";
const DERIVES: &str = "Clone Copy Debug Eq Ord PartialEq PartialOrd";
const RULES: &str = "ESCAPE DUPLICATE SELF_CLAIM UNOWNED OVERLAP RIVALS BOTH LOCK DOCUMENT UNMAPPED UNMATCHED CLAIMED ROLE";
const EDGE_RULES: &str = "CONTRACT FOREIGN DECLARED SELECTED IMPOSSIBLE NON_MEMBER";
const RELATIVE: &str = "pub fn relative(&self, path: &RelativePath) -> Option<RelativePath>";
const PROTECTED: &str =
    "Rule EdgeRule derive ref_getters assert assert_eq assert_ne format matches panic vec write";
const RETAINED_EDGE_ASSERTION: &str = "assert_eq!(\n            source.edges(),\n            &[DeclaredEdge {\n                kind: EdgeKind::Normal,\n                target: EdgeTarget::InRoot(path(target_at)),\n            }]\n        )";
const RETAINED_EDGE_HELPER_BODY: &str = "        let ids: Vec<&str> = checked\n            .packages()\n            .iter()\n            .map(|package| package.id().as_str())\n            .collect();\n        assert_eq!(ids, packages);\n        let source = checked\n            .cargo_members()\n            .iter()\n            .find(|member| member.cargo_package() == source)\n            .unwrap();\n        let target = checked\n            .cargo_members()\n            .iter()\n            .find(|member| member.cargo_package() == target)\n            .unwrap();\n        assert_eq!(source.manifest_path(), &path(source_at));\n        assert_eq!(target.crate_dir(), Some(&path(target_at)));\n        assert_eq!(\n            source.edges(),\n            &[DeclaredEdge {\n                kind: EdgeKind::Normal,\n                target: EdgeTarget::InRoot(path(target_at)),\n            }]\n        );\n";
struct Source {
    text: String,
    lib: bool,
}
fn metadata() -> Value {
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid cargo metadata")
}
fn targets(document: &Value) -> Option<Vec<(PathBuf, bool)>> {
    let package = document["packages"]
        .as_array()?
        .iter()
        .find(|package| package["name"].as_str() == Some(env!("CARGO_PKG_NAME")))?;
    let targets = package["targets"]
        .as_array()?
        .iter()
        .filter_map(|target| {
            let kinds = target["kind"].as_array()?;
            let lib = kinds.iter().any(|kind| kind.as_str() == Some("lib"));
            (lib || kinds
                .iter()
                .any(|kind| matches!(kind.as_str(), Some("bin" | "example"))))
            .then(|| {
                target["src_path"]
                    .as_str()
                    .map(|path| (PathBuf::from(path), lib))
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (!targets.is_empty()).then_some(targets)
}
fn cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        matches!(&attr.meta, syn::Meta::List(meta)
            if meta.path.is_ident("cfg") && meta.tokens.to_string() == "test")
    })
}
fn module_candidates(item: &syn::ItemMod, base: &Path) -> Option<Vec<PathBuf>> {
    let paths = item
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("path"))
        .map(|attr| {
            let syn::Meta::NameValue(value) = &attr.meta else {
                return None;
            };
            let syn::Expr::Lit(lit) = &value.value else {
                return None;
            };
            let syn::Lit::Str(path) = &lit.lit else {
                return None;
            };
            Some(PathBuf::from(path.value()))
        })
        .collect::<Option<Vec<_>>>()?;
    match paths.as_slice() {
        [path] => Some(vec![base.join(path)]),
        [] => {
            let stem = base.join(item.ident.to_string());
            Some(vec![stem.with_extension("rs"), stem.join("mod.rs")])
        }
        _ => None,
    }
}
struct ModuleFinder {
    base: PathBuf,
    refs: Vec<Vec<PathBuf>>,
    blocks: usize,
    bad: bool,
}
impl<'ast> Visit<'ast> for ModuleFinder {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if self.blocks != 0 {
            self.bad = true;
        } else if !cfg_test(&item.attrs) {
            if let Some((_, body)) = &item.content {
                let child = self.base.join(item.ident.to_string());
                let previous = std::mem::replace(&mut self.base, child);
                body.iter().for_each(|item| self.visit_item(item));
                self.base = previous;
            } else if let Some(candidates) = module_candidates(item, &self.base) {
                self.refs.push(candidates);
            } else {
                self.bad = true;
            }
        }
    }
    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.blocks += 1;
        syn::visit::visit_block(self, block);
        self.blocks -= 1;
    }
}
fn module_refs(file: &syn::File, base: &Path) -> Option<Vec<Vec<PathBuf>>> {
    let mut finder = ModuleFinder {
        base: base.to_path_buf(),
        refs: Vec::new(),
        blocks: 0,
        bad: false,
    };
    file.items.iter().for_each(|item| finder.visit_item(item));
    (!finder.bad).then_some(finder.refs)
}
fn module_dir(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?.to_path_buf();
    if path.file_name().is_some_and(|name| name == "mod.rs") {
        Some(parent)
    } else {
        Some(parent.join(path.file_stem()?))
    }
}
fn collect_source(
    path: &Path,
    lib: bool,
    root: bool,
    sources: &mut BTreeMap<PathBuf, Source>,
    read: &impl Fn(&Path) -> Option<String>,
) -> Option<()> {
    if let Some(source) = sources.get_mut(path) {
        source.lib |= lib;
        return Some(());
    }
    let text = read(path)?;
    let file = syn::parse_file(&text).ok()?;
    sources.insert(path.to_path_buf(), Source { text, lib });
    let base = if root {
        path.parent()?.to_path_buf()
    } else {
        module_dir(path)?
    };
    for candidates in module_refs(&file, &base)? {
        let present: Vec<_> = candidates
            .iter()
            .filter(|candidate| read(candidate).is_some())
            .collect();
        if present.len() != 1 {
            return None;
        }
        collect_source(present[0], false, false, sources, read)?;
    }
    Some(())
}
fn source_inventory(
    document: &Value,
    read: &impl Fn(&Path) -> Option<String>,
) -> Option<BTreeMap<PathBuf, Source>> {
    let mut sources = BTreeMap::new();
    for (path, lib) in targets(document)? {
        collect_source(&path, lib, true, &mut sources, read)?;
    }
    Some(sources)
}
#[derive(Default)]
struct Lock {
    codes: Vec<String>,
    collect: bool,
    module_attrs: bool,
    nested: bool,
    bad: bool,
    repr: usize,
    exit_repr: usize,
    all: usize,
    check_step: bool,
}
impl Lock {
    fn repr(attr: &syn::Attribute) -> bool {
        matches!(&attr.meta, syn::Meta::List(meta)
            if meta.path.is_ident("repr") && meta.tokens.to_string() == "u8")
    }
    fn all(item: &syn::ImplItemConst) -> bool {
        let syn::Type::Array(ty) = &item.ty else {
            return false;
        };
        let syn::Expr::Array(value) = &item.expr else {
            return false;
        };
        let names =
            "Discovery Regeneration ContractClassification CargoGraph Fmt Clippy Tests Quality";
        let length = matches!(&ty.len, syn::Expr::Lit(lit)
            if matches!(&lit.lit, syn::Lit::Int(value)
                if value.suffix().is_empty() && value.base10_digits() == "8"));
        matches!(item.vis, syn::Visibility::Public(_))
            && item.ident == "ALL"
            && matches!(ty.elem.as_ref(), syn::Type::Path(path)
                if path.qself.is_none() && path.path.is_ident("Self"))
            && length
            && value.elems.len() == 8
            && value
                .elems
                .iter()
                .zip(names.split(' '))
                .all(|(expr, name)| {
                    matches!(expr, syn::Expr::Path(path)
                    if path.qself.is_none() && path.path.leading_colon.is_none()
                        && path.path.segments.len() == 2
                        && path.path.segments.first().is_some_and(|part| part.ident == "Self")
                        && path.path.segments.last().is_some_and(|part| part.ident == name))
                })
    }
    fn tokens(&mut self, text: String) {
        self.bad |= text.contains("BXW")
            || text.contains('\\')
            || text
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| DANGEROUS.split(' ').any(|item| item == word));
    }
    fn direct_rule(item: &syn::ItemConst, edge: bool) -> bool {
        let syn::Expr::Tuple(outer) = item.expr.as_ref() else {
            return false;
        };
        let (code, text, source) =
            if let (true, Some(syn::Expr::Tuple(rule))) = (edge, outer.elems.first()) {
                (rule.elems.first(), rule.elems.get(1), outer.elems.get(1))
            } else if edge {
                return false;
            } else {
                (outer.elems.first(), outer.elems.get(1), None)
            };
        let text_name = format!("{}_TEXT", item.ident);
        let path = |expr: Option<&syn::Expr>, name: &str| matches!(expr, Some(syn::Expr::Path(value)) if value.path.is_ident(name));
        outer.elems.len() == 2
            && matches!(code, Some(syn::Expr::Lit(lit))
                if matches!(&lit.lit, syn::Lit::Str(code) if code.value().starts_with("BXW")))
            && path(text, text_name.as_str())
            && (!edge
                || matches!(outer.elems.first(), Some(syn::Expr::Tuple(rule)) if rule.elems.len() == 2)
                    && (path(source, "EDGE_SOURCE") || path(source, "D4_SOURCE")))
    }
    fn protected(ident: &syn::Ident) -> bool {
        PROTECTED.split(' ').any(|name| ident == name)
    }
    fn builtin_derive(path: &syn::Path) -> bool {
        path.leading_colon.is_none()
            && path.segments.len() == 1
            && path.segments.first().is_some_and(|segment| {
                matches!(&segment.arguments, syn::PathArguments::None)
                    && DERIVES.split(' ').any(|name| segment.ident == name)
            })
    }
}
impl<'ast> Visit<'ast> for Lock {
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        let allowed = if self.module_attrs {
            "doc deny forbid derive path cfg"
        } else if self.collect {
            "doc deny forbid derive"
        } else {
            "doc deny forbid derive test"
        };
        let repr = self.collect && Self::repr(attr);
        self.repr += usize::from(repr);
        self.bad |= !repr && !allowed.split(' ').any(|name| attr.path().is_ident(name));
        if self.collect && attr.path().is_ident("derive") {
            let valid = attr.meta.require_list().ok().and_then(|meta| {
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated
                    .parse2(meta.tokens.clone())
                    .ok()
            });
            self.bad |= !valid
                .as_ref()
                .is_some_and(|paths| !paths.is_empty() && paths.iter().all(Self::builtin_derive));
            self.tokens(
                attr.meta
                    .require_list()
                    .map_or_else(|_| String::new(), |meta| meta.tokens.to_string()),
            );
        }
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if self.collect {
            let syn::Type::Path(ty) = item.ty.as_ref() else {
                syn::visit::visit_item_const(self, item);
                return;
            };
            let kind = ty.path.segments.last().map(|part| part.ident.to_string());
            self.bad |= match kind.as_deref() {
                Some("Rule") => {
                    self.nested
                        || !ty.path.is_ident("Rule")
                        || !RULES.split(' ').any(|name| item.ident == name)
                        || !Self::direct_rule(item, false)
                }
                Some("EdgeRule") => {
                    self.nested
                        || !ty.path.is_ident("EdgeRule")
                        || !EDGE_RULES.split(' ').any(|name| item.ident == name)
                        || !Self::direct_rule(item, true)
                }
                _ => false,
            };
        }
        syn::visit::visit_item_const(self, item);
    }
    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        self.bad |= self.collect && item.ident != "Rule" && item.ident != "EdgeRule";
        syn::visit::visit_item_type(self, item);
    }
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        if self.collect && item.ident == "ExitCode" {
            self.exit_repr += item.attrs.iter().filter(|attr| Self::repr(attr)).count();
        }
        syn::visit::visit_item_enum(self, item);
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let check_step = item.trait_.is_none()
            && matches!(item.self_ty.as_ref(), syn::Type::Path(path)
                if path.qself.is_none() && path.path.is_ident("CheckStep"));
        let previous = std::mem::replace(&mut self.check_step, check_step);
        syn::visit::visit_item_impl(self, item);
        self.check_step = previous;
    }
    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        if self.collect {
            let all = self.check_step && Self::all(item);
            self.all += usize::from(all);
            self.bad |= !all;
        }
        syn::visit::visit_impl_item_const(self, item);
    }
    fn visit_trait_item_const(&mut self, item: &'ast syn::TraitItemConst) {
        self.bad |= self.collect;
        syn::visit::visit_trait_item_const(self, item);
    }
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let nested = std::mem::replace(&mut self.nested, true);
        syn::visit::visit_block(self, block);
        self.nested = nested;
    }
    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        self.bad |= !item.mac.path.is_ident("macro_rules")
            || item
                .ident
                .as_ref()
                .is_none_or(|ident| ident != "ref_getters");
        self.visit_macro(&item.mac);
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if self.nested
            || (cfg_test(&item.attrs) && !(item.ident == "tests" && item.attrs.len() == 1))
        {
            self.bad = true;
            return;
        }
        let previous = std::mem::replace(&mut self.module_attrs, true);
        item.attrs
            .iter()
            .for_each(|attr| self.visit_attribute(attr));
        self.module_attrs = previous;
        if let Some((_, body)) = &item.content {
            body.iter().for_each(|item| self.visit_item(item));
        }
    }
    fn visit_use_glob(&mut self, _: &'ast syn::UseGlob) {
        self.bad |= self.collect;
    }
    fn visit_use_name(&mut self, item: &'ast syn::UseName) {
        self.bad |= self.collect && Self::protected(&item.ident);
    }
    fn visit_use_rename(&mut self, item: &'ast syn::UseRename) {
        self.bad |= self.collect && (Self::protected(&item.ident) || Self::protected(&item.rename));
    }
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if self.collect && literal.value().starts_with("BXW") {
            self.codes.push(literal.value());
        }
    }
    fn visit_lit_byte_str(&mut self, _: &'ast syn::LitByteStr) {
        self.bad |= self.collect;
    }
    fn visit_expr_call(&mut self, called: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = called.func.as_ref() {
            self.bad |= self.collect
                && function
                    .path
                    .segments
                    .last()
                    .is_some_and(|part| part.ident == "from_utf8" || part.ident == "leak");
        }
        syn::visit::visit_expr_call(self, called);
    }
    fn visit_expr_method_call(&mut self, called: &'ast syn::ExprMethodCall) {
        let array_join = matches!(called.receiver.as_ref(), syn::Expr::Array(_))
            && (called.method == "concat" || called.method == "join");
        self.bad |= self.collect && (array_join || called.method == "into_boxed_str");
        syn::visit::visit_expr_method_call(self, called);
    }
    fn visit_macro(&mut self, called: &'ast syn::Macro) {
        let allowed = MACROS.split(' ').any(|name| called.path.is_ident(name))
            || (!self.collect && called.path.is_ident("include_str"));
        self.bad |= !allowed;
        if self.collect {
            self.tokens(called.tokens.to_string());
        }
    }
}
fn locked(source: &str) -> bool {
    locked_sources([&Source {
        text: source.to_owned(),
        lib: true,
    }])
}
fn test_module(item: &syn::Item) -> Option<&syn::ItemMod> {
    let syn::Item::Mod(module) = item else {
        return None;
    };
    (module.ident == "tests" && module.attrs.len() == 1 && cfg_test(&module.attrs))
        .then_some(module)
}
fn scan_source(source: &Source, lock: &mut Lock) -> bool {
    let Ok(file) = syn::parse_file(&source.text) else {
        return false;
    };
    lock.collect = true;
    file.attrs
        .iter()
        .for_each(|attr| lock.visit_attribute(attr));
    let mut test_modules = 0;
    for item in &file.items {
        if let Some(module) = test_module(item) {
            test_modules += 1;
            let Some((_, body)) = &module.content else {
                lock.bad = true;
                continue;
            };
            lock.collect = false;
            body.iter().for_each(|item| lock.visit_item(item));
        } else {
            lock.collect = true;
            lock.visit_item(item);
        }
    }
    if source.lib && test_modules != 1 {
        lock.bad = true;
    }
    true
}
fn locked_sources<'a>(sources: impl IntoIterator<Item = &'a Source>) -> bool {
    let mut lock = Lock::default();
    let mut lib = None;
    for source in sources {
        if source.lib && lib.replace(source).is_some() {
            return false;
        }
        if !scan_source(source, &mut lock) {
            return false;
        }
    }
    let Some(lib) = lib else { return false };
    lock.codes.sort_unstable();
    let edge = r#"successful_edge(workspace, packages, "s", "a/s/Cargo.toml", "t", at)"#;
    !lock.bad
        && lock.repr == 1
        && lock.exit_repr == 1
        && lock.all == 1
        && lock.codes.join(" ") == EXPECTED
        && lib.text.match_indices(edge).count() == 1
        && lib.text.match_indices(RELATIVE).count() == 1
        && lib.text.match_indices(RETAINED_EDGE_ASSERTION).count() == 1
}
fn locked_document(document: &Value, files: &BTreeMap<PathBuf, String>) -> bool {
    source_inventory(document, &|path| files.get(path).cloned())
        .is_some_and(|sources| locked_sources(sources.values()))
}
#[rustfmt::skip]
fn once(source: &str, anchor: &str, replacement: &str) -> String {
    assert_eq!(source.match_indices(anchor).count(), 1, "anchor: {anchor:?}");
    let changed = source.replacen(anchor, replacement, 1);
    assert_ne!(changed, source);
    changed
}
#[rustfmt::skip]
fn rejects(name: &str, source: &str, anchor: &str, replacement: &str) {
    assert!(!locked(&once(source, anchor, replacement)), "mutation survived: {name}");
}
#[rustfmt::skip]
#[test]
fn surface_and_live_evasions_are_locked() {
    let mut document = metadata();
    let sources = source_inventory(&document, &|path| fs::read_to_string(path).ok())
        .expect("production Rust source inventory");
    let source = &sources.values().find(|source| source.lib).unwrap().text;
    assert!(locked_sources(sources.values()));
    let header = "#[cfg(test)]\nmod tests {";
    let early = "#[cfg(test)] mod tests {}\nconst X: &str = \"BXW9999\";\n#[cfg(test)] mod tests {";
    let inject = |name, text: &str| rejects(name, source, header, &format!("{text}{header}"));
    rejects("early boundary", source, header, early);
    rejects("crate self-disable", source, source, &format!("#![cfg(not(test))]\n{source}"));
    rejects(
        "boundary self-disable",
        source,
        header,
        "#[cfg(any())]\n#[cfg(test)]\nmod tests {",
    );
    let mutations = [
        ("include", "const HIDDEN: &str = include_str!(\"hidden.rs\");\n"),
        ("renamed macro", "use hidden_macros::emit_rule as format;\nconst HIDDEN: EdgeRule = format!();\n"),
        ("ordinary macro import", "use hidden_macros::{format};\nconst HIDDEN: EdgeRule = format!();\n"),
        ("recursive macro import", "use hidden_macros::{nested::{format}};\nconst HIDDEN: EdgeRule = format!();\n"),
        ("glob macro import", "use hidden_macros::*;\nconst HIDDEN: EdgeRule = format!();\n"),
        ("qualified macro", "const X: &str = std::format!(\"BXW9999\");\n"),
        ("concat macro", "const X: &str = std::concat!(\"B\", \"XW9999\");\n"),
        ("stringify macro", "const X: &str = stringify!(BXW9999);\n"),
        ("split concat", "const X: String = [\"B\", \"XW9999\"].concat();\n"),
        ("byte code", "const X: &[u8] = b\"BXW9999\";\n"),
        ("constructed rule", "const CODE: &str = match core::str::from_utf8(b\"BXW9999\") { Ok(value) => value, Err(_) => \"\" };\nconst HIDDEN: Rule = (CODE, \"hidden\");\n"),
        ("leaked constructed code", "fn hidden() -> &'static str { Box::leak([\"B\", \"XW9999\"].join(\"\").into_boxed_str()) }\n"),
        ("qualified rule", "const HIDDEN: crate::Rule = (ROLE.0, \"different text\");\n"),
        ("aliased edge rule", "use crate::EdgeRule as HiddenRule;\nconst HIDDEN: HiddenRule = DECLARED;\n"),
        ("associated rule", "struct Hidden;\nimpl Hidden { const RULE: crate::Rule = (ROLE.0, \"different text\"); }\n"),
        ("repr escape", "#[repr(C)] pub struct Escape(u8);\n"),
        ("associated ALL escape", "pub struct Escape;\nimpl Escape { pub const ALL: u8 = 0; }\n"),
        ("qualified derive", "#[hidden::derive(BXW9999)] struct Hidden;\n"),
        ("aliased derive", "use hidden::derive;\n#[derive(BXW9999)] struct Hidden;\n"),
        ("proc derive", "#[derive(hidden::Inject)] struct Hidden;\n"),
        ("test attribute", "#[test]\nfn hidden() {}\n"),
        ("cfg attribute", "#[cfg(test)]\nconst HIDDEN: Rule = (\"BXW9999\", \"hidden\");\n"),
        ("nested module", "fn hidden() { #[path = \"hidden.rs\"] mod nested; }\n"),
        ("duplicate registration", "const HIDDEN: Rule = (\"BXW0055\", \"hidden\");\n"),
        ("stale registration", "const HIDDEN: Rule = (\"BXW9999\", \"hidden\");\n"),
    ];
    for (name, mutation) in mutations {
        inject(name, mutation);
    }
    for (name, anchor, replacement) in [
        ("retained edge assertion", r#"successful_edge(workspace, packages, "s", "a/s/Cargo.toml", "t", at)"#, "assert_eq!(workspace.edges(), workspace.edges())"),
        ("retained edge helper no-op", RETAINED_EDGE_HELPER_BODY, ""),
        ("retained edge assertion body", RETAINED_EDGE_ASSERTION, "{}"),
    ] {
        rejects(name, source, anchor, replacement);
    }
    rejects("public relative seam", source, RELATIVE, &RELATIVE.replacen("pub ", "", 1));
    rejects("post-test registration", source, source, &format!("{source}\nconst HIDDEN: Rule = (\"BXW9999\", \"hidden\");\n"));
    for (name, anchor, replacement) in [
        ("macro registration", "($(#[$meta:meta] $name:ident: $return:ty = $field:tt;)*) => {$(", "($(#[$meta:meta] $name:ident: $return:ty = $field:tt;)*) => {$(const X: Rule = (\"BXW9999\", \"x\");"),
        ("macro self-disable", "#[$meta] pub fn $name(&self) -> $return { &self.$field }", "#[cfg(test)] #[$meta] pub fn $name(&self) -> $return { &self.$field }"),
    ] {
        rejects(name, source, anchor, replacement);
    }
    let mut files: BTreeMap<_, _> = sources.iter().map(|(path, source)| (path.clone(), source.text.clone())).collect();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/surface_escape.rs");
    files.insert(path.clone(), "fn main() { let _ = \"BXW9999\"; }\n".into());
    let package = document["packages"].as_array_mut().unwrap().iter_mut().find(|package| {
        package["name"].as_str() == Some(env!("CARGO_PKG_NAME"))
    }).unwrap();
    let targets = package["targets"].as_array_mut().unwrap();
    targets.push(serde_json::json!({
        "kind": ["bin"],
        "crate_types": ["bin"],
        "name": "surface_escape",
        "src_path": path.to_string_lossy().to_string(),
    }));
    assert_eq!(targets.iter().filter(|target| target["name"].as_str() == Some("surface_escape")).count(), 1);
    assert!(!locked_document(&document, &files));
}
