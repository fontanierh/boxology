use boxology_cli::walk;
use boxology_manifest::RelativePath;
use boxology_workspace::FileEntry;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use syn::visit::Visit;

const NAMES: &str = "lib.rs walk.rs";
const LIB: &str = include_str!("../src/lib.rs");
const WALK: &str = include_str!("../src/walk.rs");
const SOURCES: &[(&str, &str)] = &[("lib.rs", LIB), ("walk.rs", WALK)];
const CODES: &str = "BXW0061 BXW0062 BXW0063";
const RULES: &str = "const ROOT: Rule = (\"BXW0061\", ROOT_TEXT, RULE_SOURCE);\nconst IO: Rule = (\"BXW0062\", IO_TEXT, RULE_SOURCE);\nconst PATH: Rule = (\"BXW0063\", PATH_TEXT, RULE_SOURCE);";
const DETAILS: &str = "const ROOT_TEXT: &str = \"workspace root must contain a regular Cargo.toml\";\nconst IO_TEXT: &str = \"filesystem refused a directory, symlink, or manifest read\";\nconst PATH_TEXT: &str = \"walked name/path is not a valid RelativePath\";";
const ANCHORS: &str = "symlink_metadata(&cargo).is_ok_and\nentry.file_name() == \".git\" || entry.file_name() == \"target\"\nlogical_path(root, &physical)?\nkind.is_symlink()\nfs::read_link(&physical)\nentry.file_name() == MANIFEST\nfs::read(&physical)\nfiles.sort_unstable_by\nmanifests.sort_unstable_by";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> Fixture {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("boxology-cli-surface-{}-{id}", std::process::id()));
    fs::create_dir(&root).expect("fixture root is new");
    Fixture(root)
}
fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).expect("fixture parent can be created");
    fs::write(path, bytes).expect("fixture file can be written");
}
fn path(name: &str) -> RelativePath {
    RelativePath::new(name).expect("test path is valid")
}
fn files(names: &[&str]) -> Vec<FileEntry> {
    names
        .iter()
        .map(|name| FileEntry::file(path(name)))
        .collect()
}
fn error_is(error: boxology_cli::WalkError, code: &str, at: &Path) {
    assert_eq!(error.code(), code);
    assert_eq!(error.path(), at);
    assert!(error.to_string().contains(code));
}

#[test]
fn root_gate_is_exact() {
    let fixture = fixture();
    let root = fixture.0.join("missing");
    error_is(
        walk(&root).expect_err("missing root must fail"),
        "BXW0061",
        &root.join("Cargo.toml"),
    );
}

#[test]
fn walk_is_opaque_sorted_and_exact() {
    let fixture = fixture();
    put(&fixture.0, "z.txt", b"z");
    put(&fixture.0, "nested/boxology.toml", b"nested");
    put(&fixture.0, "a/low.txt", b"low");
    put(&fixture.0, "a/boxology.toml", b"a");
    put(&fixture.0, "boxology.toml", b"root");
    put(&fixture.0, "docs/not-boxology.toml", b"near miss");
    put(&fixture.0, "boxology.toml.bak", b"near miss");
    put(&fixture.0, "Cargo.toml", b"cargo");
    put(&fixture.0, ".git/objects/ignored", b"git");
    put(&fixture.0, "target/debug/ignored", b"target");
    put(&fixture.0, "nested/target/ignored", b"nested target");
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        put(&fixture.0, "real/boxology.toml", b"real");
        put(&fixture.0, "real/hidden.txt", b"hidden");
        symlink("real", fixture.0.join("alias")).expect("symlink can be created");
    }

    let walked = walk(&fixture.0).expect("fixture is walkable");
    let mut expected = files(&[
        "Cargo.toml",
        "a/boxology.toml",
        "a/low.txt",
        "boxology.toml",
        "boxology.toml.bak",
        "docs/not-boxology.toml",
        "nested/boxology.toml",
        "z.txt",
    ]);
    #[cfg(unix)]
    {
        expected.insert(3, FileEntry::symlink(path("alias"), "real".to_owned()));
        expected.insert(8, FileEntry::file(path("real/boxology.toml")));
        expected.insert(9, FileEntry::file(path("real/hidden.txt")));
    }
    assert_eq!(walked.files(), expected.as_slice());
    let mut manifests = vec![
        (path("a/boxology.toml"), b"a".to_vec()),
        (path("boxology.toml"), b"root".to_vec()),
        (path("nested/boxology.toml"), b"nested".to_vec()),
    ];
    #[cfg(unix)]
    manifests.push((path("real/boxology.toml"), b"real".to_vec()));
    assert_eq!(walked.manifests(), manifests.as_slice());
}

