use boxology_manifest::RelativePath;
use boxology_workspace::{FileEntry, WorkspaceInputs};
use std::{fs, path::Path};

// A fixed in-memory listing, deliberately out of report order. No host path, clock, environment
// value, locale, or map iteration reaches the report, so its bytes are identical on every
// platform and every repetition by construction.
const LISTING: [(&str, Option<&str>); 5] = [
    ("z/escape", Some("../../outside")),
    ("a/b/escape", Some("/etc/passwd")),
    ("a/keep", Some("../b/c.rs")),
    ("a/b/c.rs", None),
    ("a/escape", Some("..\\windows")),
];
// Two fixed manifests, so the subject covers a mixed report — a manifest parse diagnostic
// interleaved with workspace findings — and not only the symlink escapes above.
const PARSES: &str = "schema = 1\nid = \"demo\"\nkind = \"box\"\nowned = []\n";
const MANIFESTS: [(&str, &str); 2] = [
    ("a/boxology.toml", PARSES),
    ("b/boxology.toml", "schema = 9\nnot toml"),
];
// A third fixed input set, this one clean, so the subject covers the success body too: every
// tracked file classified exactly once, rendered in the frozen (package id, path) order.
// The `[[derived]]` element makes the subject cover a derived classification, and the lockfile
// under `c/bad/` pins the distinction the workspace lockfile rule turns on: a fixture subtree's own
// lockfile is this package's owned non-derived material, not its declared global artifact.
// The two `[[crates]]` entries map the two Cargo members below, so the subject covers a clean
// crate mapping as well: a platform package hosting platform crates, one nested inside the other.
const OWNS: &str = "schema = 1\nid = \"root\"\nkind = \"platform\"\n\
                    owned = [\"boxology.toml\", \"z/**\"]\nfixtures = [\"c/**\"]\n\
                    [[crates]]\ncargo_package = \"zulu\"\npath = \"z\"\nrole = \"platform\"\n\
                    [[crates]]\ncargo_package = \"alpha\"\npath = \"z/a\"\nrole = \"platform\"\n\
                    [[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\n\
                    inputs = [\"boxology.toml\"]\noutputs = [\"Cargo.lock\"]\n";
// A fixed `cargo metadata` document, so the subject covers the members read out of one: two whose
// declaration order is not their sorted order, plus one `packages[]` element no `workspace_members`
// entry names. No host path reaches it; `/w` is as fixed as the listing above.
const MEMBERS: &str = "{\"workspace_root\":\"/w\",\"workspace_members\":[\"a\",\"b\"],\
                       \"packages\":[{\"id\":\"a\",\"name\":\"zulu\",\
                       \"manifest_path\":\"/w/z/Cargo.toml\"},{\"id\":\"b\",\"name\":\"alpha\",\
                       \"manifest_path\":\"/w/z/a/Cargo.toml\"},{\"id\":\"c\",\"name\":\"vendor\",\
                       \"manifest_path\":\"/vendor/Cargo.toml\"}]}";
const NO_MEMBERS: &str = "{\"workspace_root\":\"/w\",\"workspace_members\":[],\"packages\":[]}";
const TRACKED: [&str; 6] = [
    "z/b.rs",
    "c/bad/boxology.toml",
    "z/a.rs",
    "boxology.toml",
    "Cargo.lock",
    "c/bad/Cargo.lock",
];
fn rel(path: &str) -> Result<RelativePath, String> {
    RelativePath::new(path).map_err(|_| format!("fixed path {path} is invalid"))
}
fn classification() -> Result<String, String> {
    let mut files = Vec::new();
    for path in TRACKED {
        files.push(FileEntry::file(rel(path)?));
    }
    let manifests = vec![
        (rel("boxology.toml")?, OWNS.as_bytes().to_vec()),
        (rel("c/bad/boxology.toml")?, b"not toml".to_vec()),
    ];
    let inputs = WorkspaceInputs::new(files, manifests, MEMBERS)
        .map_err(|_| String::from("fixed clean listing rejected"))?;
    let workspace = inputs.check().map_err(|found| format!("found {found}"))?;
    let mut body = format!("{}\n", workspace.render_report());
    for held in workspace.cargo_members() {
        let at = held.crate_dir().map_or("", RelativePath::as_str);
        body.push_str(&format!("{} {at}\n", held.cargo_package()));
    }
    Ok(body)
}
fn report() -> Result<String, String> {
    let mut files = Vec::new();
    for (path, target) in LISTING {
        let path = rel(path)?;
        files.push(match target {
            Some(target) => FileEntry::symlink(path, String::from(target)),
            None => FileEntry::file(path),
        });
    }
    let mut manifests = Vec::new();
    for (path, text) in MANIFESTS {
        let path = rel(path)?;
        files.push(FileEntry::file(path.clone()));
        manifests.push((path, text.as_bytes().to_vec()));
    }
    let inputs = WorkspaceInputs::new(files, manifests, NO_MEMBERS)
        .map_err(|_| String::from("fixed listing rejected"))?;
    let findings = inputs.check().err().ok_or("fixed listing is clean")?;
    Ok(format!("{findings}\n"))
}
pub(crate) fn run(out: &Path) -> Result<(), String> {
    let written = [
        ("workspace-report.txt", report()?),
        ("workspace-classification.txt", classification()?),
    ];
    for (name, body) in written {
        fs::write(out.join(name), body).map_err(|error| format!("write {name}: {error}"))?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn subject_report_is_golden_and_repeatable() {
        let rendered = super::report().expect("the fixed listing renders");
        assert_eq!(rendered, super::report().expect("it renders again"));
        assert_eq!(
            rendered,
            "BXW0048 a/b/escape package= candidates=[]\n\
             BXW0048 a/escape package= candidates=[]\n\
             BXW0002 b/boxology.toml:2:5-2:5 offending=\"manifest document\" \
             rule=\"boxology.toml must be well-formed TOML\" \
             source=\"specs/s5-manifest-and-validation.md D2\"\n\
             BXW0048 z/escape package= candidates=[]\n"
        );
    }
    #[test]
    fn subject_classification_is_golden_and_repeatable() {
        let again = super::classification();
        let rendered = super::classification().expect("the clean listing classifies");
        assert_eq!(rendered, again.expect("it classifies again"));
        assert_eq!(
            rendered,
            "root Cargo.lock derived=lockfile\n\
             root boxology.toml derived=\n\
             root c/bad/Cargo.lock derived=\n\
             root c/bad/boxology.toml derived=\n\
             root z/a.rs derived=\n\
             root z/b.rs derived=\n\
             alpha z/a\n\
             zulu z\n"
        );
    }
}
