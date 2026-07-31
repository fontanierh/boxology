use boxology_classifier::classify;
use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use boxology_schema::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SchemaCapability, SchemaDocument,
    SchemaPayload, SchemaType, SchemaVariant, Shape,
};
use serde_json::json;
use std::fs;
use std::path::Path;
use syn::visit::{self, Visit};
use syn::{Item, Meta, Visibility};

const REVISION: &str = "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";
const OTHER_REVISION: &str =
    "sha256:a45a70dacfc5e3ea7911944d3f4fd385da1de2cdabfac86d554d4a321e3244cc";
const RUST_SOURCES: &[&str] = &["lib.rs", "tests.rs"];

#[derive(Default)]
struct MacroDetector {
    found: bool,
}

impl<'ast> Visit<'ast> for MacroDetector {
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.found = true;
        visit::visit_macro(self, item);
    }
}

fn allowed_derive(path: &syn::Path) -> bool {
    path.get_ident().is_some_and(|ident| {
        matches!(
            ident.to_string().as_str(),
            "Clone" | "Copy" | "Debug" | "Eq" | "Ord" | "PartialEq" | "PartialOrd"
        )
    })
}

fn allowed_production_attribute(attribute: &syn::Attribute) -> bool {
    if attribute.path().is_ident("doc") {
        return true;
    }
    if !attribute.path().is_ident("derive") {
        return false;
    }
    let Ok(derives) = attribute.parse_args_with(
        syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
    ) else {
        return false;
    };
    !derives.is_empty() && derives.iter().all(allowed_derive)
}

#[derive(Default)]
struct ProductionLock {
    bad: bool,
}

impl<'ast> Visit<'ast> for ProductionLock {
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        self.bad |= !allowed_production_attribute(attribute);
    }

    fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {
        self.bad = true;
    }

    fn visit_macro(&mut self, _: &'ast syn::Macro) {
        self.bad = true;
    }
}

fn require_allowed_modules(source: &str) -> Result<(), &'static str> {
    let file = syn::parse_file(source).map_err(|_| "invalid Rust source")?;
    let mut macros = MacroDetector::default();
    macros.visit_file(&file);
    if macros.found {
        return Err("production macros are forbidden");
    }
    if file.attrs.iter().any(|attribute| {
        !attribute
            .path()
            .get_ident()
            .is_some_and(|ident| matches!(ident.to_string().as_str(), "doc" | "deny" | "forbid"))
    }) {
        return Err("unexpected crate attribute");
    }
    let Some((item, production)) = file.items.split_last() else {
        return Err("tests module must be terminal");
    };
    let Item::Mod(module) = item else {
        return Err("tests module must be terminal");
    };
    let cfg_test = matches!(
        &module.attrs[..],
        [attribute]
            if matches!(&attribute.meta, Meta::List(meta)
                if meta.path.is_ident("cfg") && meta.tokens.to_string() == "test")
    );
    if module.ident != "tests"
        || !matches!(module.vis, Visibility::Inherited)
        || module.content.is_some()
        || !cfg_test
    {
        return Err("unexpected module declaration");
    }
    let mut lock = ProductionLock::default();
    for item in production {
        lock.visit_item(item);
    }
    if lock.bad {
        return Err("unexpected production attribute");
    }
    Ok(())
}

fn rust_source_inventory(root: &Path) -> Result<Vec<String>, &'static str> {
    if fs::symlink_metadata(root)
        .map_err(|_| "cannot inspect source root")?
        .file_type()
        .is_symlink()
    {
        return Err("source root symlink is forbidden");
    }
    fn visit(root: &Path, directory: &Path, sources: &mut Vec<String>) -> Result<(), &'static str> {
        for entry in fs::read_dir(directory).map_err(|_| "cannot read source directory")? {
            let entry = entry.map_err(|_| "cannot read source entry")?;
            let file_type = entry
                .file_type()
                .map_err(|_| "cannot read source entry type")?;
            if file_type.is_symlink() {
                return Err("source symlinks are forbidden");
            }
            let path = entry.path();
            if file_type.is_dir() {
                visit(root, &path, sources)?;
            } else if file_type.is_file() && path.extension().is_some_and(|value| value == "rs") {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| "source escaped source root")?;
                let parts: Result<Vec<_>, _> = relative
                    .iter()
                    .map(|part| part.to_str().ok_or("source path is not UTF-8"))
                    .collect();
                sources.push(parts?.join("/"));
            }
        }
        Ok(())
    }

    let mut sources = Vec::new();
    visit(root, root, &mut sources)?;
    sources.sort();
    Ok(sources)
}

