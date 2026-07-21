use boxology_contract::BoxId;
use boxology_generator_model::{
    ContractDeclarationSyntax, GenerationRequest, Manifest, ParsedRustInputs,
};
use std::{fs, path::Path};

pub(crate) fn run(out: &Path) -> Result<(), String> {
    let id = || BoxId::new("subject-box").expect("fixed id is valid");
    let input = |path: &str, bytes: &[u8]| (path.into(), bytes.to_vec());
    let request = GenerationRequest::new(
        id(),
        "source/custom-entry.rs".into(),
        vec![
            (
                "boxology.toml".into(),
                b"schema = 1\nid = \"subject-box\"\nkind = \"box\"\n".to_vec(),
            ),
            (
                "source/custom-entry.rs".into(),
                b"#[boxology::contract]\nstruct Root;\nmod flat;\nmod inline { #[boxology::contract] struct Inner; mod r#type; }\nfn custom_entry() {}\n".to_vec(),
            ),
            ("source/flat.rs".into(), b"#[boxology::contract]\nenum Flat { A }\n".to_vec()),
            ("source/inline/type.rs".into(), b"#[boxology::contract(error)]\nenum Raw { A }\n".to_vec()),
            ("source/unreachable.rs".into(), b"fn hidden() {}\n".to_vec()),
            ("src/z.rs".into(), b"fn z() {}\n".to_vec()),
            ("src/a.rs".into(), b"struct A;\nfn a() {}\n".to_vec()),
        ],
        vec![],
        vec!["generated/schema.json".into()],
    )
    .map_err(|error| format!("fixed valid request failed: {error}"))?;
    let manifest = Manifest::parse(&request)
        .map_err(|error| format!("fixed valid manifest failed: {error}"))?;
    let rust_inputs = ParsedRustInputs::parse(&request)
        .map_err(|error| format!("fixed valid Rust inputs failed: {error}"))?;
    let rust_modules = rust_inputs
        .resolve_reachable_inputs()
        .map_err(|error| format!("fixed valid module topology failed: {error}"))?
        .iter()
        .map(|input| input.path().as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let declaration_projection = rust_inputs
        .discover_contract_declarations()
        .map_err(|error| format!("fixed contract declarations failed: {error}"))?
        .into_iter()
        .map(|declaration| {
            let kind = match declaration.syntax() {
                ContractDeclarationSyntax::Struct(_) => "struct",
                ContractDeclarationSyntax::Enum(_) => "enum",
            };
            format!(
                "{kind} module={:?} name={} source={} span={:?}",
                declaration.module_path(),
                declaration.lifted_name(),
                declaration.source().as_str(),
                declaration.identifier_span()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let collision_request = GenerationRequest::new(
        id(),
        "c.rs".into(),
        vec![
            input("boxology.toml", request.inputs()[0].bytes()),
            input("c.rs", b"#[boxology::contract] struct Clash;\nmod n;\n"),
            input("n.rs", b"#[boxology::contract] enum r#Clash { A }\n"),
        ],
        vec![],
        vec![],
    )
    .map_err(|error| format!("fixed collision request failed: {error}"))?;
    let collision_inputs = ParsedRustInputs::parse(&collision_request)
        .map_err(|error| format!("fixed collision inputs failed: {error}"))?;
    let collision_diagnostics = match collision_inputs.discover_contract_declarations() {
        Ok(_) => return Err("fixed collision unexpectedly succeeded".into()),
        Err(diagnostics) => diagnostics,
    };
    let invalid_manifest_request = GenerationRequest::new(
        id(),
        "source/custom-entry.rs".into(),
        vec![
            (
                "boxology.toml".into(),
                b"schema = 1\nid = \"subject-box\"\nkind = \"composition\"\n".to_vec(),
            ),
            ("source/custom-entry.rs".into(), vec![]),
        ],
        vec![],
        vec![],
    )
    .map_err(|error| format!("fixed invalid-manifest request failed: {error}"))?;
    let manifest_diagnostics =
        Manifest::parse(&invalid_manifest_request).expect_err("fixed invalid manifest must fail");
    let invalid_rust_request = GenerationRequest::new(
        id(),
        "a.rs".into(),
        vec![
            ("boxology.toml".into(), b"manifest\n".to_vec()),
            ("b.rs".into(), "fn café() { @ }\n".as_bytes().to_vec()),
            ("a.rs".into(), b"fn good() {}\nfn bad() { @ }\n".to_vec()),
        ],
        vec![],
        vec![],
    )
    .map_err(|error| format!("fixed invalid-Rust request failed: {error}"))?;
    let rust_diagnostics = match ParsedRustInputs::parse(&invalid_rust_request) {
        Ok(_) => return Err("fixed invalid Rust inputs unexpectedly parsed".into()),
        Err(diagnostics) => diagnostics,
    };
    let invalid_module_request = GenerationRequest::new(
        id(),
        "modules/root.rs".into(),
        vec![
            (
                "boxology.toml".into(),
                b"schema = 1\nid = \"subject-box\"\nkind = \"box\"\n".to_vec(),
            ),
            (
                "modules/root.rs".into(),
                b"#[path = \"private.rs\"] mod redirected;\nmod missing;\nmod duplicate;\n"
                    .to_vec(),
            ),
            ("modules/duplicate.rs".into(), vec![]),
            ("modules/duplicate/mod.rs".into(), vec![]),
        ],
        vec![],
        vec![],
    )
    .map_err(|error| format!("fixed invalid-topology request failed: {error}"))?;
    let invalid_modules = ParsedRustInputs::parse(&invalid_module_request)
        .map_err(|error| format!("fixed invalid-topology Rust inputs failed: {error}"))?;
    let module_diagnostics = match invalid_modules.resolve_reachable_inputs() {
        Ok(_) => return Err("fixed invalid module topology unexpectedly resolved".into()),
        Err(diagnostics) => diagnostics,
    };
    let declaration_request = GenerationRequest::new(
        id(),
        "d/r.rs".into(),
        vec![
            input(
                "boxology.toml",
                b"schema = 1\nid = \"subject-box\"\nkind = \"box\"\n",
            ),
            input("d/r.rs", b"#[cfg(private)]\nmod exported;\n"),
            input("d/exported.rs", b"#[boxology::contract]\nstruct Export;\n"),
            input("d/dead.rs", b"#[boxology::capability]\nfn hidden() {}\n"),
        ],
        vec![],
        vec![],
    )
    .map_err(|error| format!("fixed declaration-error request failed: {error}"))?;
    let declaration_inputs = ParsedRustInputs::parse(&declaration_request)
        .map_err(|error| format!("fixed declaration-error inputs failed: {error}"))?;
    let declaration_diagnostics = match declaration_inputs.resolve_reachable_inputs() {
        Ok(_) => return Err("fixed declaration errors unexpectedly resolved".into()),
        Err(diagnostics) => diagnostics,
    };
    let diagnostics = GenerationRequest::new(
        id(),
        "root.rs".into(),
        vec![("root.rs".into(), vec![]), ("/absolute.rs".into(), vec![])],
        vec![],
        vec![],
    )
    .expect_err("fixed invalid request must fail");

    let summary = format!(
        "box_id={}\ncrate_root={}\ninputs={}\nimports={}\noutputs={}\n",
        request.box_id(),
        request.crate_root().as_str(),
        request.inputs().len(),
        request.imports().len(),
        request.outputs().len()
    );
    fs::write(out.join("request.txt"), summary)
        .map_err(|error| format!("write request.txt: {error}"))?;
    fs::write(out.join("diagnostics.txt"), format!("{diagnostics}\n"))
        .map_err(|error| format!("write diagnostics.txt: {error}"))?;
    fs::write(
        out.join("manifest.txt"),
        format!("manifest_id={}\n", manifest.id()),
    )
    .map_err(|error| format!("write manifest.txt: {error}"))?;
    fs::write(
        out.join("manifest-diagnostics.txt"),
        format!("{manifest_diagnostics}\n"),
    )
    .map_err(|error| format!("write manifest-diagnostics.txt: {error}"))?;
    let rust_summary = rust_inputs
        .as_slice()
        .iter()
        .map(|input| {
            format!(
                "{} items={}",
                input.path().as_str(),
                input.syntax().items.len()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(out.join("rust-inputs.txt"), format!("{rust_summary}\n"))
        .map_err(|error| format!("write rust-inputs.txt: {error}"))?;
    fs::write(out.join("rust-modules.txt"), format!("{rust_modules}\n"))
        .map_err(|error| format!("write rust-modules.txt: {error}"))?;
    fs::write(
        out.join("module-diagnostics.txt"),
        format!("{module_diagnostics}\n"),
    )
    .map_err(|error| format!("write module-diagnostics.txt: {error}"))?;
    fs::write(
        out.join("declaration-diagnostics.txt"),
        format!("{declaration_diagnostics}\n"),
    )
    .map_err(|error| format!("write declaration-diagnostics.txt: {error}"))?;
    fs::write(
        out.join("contract-declarations.txt"),
        format!("success\n{declaration_projection}\ncollision\n{collision_diagnostics}\n"),
    )
    .map_err(|error| format!("write contract-declarations.txt: {error}"))?;
    fs::write(
        out.join("rust-diagnostics.txt"),
        format!("{rust_diagnostics}\n"),
    )
    .map_err(|error| format!("write rust-diagnostics.txt: {error}"))
}
