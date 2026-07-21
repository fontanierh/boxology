use boxology_contract::BoxId;
use boxology_generator_model::{
    ContractDeclaration, ContractDeclarationShape, ContractDeclarationSyntax, ContractFields,
    GenerationRequest, Manifest, ParsedRustInputs,
};
use std::{fs, path::Path};

fn field_projection(fields: &ContractFields<'_>) -> String {
    let fields = match fields {
        ContractFields::Named(fields) | ContractFields::Unnamed(fields) => fields,
        ContractFields::Unit => return "unit".into(),
    };
    format!(
        "{:?}",
        fields
            .iter()
            .map(|field| (
                field.ordinal(),
                field.identity().map(|identity| identity.name()),
                field.metadata().deprecation().map(|value| value.note()),
            ))
            .collect::<Vec<_>>()
    )
}

fn shape_projection(declaration: &ContractDeclaration<'_>) -> String {
    let deprecated = declaration
        .metadata()
        .deprecation()
        .map(|value| value.note());
    match declaration.shape() {
        ContractDeclarationShape::Struct(fields) => format!(
            "{} struct deprecated={deprecated:?} fields={}",
            declaration.lifted_name(),
            field_projection(fields)
        ),
        ContractDeclarationShape::Enum(variants) => format!(
            "{} enum deprecated={deprecated:?} variants={:?}",
            declaration.lifted_name(),
            variants
                .iter()
                .map(|variant| (
                    variant.ordinal(),
                    variant.identity().name(),
                    variant.metadata().deprecation().map(|value| value.note()),
                    field_projection(variant.fields()),
                ))
                .collect::<Vec<_>>()
        ),
    }
}