fn document(box_id: &str) -> SchemaDocument {
    SchemaDocument {
        box_id: BoxId::new(box_id).unwrap(),
        capabilities: vec![SchemaCapability {
            name: CapabilityName::new("greet").unwrap(),
            docs: Vec::new(),
            deprecation: None,
            error: "GreetError".to_owned(),
            input: InputSlot {
                name: "name".to_owned(),
                leaf: BoundaryLeaf::String,
            },
            output: OutputSlot {
                leaf: BoundaryLeaf::String,
            },
            shape: Shape::Unary,
            max_exposure: ExposureLevel::External,
            idempotency: Idempotency::None,
        }],
        provenance: Provenance::new(json!(null)),
        revision: REVISION.to_owned(),
        types: vec![SchemaType {
            name: "GreetError".to_owned(),
            docs: Vec::new(),
            deprecation: None,
            variants: vec![SchemaVariant {
                name: "EmptyName".to_owned(),
                docs: Vec::new(),
                deprecation: None,
                payload: SchemaPayload::Unit,
            }],
        }],
    }
}

#[test]
fn surface_and_live_evasions_are_locked() {
    production_inventory_and_code_anchors_are_fail_closed();
    symlinked_source_root_fails_inventory();
    production_ast_escapes_fail_closed();
    descendant_include_fails_the_production_inventory();
    every_classifier_code_is_reachable();
}

fn production_inventory_and_code_anchors_are_fail_closed() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("src");
    assert_eq!(rust_source_inventory(&root).unwrap(), RUST_SOURCES);
    let root_source = fs::read_to_string(root.join("lib.rs")).unwrap();
    assert_eq!(require_allowed_modules(&root_source), Ok(()));
    let source = fs::read_to_string(root.join("lib.rs")).unwrap();
    let anchors = [
        ("BXC0024", "Diagnostic::classification_requires_document()"),
        ("BXC0025", "Diagnostic::box_id_mismatch()"),
        ("BXC0026", "\"BXC0026\""),
        ("BXC0027", "\"BXC0027\""),
        ("BXC0028", "\"BXC0028\""),
        ("BXC0029", "\"BXC0029\""),
        ("BXC0029 condition", "\"unknown-variant tolerance\""),
        ("classify", "pub fn classify("),
    ];
    for (code, anchor) in anchors {
        assert_eq!(source.matches(anchor).count(), 1, "{code} anchor count");
    }
}

fn symlinked_source_root_fails_inventory() {
    let link = std::env::temp_dir().join(format!("classifier-source-link-{}", std::process::id()));
    let target = link.with_extension("target");
    fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let result = rust_source_inventory(&link);
    fs::remove_file(link).unwrap();
    fs::remove_dir(target).unwrap();
    assert_eq!(result, Err("source root symlink is forbidden"));
}

fn production_ast_escapes_fail_closed() {
    let source = include_str!("../src/lib.rs");
    for mutant in [
        source.replacen("pub fn classify(", "#[cfg(test)]\npub fn classify(", 1),
        source.replacen("pub fn classify(", "#[classifier]\npub fn classify(", 1),
        source.replacen(
            "pub fn classify(",
            "#[cfg_attr(test, classifier)]\npub fn classify(",
            1,
        ),
    ] {
        assert_eq!(mutant.matches("pub fn classify(").count(), 1);
        assert_eq!(
            require_allowed_modules(&mutant),
            Err("unexpected production attribute")
        );
    }
    let divergent = format!("{source}\npub fn classify() {{}}\n");
    assert_eq!(divergent.matches("pub fn classify(").count(), 2);
    assert_eq!(
        require_allowed_modules(&divergent),
        Err("tests module must be terminal")
    );
}

fn descendant_include_fails_the_production_inventory() {
    let source = include_str!("../src/lib.rs");
    let marker = "\n#[cfg(test)]\nmod tests;";
    for attack in [
        "include!(\"hidden/probe.rs\");",
        "std::include!(\"../review_external_include.rs\");",
        "macro_rules! hidden { () => { include!(\"../hidden.rs\"); } }\nhidden!();",
    ] {
        let mutant = source.replacen(marker, &format!("\n{attack}{marker}"), 1);
        assert_eq!(
            require_allowed_modules(&mutant),
            Err("production macros are forbidden")
        );
    }
}

fn every_classifier_code_is_reachable() {
    let missing = classify(None, None).unwrap_err().into_vec();
    let mismatch = classify(Some(&document("hello")), Some(&document("other")))
        .unwrap_err()
        .into_vec();
    let introduced = classify(None, Some(&document("hello"))).unwrap();
    let removed = classify(Some(&document("hello")), None).unwrap();
    let mut changed = document("hello");
    changed.revision = OTHER_REVISION.to_owned();
    let unclassified = classify(Some(&document("hello")), Some(&changed)).unwrap();
    let mut variant_addition = document("hello");
    variant_addition.types[0].variants.push(SchemaVariant {
        name: "Other".to_owned(),
        docs: Vec::new(),
        deprecation: None,
        payload: SchemaPayload::Unit,
    });
    variant_addition.revision = OTHER_REVISION.to_owned();
    let conditional = classify(Some(&document("hello")), Some(&variant_addition)).unwrap();
    assert_eq!(
        [
            missing[0].code(),
            mismatch[0].code(),
            introduced.findings()[0].code(),
            removed.findings()[0].code(),
            unclassified.findings()[0].code(),
            conditional.findings()[0].code(),
        ],
        [
            "BXC0024", "BXC0025", "BXC0026", "BXC0027", "BXC0028", "BXC0029",
        ]
    );
}
