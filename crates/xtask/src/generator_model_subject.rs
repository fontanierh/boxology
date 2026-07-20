use boxology_contract::BoxId;
use boxology_generator_model::{GenerationRequest, Manifest};
use std::{fs, path::Path};

pub(crate) fn run(out: &Path) -> Result<(), String> {
    let id = || BoxId::new("subject-box").expect("fixed id is valid");
    let request = GenerationRequest::new(
        id(),
        vec![(
            "boxology.toml".into(),
            b"schema = 1\nid = \"subject-box\"\nkind = \"box\"\n".to_vec(),
        )],
        vec![],
        vec!["generated/schema.json".into()],
    )
    .map_err(|error| format!("fixed valid request failed: {error}"))?;
    let manifest = Manifest::parse(&request)
        .map_err(|error| format!("fixed valid manifest failed: {error}"))?;
    let invalid_manifest_request = GenerationRequest::new(
        id(),
        vec![(
            "boxology.toml".into(),
            b"schema = 1\nid = \"subject-box\"\nkind = \"composition\"\n".to_vec(),
        )],
        vec![],
        vec![],
    )
    .map_err(|error| format!("fixed invalid-manifest request failed: {error}"))?;
    let manifest_diagnostics =
        Manifest::parse(&invalid_manifest_request).expect_err("fixed invalid manifest must fail");
    let diagnostics =
        GenerationRequest::new(id(), vec![("/absolute.rs".into(), vec![])], vec![], vec![])
            .expect_err("fixed invalid request must fail");

    let summary = format!(
        "box_id={}\ninputs={}\nimports={}\noutputs={}\n",
        request.box_id(),
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
    .map_err(|error| format!("write manifest-diagnostics.txt: {error}"))
}