fn docs_projection(declaration: &ContractDeclaration<'_>) -> String {
    let fields = |fields: &ContractFields<'_>| match fields {
        ContractFields::Named(fields) | ContractFields::Unnamed(fields) => format!(
            "{:?}",
            fields
                .iter()
                .map(|field| field.metadata().docs())
                .collect::<Vec<_>>()
        ),
        ContractFields::Unit => "unit".into(),
    };
    match declaration.shape() {
        ContractDeclarationShape::Struct(shape) => format!(
            "{} struct docs={:?} fields={}",
            declaration.lifted_name(),
            declaration.metadata().docs(),
            fields(shape)
        ),
        ContractDeclarationShape::Enum(variants) => format!(
            "{} enum docs={:?} variants={:?}",
            declaration.lifted_name(),
            declaration.metadata().docs(),
            variants
                .iter()
                .map(|variant| (variant.metadata().docs(), fields(variant.fields())))
                .collect::<Vec<_>>()
        ),
    }
}

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
    let contracts = |source: &[u8], projection| -> Result<_, String> {
        let evaluate = |reversed| -> Result<_, String> {
            let mut files = vec![
                input("boxology.toml", request.inputs()[0].bytes()),
                input("attributes.rs", source),
            ];
            if reversed {
                files.reverse();
            }
            let case = GenerationRequest::new(id(), "attributes.rs".into(), files, vec![], vec![])
                .map_err(|error| format!("fixed attribute request failed: {error}"))?;
            let parsed = ParsedRustInputs::parse(&case)
                .map_err(|error| format!("fixed attribute inputs failed: {error}"))?;
            Ok(parsed.discover_contract_declarations().map(|declarations| {
                declarations
                    .iter()
                    .map(|declaration| {
                        if projection == 3 {
                            docs_projection(declaration)
                        } else if projection == 2 {
                            shape_projection(declaration)
                        } else if projection == 1 {
                            format!("{}={:?}", declaration.lifted_name(), declaration.role())
                        } else {
                            declaration.lifted_name().to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }))
        };
        Ok([evaluate(false)?, evaluate(true)?])
    };
    let valid_attributes = contracts(b"#[doc = \"allowed\"]\n#[deprecated(note = \"later\")]\n#[derive(Debug, Clone, PartialEq)]\n#[boxology::contract]\nstruct Allowed { #[boxology::field] value: u8 }\n", 0)?;
    let invalid_attributes = contracts(b"#[boxology::contract]\n#[PrivateAttribute(secret)]\nstruct Invalid { #[derive(PrivateDerive)] field: u8 }\n", 0)?;
    if valid_attributes[0] != valid_attributes[1] || invalid_attributes[0] != invalid_attributes[1]
    {
        return Err("contract attribute result changed with input order".into());
    }
    let attribute_projection = valid_attributes[0]
        .as_ref()
        .map_err(|error| format!("fixed valid attributes failed: {error}"))?;
    let attribute_diagnostics = invalid_attributes[0]
        .as_ref()
        .expect_err("fixed invalid attributes must fail");
    let valid_roles = contracts(b"#[boxology::contract]\nstruct Value;\n#[boxology::contract]\nenum Choice { A }\n#[boxology::contract(error)]\nenum Fault { A }\n", 1)?;
    let invalid_roles = contracts(b"#[boxology::contract(error)]\nstruct PrivateStruct;\n#[boxology::contract(PrivateMarker)]\nenum PrivateEnum { PrivateVariant }\n", 1)?;
    if valid_roles[0] != valid_roles[1] || invalid_roles[0] != invalid_roles[1] {
        return Err("contract role result changed with input order".into());
    }
    let role_projection = valid_roles[0]
        .as_ref()
        .map_err(|error| format!("fixed valid roles failed: {error}"))?;
    let role_diagnostics = invalid_roles[0]
        .as_ref()
        .expect_err("fixed invalid roles must fail");
    let valid_deprecations = contracts(b"#[deprecated]\n#[boxology::contract]\nstruct Value { #[deprecated(note = \"field\")] field: u8 }\n#[deprecated(note = \"type\")]\n#[boxology::contract]\nenum Event { #[deprecated] Unit, Named { #[deprecated(note = \"variant field\")] value: u8 } }\n", 0)?;
    let invalid_deprecations = contracts(b"#[deprecated(PrivateType)]\n#[boxology::contract]\nstruct Invalid { #[deprecated(note = PrivateField)] field: u8 }\n#[boxology::contract]\nenum InvalidEvent { #[deprecated(PrivateVariant)] Bad { #[deprecated(note = PrivateVariantField)] value: u8 } }\n", 0)?;
    if valid_deprecations[0] != valid_deprecations[1]
        || invalid_deprecations[0] != invalid_deprecations[1]
    {
        return Err("contract deprecation result changed with input order".into());
    }
    let deprecation_projection = valid_deprecations[0]
        .as_ref()
        .map_err(|error| format!("fixed valid deprecations failed: {error}"))?;
    let deprecation_diagnostics = invalid_deprecations[0]
        .as_ref()
        .expect_err("fixed invalid deprecations must fail");
    let shapes = contracts(b"#[deprecated]\n#[boxology::contract]\nstruct Named { #[deprecated(note = \"named\\nfield\")] r#field: &'static [u8; 7] }\n#[deprecated(note = \"tuple\")] #[boxology::contract] struct Tuple(#[deprecated] u8);\n#[boxology::contract] struct Unit;\n#[deprecated(note = \"error\")] #[boxology::contract(error)] enum Event { #[deprecated] r#Unit, #[deprecated(note = \"variant\")] Tuple(#[deprecated] u8), Named { #[deprecated(note = \"value\")] r#value: u8 } }\n", 2)?;
    if shapes[0] != shapes[1] {
        return Err("contract shape result changed with input order".into());
    }
    let shape_projection = shapes[0]
        .as_ref()
        .map_err(|error| format!("fixed valid shapes failed: {error}"))?;
    let valid_docs = contracts(b"#[doc = \" first \" ] #[r#doc = r#\"second\"#] #[boxology::contract] struct Named { #[doc = \"\"] value: u8 }\n#[doc = \"tuple\"] #[boxology::contract] struct Tuple(#[doc = \" tuple field \" ] u8);\n#[boxology::contract] struct Unit;\n#[doc = \"enum\"] #[boxology::contract(error)] enum Event { #[doc = \"unit variant\"] Unit, #[doc = \"tuple variant\"] Tuple(#[doc = \"tuple field\"] u8), #[doc = \"named variant\"] Named { #[doc = \"\"] value: u8 } }\n", 3)?;
    let invalid_docs = contracts(b"#[doc] #[boxology::contract] struct Invalid { #[doc = 7] field: u8 }\n#[boxology::contract] enum InvalidEvent { #[doc(private)] Bad { #[r#doc = concat!(\"private\")] field: u8 } }\n", 3)?;
    if valid_docs[0] != valid_docs[1] || invalid_docs[0] != invalid_docs[1] {
        return Err("contract documentation result changed with input order".into());
    }
    let docs = valid_docs[0]
        .as_ref()
        .map_err(|error| format!("fixed valid documentation failed: {error}"))?;
    let doc_diagnostics = invalid_docs[0]
        .as_ref()
        .expect_err("fixed invalid documentation must fail");
    let valid_members = contracts(
        b"#[boxology::contract]\nstruct First { r#type: u8, field: u16 }\n#[boxology::contract(error)]\nenum Event { r#match, Named { r#type: u8, field: u16 }, Tuple(u8), Unit }\n",
        2,
    )?;
    let invalid_members = contracts(
        b"#[boxology::contract]\nstruct PrivateOne { PrivateField: u8, PrivateField: u16 }\n#[boxology::contract(error)]\nenum PrivateEvent { PrivateVariant { PrivateNested: u8, PrivateNested: u16 }, PrivateVariant }\n",
        2,
    )?;
    if valid_members[0] != valid_members[1] || invalid_members[0] != invalid_members[1] {
        return Err("contract member result changed with input order".into());
    }
    let member_projection = valid_members[0]
        .as_ref()
        .map_err(|error| format!("fixed valid members failed: {error}"))?;
    let member_diagnostics = invalid_members[0]
        .as_ref()
        .expect_err("fixed invalid members must fail");
    let placement = |root: &[u8], child: &[u8]| -> Result<_, String> {
        let evaluate = |reversed| -> Result<_, String> {
            let mut files = vec![
                input("boxology.toml", request.inputs()[0].bytes()),
                input("placement/root.rs", root),
                input("placement/child.rs", child),
            ];
            if reversed {
                files.reverse();
            }
            let case =
                GenerationRequest::new(id(), "placement/root.rs".into(), files, vec![], vec![])
                    .map_err(|error| format!("fixed placement request failed: {error}"))?;
            let parsed = ParsedRustInputs::parse(&case)
                .map_err(|error| format!("fixed placement inputs failed: {error}"))?;
            let contracts = parsed.discover_contract_declarations();
            if contracts.as_ref().is_ok_and(Vec::is_empty) {
                return Ok(parsed.discover_capability_declarations().map(|items| {
                    items
                        .iter()
                        .map(|item| {
                            format!(
                                "{}|{:?}|{}|{:?}|{}",
                                item.method().sig.ident,
                                item.module_path(),
                                item.source().as_str(),
                                item.identifier_span(),
                                item.implementation().items.len()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }));
            }
            Ok(contracts.map(|items| {
                items
                    .iter()
                    .map(|item| item.lifted_name())
                    .collect::<Vec<_>>()
                    .join("\n")
            }))
        };
        let results = [evaluate(false)?, evaluate(true)?];
        if results[0] != results[1] {
            return Err("contract placement result changed with input order".into());
        }
        Ok(results.into_iter().next().unwrap())
    };
    let placement_projection = placement(
        b"mod child;\n#[boxology::contract] struct Root;\n",
        b"#[::boxology::contract(error)] enum Fault { A }\n",
    )?
    .map_err(|error| format!("fixed valid placement failed: {error}"))?;
    let placement_diagnostics = placement(
        b"mod child;\n#[boxology::contract(PrivateList)] fn PrivateRoot() {}\n",
        b"struct Hidden { #[::boxology::contract = \"PrivateValue\"] value: u8 }\n",
    )?
    .expect_err("fixed invalid placement must fail");
    let capability_projection = placement(
        b"mod child; struct Root; impl Root { #[boxology::capability] fn root() {} } mod inline { struct Inline; impl Inline { #[::r#boxology::r#capability] fn inline() {} } } fn outer() { mod local { struct Host; impl Host { #[boxology::capability] fn cap() {} } } } #[holder = { struct Phantom; impl Phantom { #[cfg(Private)] #[boxology::capability] fn hidden() {} } }] fn payload() {}\n",
        b"struct Child; impl Child { #[::boxology::capability] fn child() {} }\n",
    )?.map_err(|error| format!("fixed valid capability placement failed: {error}"))?;
    let expected_capabilities = "root|[]|placement/root.rs|Span { start: LineColumn { line: 1, column: 64 }, end: LineColumn { line: 1, column: 68 } }|1\nchild|[\"child\"]|placement/child.rs|Span { start: LineColumn { line: 1, column: 57 }, end: LineColumn { line: 1, column: 62 } }|1\ninline|[\"inline\"]|placement/root.rs|Span { start: LineColumn { line: 1, column: 151 }, end: LineColumn { line: 1, column: 157 } }|1\ncap|[\"local\"]|placement/root.rs|Span { start: LineColumn { line: 1, column: 244 }, end: LineColumn { line: 1, column: 247 } }|1";
    if capability_projection != expected_capabilities {
        return Err("fixed capability projection changed".into());
    }
    let capability_diagnostics = placement(
        b"mod child; #[boxology::capability] fn PrivatePath() {} #[boxology::capability(PrivateList)] trait PrivateTrait {}\n",
        b"struct Hidden { #[::boxology::capability = \"PrivateValue\"] value: u8 }\n",
    )?.expect_err("fixed invalid capability placement must fail");
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
        out.join("contract-attributes.txt"),
        format!(
            "success count={}\n{attribute_projection}\ninvalid\n{attribute_diagnostics}\n",
            attribute_projection.lines().count()
        ),
    )
    .map_err(|error| format!("write contract-attributes.txt: {error}"))?;
    fs::write(
        out.join("contract-roles.txt"),
        format!("success\n{role_projection}\ninvalid\n{role_diagnostics}\n"),
    )
    .map_err(|error| format!("write contract-roles.txt: {error}"))?;
    fs::write(
        out.join("contract-deprecations.txt"),
        format!("success\n{deprecation_projection}\ninvalid\n{deprecation_diagnostics}\n"),
    )
    .map_err(|error| format!("write contract-deprecations.txt: {error}"))?;
    fs::write(
        out.join("contract-shapes.txt"),
        format!("success\n{shape_projection}\n"),
    )
    .map_err(|error| format!("write contract-shapes.txt: {error}"))?;
    fs::write(
        out.join("contract-docs.txt"),
        format!("success\n{docs}\ninvalid\n{doc_diagnostics}\n"),
    )
    .map_err(|error| format!("write contract-docs.txt: {error}"))?;
    fs::write(
        out.join("contract-members.txt"),
        format!("success\n{member_projection}\ninvalid\n{member_diagnostics}\n"),
    )
    .map_err(|error| format!("write contract-members.txt: {error}"))?;
    fs::write(
        out.join("contract-placement.txt"),
        format!("success\n{placement_projection}\ninvalid\n{placement_diagnostics}\n"),
    )
    .map_err(|error| format!("write contract-placement.txt: {error}"))?;
    fs::write(
        out.join("capability-placement.txt"),
        format!("success\n{capability_projection}\ninvalid\n{capability_diagnostics}\n",),
    )
    .map_err(|error| format!("write capability-placement.txt: {error}"))?;
    fs::write(
        out.join("rust-diagnostics.txt"),
        format!("{rust_diagnostics}\n"),
    )
    .map_err(|error| format!("write rust-diagnostics.txt: {error}"))
}
