use boxology_init::{InitRequest, confined_destination, initialize};
use std::{fs, path::Path};

pub(crate) fn run(out: &Path) -> Result<(), String> {
    let request = InitRequest::new("example", "../boxology")
        .map_err(|error| format!("canonical initializer request failed: {error}"))?;
    let tree = initialize(&request)
        .map_err(|error| format!("canonical initialization failed: {error}"))?;
    for file in tree.files() {
        write_file(out, file.path(), file.bytes())?;
    }
    Ok(())
}

fn write_file(out: &Path, logical: &str, bytes: &[u8]) -> Result<(), String> {
    let path = confined_destination(out, logical).map_err(str::to_owned)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("create output parent: {error}"))?;
    }
    fs::write(path, bytes).map_err(|error| format!("write {logical}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[test]
    #[rustfmt::skip]
    fn request_dependent_escape_cannot_write_outside_destination() {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("boxology-init-escape-{}-{id}", std::process::id()));
        let out = root.join("out");
        fs::create_dir_all(&out).unwrap();
        let request_dependent_path = "../escape";
        let result = write_file(&out, request_dependent_path, b"escaped");
        let entries: Vec<_> = fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        fs::remove_dir_all(root).unwrap();
        assert_eq!(result, Err("generated path is not a confined relative path".into()));
        assert_eq!(entries, ["out"]);
    }
}
