use syn::visit::Visit;
const SOURCE: &str = include_str!("../src/lib.rs");
const EXPECTED: &str = "BXW0042 BXW0043 BXW0044 BXW0045 BXW0046 BXW0047 BXW0048 BXW0049 BXW0050 BXW0051 BXW0052 BXW0053 BXW0054 BXW0055 BXW0056 BXW0057 BXW0058 BXW0059 BXW0060";
const DANGEROUS: &str = "mod include include_str concat stringify cfg cfg_attr test";
const MACROS: &str =
    "macro_rules ref_getters assert assert_eq assert_ne format matches panic vec write";
const RULES: &str = "ESCAPE DUPLICATE SELF_CLAIM UNOWNED OVERLAP RIVALS BOTH LOCK DOCUMENT UNMAPPED UNMATCHED CLAIMED ROLE";
const EDGE_RULES: &str = "CONTRACT FOREIGN DECLARED SELECTED IMPOSSIBLE NON_MEMBER";
const PROTECTED: &str =
    "Rule EdgeRule derive ref_getters assert assert_eq assert_ne format matches panic vec write";
#[derive(Default)]
struct Lock {
    codes: Vec<String>,
    collect: bool,
    nested: bool,
    bad: bool,
}
impl Lock {
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
}
impl<'ast> Visit<'ast> for Lock {
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        let allowed = if self.collect {
            "doc deny forbid derive"
        } else {
            "doc deny forbid derive test"
        };
        self.bad |= !allowed.split(' ').any(|name| attr.path().is_ident(name));
        if self.collect && attr.path().is_ident("derive") {
            let tokens = attr.meta.require_list().map(|meta| meta.tokens.to_string());
            self.bad |= tokens.is_err();
            self.tokens(tokens.unwrap_or_default());
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
    fn visit_impl_item_const(&mut self, item: &'ast syn::ImplItemConst) {
        self.bad |= self.collect;
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
        self.bad = true;
        syn::visit::visit_item_mod(self, item);
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
    let Ok(file) = syn::parse_file(source) else {
        return false;
    };
    let Some((syn::Item::Mod(tests), production)) = file.items.split_last() else {
        return false;
    };
    let cfg = |attr: &syn::Attribute| {
        matches!(&attr.meta, syn::Meta::List(meta)
            if meta.path.is_ident("cfg") && meta.tokens.to_string() == "test")
    };
    let Some((_, body)) = &tests.content else {
        return false;
    };
    if tests.ident != "tests" || tests.attrs.len() != 1 || !cfg(&tests.attrs[0]) {
        return false;
    }
    let mut lock = Lock::default();
    file.attrs
        .iter()
        .for_each(|attr| lock.visit_attribute(attr));
    lock.collect = true;
    production.iter().for_each(|item| lock.visit_item(item));
    lock.collect = false;
    body.iter().for_each(|item| lock.visit_item(item));
    lock.codes.sort_unstable();
    let edge = r#"successful_edge(workspace, packages, "s", "a/s/Cargo.toml", "t", at)"#;
    !lock.bad && lock.codes.join(" ") == EXPECTED && source.match_indices(edge).count() == 1
}
#[rustfmt::skip]
fn once(source: &str, anchor: &str, replacement: &str) -> String {
    assert_eq!(source.match_indices(anchor).count(), 1, "anchor: {anchor:?}");
    let changed = source.replacen(anchor, replacement, 1);
    assert_ne!(changed, source);
    changed
}
#[rustfmt::skip]
fn rejects(name: &str, anchor: &str, replacement: &str) {
    assert!(!locked(&once(SOURCE, anchor, replacement)), "mutation survived: {name}");
}
#[rustfmt::skip]
#[test]
fn surface_and_live_evasions_are_locked() {
    assert!(locked(SOURCE) && include_str!("../Cargo.toml").contains("[[test]]\nname = \"surface_lock\"\npath = \"tests/surface_lock.rs\""));
    let header = "#[cfg(test)]\nmod tests {";
    let early = "#[cfg(test)] mod tests {}\nconst X: &str = \"BXW9999\";\n#[cfg(test)] mod tests {";
    let inject = |name, text: &str| rejects(name, header, &format!("{text}{header}"));
    rejects("early boundary", header, early);
    rejects("crate self-disable", SOURCE, &format!("#![cfg(not(test))]\n{SOURCE}"));
    rejects("boundary self-disable", header, "#[cfg(any())]\n#[cfg(test)]\nmod tests {");
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
        ("qualified derive", "#[hidden::derive(BXW9999)] struct Hidden;\n"),
        ("aliased derive", "use hidden::derive;\n#[derive(BXW9999)] struct Hidden;\n"),
        ("test attribute", "#[test]\nfn hidden() {}\n"),
        ("cfg attribute", "#[cfg(test)]\nconst HIDDEN: Rule = (\"BXW9999\", \"hidden\");\n"),
        ("nested module", "fn hidden() { #[path = \"hidden.rs\"] mod nested; }\n"),
        ("duplicate registration", "const HIDDEN: Rule = (\"BXW0055\", \"hidden\");\n"),
        ("stale registration", "const HIDDEN: Rule = (\"BXW9999\", \"hidden\");\n"),
    ];
    for (name, mutation) in mutations {
        inject(name, mutation);
    }
    rejects("retained edge assertion", r#"successful_edge(workspace, packages, "s", "a/s/Cargo.toml", "t", at)"#, "assert_eq!(workspace.edges(), workspace.edges())");
    rejects("post-test registration", SOURCE, &format!("{SOURCE}\nconst HIDDEN: Rule = (\"BXW9999\", \"hidden\");\n"));
    rejects("macro registration", "($(#[$meta:meta] $name:ident: $return:ty = $field:tt;)*) => {$(", "($(#[$meta:meta] $name:ident: $return:ty = $field:tt;)*) => {$(const X: Rule = (\"BXW9999\", \"x\");");
    rejects("macro self-disable", "#[$meta] pub fn $name(&self) -> $return { &self.$field }", "#[cfg(test)] #[$meta] pub fn $name(&self) -> $return { &self.$field }");
}
