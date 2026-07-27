use boxology_init::{InitRequest, initialize};
use std::{fs, path::Path};

pub(crate) fn run(out: &Path) -> Result<(), String> {
    let request = InitRequest::new("example", "../boxology")
        .map_err(|error| format!("canonical initializer request failed: {error}"))?;
    let tree = initialize(&request)
        .map_err(|error| format!("canonical initialization failed: {error}"))?;
    for file in tree.files() {
        let path = out.join(file.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("create output parent: {error}"))?;
        }
        fs::write(&path, file.bytes())
            .map_err(|error| format!("write {}: {error}", file.path()))?;
    }
    Ok(())
}
