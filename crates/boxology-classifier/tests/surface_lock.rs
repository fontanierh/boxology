use boxology_classifier::classify;
use boxology_contract::{BoxId, CapabilityName, ExposureLevel, Idempotency};
use boxology_schema::{
    BoundaryLeaf, InputSlot, OutputSlot, Provenance, SchemaCapability, SchemaDocument,
    SchemaPayload, SchemaType, SchemaVariant, Shape,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::Command;
use syn::visit::{self, Visit};
use syn::{Item, Meta, Visibility};

const REVISION: &str = "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";
const RUST_SOURCES: &[&str] = &["lib.rs", "tests.rs"];
const PRODUCTION_RUST_SOURCES: &[&str] = &["lib.rs"];

#[derive(Default)]
struct IncludeDetector {
    found: bool,
}

impl<'ast> Visit<'ast> for IncludeDetector {
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.found |= item
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "include");
        visit::visit_macro(self, item);
    }
}

fn require_allowed_modules(source: &str) -> Result<(), &'static str> {
    let file = syn::parse_file(source).map_err(|_| "invalid Rust source")?;
    let mut includes = IncludeDetector::default();
    includes.visit_file(&file);
    if includes.found {
        return Err("production include macros are forbidden");
    }
    let modules: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Mod(module) => Some(module),
            _ => None,
        })
        .collect();
    if modules.len() != 1 {
        return Err("expected exactly one module");
    }
    let module = modules[0];
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
    Ok(())
}

fn rust_source_inventory(root: &Path) -> Result<Vec<String>, &'static str> {
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

fn production_source(root: &Path) -> Result<String, &'static str> {
    let mut source = String::new();
    for relative in PRODUCTION_RUST_SOURCES {
        source.push_str(
            &fs::read_to_string(root.join(relative))
                .map_err(|_| "cannot read production source")?,
        );
        source.push('\n');
    }
    Ok(source)
}

fn cargo_metadata(manifest_dir: &Path) -> Result<Value, &'static str> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest_dir.join("Cargo.toml"))
        .output()
        .map_err(|_| "cannot run cargo metadata")?;
    if !output.status.success() {
        return Err("cargo metadata failed");
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "cargo metadata was not JSON")
}

fn require_cargo_targets(metadata: &Value, manifest_dir: &Path) -> Result<(), &'static str> {
    let manifest = manifest_dir.join("Cargo.toml");
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata omitted packages")?;
    let package = packages
        .iter()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| Path::new(path) == manifest)
        })
        .ok_or("cargo metadata omitted classifier package")?;
    let targets = package["targets"]
        .as_array()
        .ok_or("cargo metadata omitted targets")?;
    let has_kind = |target: &Value, expected: &str| {
        target["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == expected))
    };
    if targets
        .iter()
        .any(|target| has_kind(target, "custom-build"))
    {
        return Err("build hooks are forbidden");
    }
    let libraries: Vec<_> = targets
        .iter()
        .filter(|target| has_kind(target, "lib"))
        .collect();
    if libraries.len() != 1 {
        return Err("expected exactly one library target");
    }
    let source = libraries[0]["src_path"]
        .as_str()
        .ok_or("library target omitted source path")?;
    if Path::new(source) != manifest_dir.join("src/lib.rs") {
        return Err("library target must be src/lib.rs");
    }
    Ok(())
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
fn production_inventory_and_code_anchors_are_fail_closed() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("src");
    assert_eq!(rust_source_inventory(&root).unwrap(), RUST_SOURCES);
    let root_source = fs::read_to_string(root.join("lib.rs")).unwrap();
    assert_eq!(require_allowed_modules(&root_source), Ok(()));
    assert_eq!(
        require_cargo_targets(&cargo_metadata(manifest_dir).unwrap(), manifest_dir),
        Ok(())
    );
    let source = production_source(&root).unwrap();
    let anchors = [
        ("BXC0024", "Diagnostic::classification_requires_document()"),
        ("BXC0025", "Diagnostic::box_id_mismatch()"),
        ("BXC0026", "\"BXC0026\""),
        ("BXC0027", "\"BXC0027\""),
        ("BXC0028", "\"BXC0028\""),
        ("BXC0029", "\"BXC0029\""),
        ("BXC0029 condition", "\"unknown-variant tolerance\""),
    ];
    for (code, anchor) in anchors {
        assert_eq!(source.matches(anchor).count(), 1, "{code} anchor count");
    }
}
#[test]
fn attributed_public_module_fails_the_ast_inventory() {
    let source = include_str!("../src/lib.rs");
    let attacks = [
        "mod stray {}",
        "mod probe;",
        "pub mod stray {}",
        "pub(crate) mod stray {}",
        "#[allow(dead_code)] pub mod stray {}",
    ];
    for attack in attacks {
        assert_eq!(
            require_allowed_modules(&format!("{source}\n{attack}\n")),
            Err("expected exactly one module")
        );
    }
}

#[test]
fn descendant_include_fails_the_production_inventory() {
    let source = include_str!("../src/lib.rs");
    for attack in [
        "include!(\"hidden/probe.rs\");",
        "std::include!(\"../review_external_include.rs\");",
    ] {
        assert_eq!(
            require_allowed_modules(&format!("{source}\n{attack}\n")),
            Err("production include macros are forbidden")
        );
    }
}

#[test]
fn alternate_root_and_build_hook_fail_cargo_target_lock() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = manifest_dir.join("Cargo.toml");
    let alternate = manifest_dir.join("review_alt_root.rs");
    let metadata = |targets: Value| {
        json!({"packages": [{
            "manifest_path": manifest,
            "targets": targets,
        }]})
    };
    assert_eq!(
        require_cargo_targets(
            &metadata(json!([{"kind": ["lib"], "src_path": alternate}])),
            manifest_dir,
        ),
        Err("library target must be src/lib.rs")
    );
    assert_eq!(
        require_cargo_targets(
            &metadata(json!([
                {"kind": ["lib"], "src_path": manifest_dir.join("src/lib.rs")},
                {"kind": ["custom-build"], "src_path": manifest_dir.join("build.rs")},
            ])),
            manifest_dir,
        ),
        Err("build hooks are forbidden")
    );
}

#[test]
fn every_classifier_code_is_reachable() {
    let missing = classify(None, None).unwrap_err().into_vec();
    let mismatch = classify(Some(&document("hello")), Some(&document("other")))
        .unwrap_err()
        .into_vec();
    let introduced = classify(None, Some(&document("hello"))).unwrap();
    let removed = classify(Some(&document("hello")), None).unwrap();
    let mut changed = document("hello");
    changed.revision.push('x');
    let unclassified = classify(Some(&document("hello")), Some(&changed)).unwrap();
    let mut variant_addition = document("hello");
    variant_addition.types[0].variants.push(SchemaVariant {
        name: "Other".to_owned(),
        docs: Vec::new(),
        deprecation: None,
        payload: SchemaPayload::Unit,
    });
    variant_addition.revision.push('x');
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