#[cfg(unix)]
#[test]
fn refused_read_and_invalid_path_are_not_skipped() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture();
    put(&fixture.0, "Cargo.toml", b"cargo");
    let manifest = fixture.0.join("blocked/boxology.toml");
    put(&fixture.0, "blocked/boxology.toml", b"blocked");
    let mut permissions = fs::metadata(&manifest).unwrap().permissions();
    permissions.set_mode(0o0);
    fs::set_permissions(&manifest, permissions).unwrap();
    if fs::read(&manifest).is_err() {
        let error = walk(&fixture.0).expect_err("unreadable manifest must fail");
        error_is(error, "BXW0062", &manifest);
    }
    permissions = fs::metadata(&manifest).unwrap().permissions();
    permissions.set_mode(0o644);
    fs::set_permissions(&manifest, permissions).unwrap();

    let invalid = fixture.0.join("bad\nname");
    fs::write(&invalid, b"bad").unwrap();
    error_is(
        walk(&fixture.0).expect_err("invalid path must not be skipped"),
        "BXW0063",
        &invalid,
    );
}

#[test]
fn source_surface_is_exact_and_mutation_resistant() {
    assert!(locked());
    let cases = vec![
        format!("{LIB}\n#[cfg(test)] const HIDDEN: &str = \"BXW9999\";"),
        format!("{WALK}\nconst HIDDEN: &str = \"BXW9999\";"),
        format!("#[cfg(test)] mod tests {{}}\n{WALK}"),
        format!("{WALK}\n{}", ANCHORS.lines().next().unwrap()),
    ];
    for source in cases {
        assert!(!locked_sources(&[
            ("lib.rs", LIB),
            ("walk.rs", source.as_str())
        ]));
    }
    assert!(!locked_sources(&[
        ("lib.rs", LIB),
        ("walk.rs", WALK),
        ("extra.rs", "const HIDDEN: &str = \"BXW9999\";"),
    ]));
}

#[derive(Default)]
struct Lock {
    codes: Vec<String>,
    rules: usize,
    bad: bool,
    tests: usize,
    walks: usize,
}
impl<'ast> Visit<'ast> for Lock {
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        self.bad |= attr.path().is_ident("cfg") || attr.path().is_ident("cfg_attr");
        syn::visit::visit_attribute(self, attr);
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if matches!(item.ty.as_ref(), syn::Type::Path(path)
            if path.path.segments.last().is_some_and(|segment| segment.ident == "Rule"))
        {
            self.rules += 1;
        }
        syn::visit::visit_item_const(self, item);
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        self.bad = true;
        syn::visit::visit_item_mod(self, item);
    }
    fn visit_lit_byte_str(&mut self, _: &'ast syn::LitByteStr) {
        self.bad = true;
    }
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if literal.value().starts_with("BXW") {
            self.codes.push(literal.value());
        }
    }
    fn visit_macro(&mut self, called: &'ast syn::Macro) {
        self.bad |= !called.path.is_ident("write");
        syn::visit::visit_macro(self, called);
    }
    fn visit_use_glob(&mut self, _: &'ast syn::UseGlob) {
        self.bad = true;
    }
}

fn locked() -> bool {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut names = fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".rs"))
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
        .iter()
        .map(String::as_str)
        .eq(NAMES.split_whitespace())
        && SOURCES.iter().all(|(name, source)| {
            fs::read_to_string(directory.join(name)).is_ok_and(|current| current == *source)
        })
        && locked_sources(SOURCES)
}
fn locked_sources(sources: &[(&str, &str)]) -> bool {
    if !sources
        .iter()
        .map(|(name, _)| *name)
        .eq(NAMES.split_whitespace())
    {
        return false;
    }
    let mut lock = Lock::default();
    for (name, source) in sources {
        let Ok(file) = syn::parse_file(source) else {
            return false;
        };
        for attr in &file.attrs {
            lock.visit_attribute(attr);
        }
        for item in &file.items {
            if test_module(item) {
                lock.tests += 1;
            } else if *name == "lib.rs" && walk_module(item) {
                lock.walks += 1;
            } else {
                lock.visit_item(item);
            }
        }
    }
    let walk = sources[1].1;
    let codes = CODES
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lock.codes.sort_unstable();
    !lock.bad
        && lock.tests == 0
        && lock.walks == 1
        && lock.rules == 3
        && lock.codes == codes
        && RULES.lines().all(|rule| walk.matches(rule).count() == 1)
        && DETAILS
            .lines()
            .all(|detail| walk.matches(detail).count() == 1)
        && ANCHORS
            .lines()
            .all(|anchor| walk.matches(anchor).count() == 1)
        && sources[0].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[0].1.matches("#![forbid(unsafe_code)]").count() == 1
        && walk
            .matches("boxology-details/02-packages.md discovery walk")
            .count()
            == 1
        && walk.matches("S5-T4 #326 PR1 task authority").count() == 1
}
fn test_module(item: &syn::Item) -> bool {
    let syn::Item::Mod(module) = item else {
        return false;
    };
    module.ident == "tests"
        && module.content.is_some()
        && module.attrs.len() == 1
        && matches!(&module.attrs[0].meta, syn::Meta::List(meta)
            if meta.path.is_ident("cfg") && meta.tokens.to_string() == "test")
}
fn walk_module(item: &syn::Item) -> bool {
    matches!(item, syn::Item::Mod(module)
        if module.ident == "walk" && module.attrs.is_empty() && module.content.is_none())
}
