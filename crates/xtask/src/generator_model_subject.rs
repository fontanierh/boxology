use boxology_contract::BoxId;
use boxology_generator_model::GenerationRequest;
use std::{fs, path::Path};

pub(crate) fn run(out: &Path) -> Result<(), String> {
    let id = || BoxId::new("subject-box").expect("fixed id is valid");
    let request = GenerationRequest::new(
        id(),
        vec![("boxology.toml".into(), b"[package]\n".to_vec())],
        vec![],
        vec!["generated/schema.json".into()],
    )
    .map_err(|error| format!("fixed valid request failed: {error}"))?;
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
        .map_err(|error| format!("write diagnostics.txt: {error}"))
}
