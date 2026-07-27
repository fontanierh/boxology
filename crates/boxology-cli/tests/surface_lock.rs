use boxology_cli::walk;
use boxology_manifest::RelativePath;
use boxology_workspace::FileEntry;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use syn::visit::Visit;

const NAMES: &str = "lib.rs walk.rs";
const LIB: &str = include_str!("../src/lib.rs");
const WALK: &str = include_str!("../src/walk.rs");
const SOURCES: &[(&str, &str)] = &[("lib.rs", LIB), ("walk.rs", WALK)];
const GOLDEN: &str = include_str!("bxw.golden");
const CODES: &str = "BXW0061 BXW0062 BXW0063";
const ANCHORS: &str = "symlink_metadata(root).is_ok_and\nsymlink_metadata(&cargo).is_ok_and\nentry.file_name() == \".git\" || entry.file_name() == \"target\"\nlogical_path(root, &physical)?\nkind.is_symlink()\nfs::read_link(&physical)\nentry.file_name() == MANIFEST\nread_manifest(&physical, |path| fs::read(path))?\nfiles.sort_unstable_by\nmanifests.sort_unstable_by";
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
fn error_is(error: boxology_cli::WalkError, code: &str, at: &Path, detail: &str) {
    assert_eq!(error.code(), code);
    assert_eq!(error.path(), at);
    assert_eq!(error.detail(), detail);
    assert_eq!(error.to_string(), format!("{code} {at:?}: {detail}"));
}

#[test]
fn root_gate_is_exact() {
    let fixture = fixture();
    let root = fixture.0.join("missing");
    error_is(
        walk(&root).expect_err("missing root must fail"),
        "BXW0061",
        &root,
        "workspace root must be a real directory containing a regular Cargo.toml",
    );
    let empty = fixture.0.join("empty");
    fs::create_dir(&empty).unwrap();
    error_is(
        walk(&empty).expect_err("root manifest is required"),
        "BXW0061",
        &empty.join("Cargo.toml"),
        "workspace root must be a real directory containing a regular Cargo.toml",
    );
}

#[cfg(unix)]
#[test]
fn symlink_root_is_rejected_before_external_ingestion() {
    use std::os::unix::fs::symlink;

    let fixture = fixture();
    let external = fixture.0.join("external");
    put(&external, "Cargo.toml", b"cargo");
    put(&external, "boxology.toml", b"must not be ingested");
    let root = fixture.0.join("root-link");
    symlink(&external, &root).expect("root symlink can be created");
    error_is(
        walk(&root).expect_err("symlink root must fail before traversal"),
        "BXW0061",
        &root,
        "workspace root must be a real directory containing a regular Cargo.toml",
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
    put(&fixture.0, "nested/.git/ignored", b"nested git");
    put(&fixture.0, "nested/.git/boxology.toml", b"excluded git");
    put(&fixture.0, "nested/target/ignored", b"nested target");
    put(
        &fixture.0,
        "nested/target/boxology.toml",
        b"excluded target",
    );
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
fn invalid_path_is_not_skipped_on_unix() {
    let fixture = fixture();
    put(&fixture.0, "Cargo.toml", b"cargo");
    let invalid = fixture.0.join("bad\nname");
    fs::write(&invalid, b"bad").unwrap();
    error_is(
        walk(&fixture.0).expect_err("invalid path must not be skipped"),
        "BXW0063",
        &invalid,
        "walked name/path is not a valid RelativePath",
    );
}

#[test]
fn source_surface_is_exact_and_mutation_resistant() {
    assert!(locked());
    let cases = vec![
        format!("{LIB}\n#[cfg(test)] const STRAY: u8 = 0;"),
        format!("{WALK}\nconst AFTER_TESTS: u8 = 0;"),
        WALK.replace("\"BXW0062\"", "\"BXC9999\""),
        WALK.replace(
            "read_manifest(&physical, |path| fs::read(path))?",
            "Vec::new() /* unreachable IO */",
        ),
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
        ("extra.rs", "const EXTRA: u8 = 0;"),
    ]));
}

#[derive(Default)]
struct Lock {
    codes: Vec<String>,
    constants: BTreeMap<String, String>,
    rules: Vec<(String, String, String)>,
    items: Vec<String>,
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
        if let syn::Expr::Lit(expression) = item.expr.as_ref()
            && let syn::Lit::Str(value) = &expression.lit
        {
            self.constants.insert(item.ident.to_string(), value.value());
        }
        if type_ident(item.ty.as_ref()).as_deref() == Some("Rule")
            && let syn::Expr::Tuple(tuple) = item.expr.as_ref()
            && let [code, text, source] = tuple.elems.iter().collect::<Vec<_>>().as_slice()
            && let (Some(code), Some(text), Some(source)) = (
                literal(code),
                expression_ident(text),
                expression_ident(source),
            )
        {
            self.rules.push((code, text, source));
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
        if diagnostic(&literal.value()) {
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
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.items.push(format!("method:{}", item.sig.ident));
        syn::visit::visit_impl_item_fn(self, item);
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
        if !lock.items.is_empty() {
            lock.items.push("|".to_owned());
        }
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
                lock.items.push("mod:walk".to_owned());
            } else {
                let Some(key) = item_key(item) else {
                    return false;
                };
                lock.items.push(key);
                lock.visit_item(item);
            }
        }
    }
    let codes = CODES
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lock.codes.sort_unstable();
    lock.rules.sort_unstable();
    !lock.bad
        && lock.tests == 1
        && lock.walks == 1
        && lock.codes == codes
        && render(&lock).is_some_and(|golden| golden == GOLDEN)
        && ANCHORS
            .lines()
            .all(|anchor| sources[1].1.matches(anchor).count() == 1)
        && sources[0].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[0].1.matches("#![forbid(unsafe_code)]").count() == 1
}
fn render(lock: &Lock) -> Option<String> {
    let mut output = format!(
        "sources={}\nitems={}\n",
        NAMES.replace(' ', ","),
        lock.items.join(",").replace(",|,", ";")
    );
    for (code, text, source) in &lock.rules {
        output.push_str(&format!(
            "{code}|{}|{}\n",
            lock.constants.get(text)?,
            lock.constants.get(source)?
        ));
    }
    Some(output)
}
fn item_key(item: &syn::Item) -> Option<String> {
    Some(match item {
        syn::Item::Use(item) => format!("use:{}", visibility(&item.vis)),
        syn::Item::Type(item) => format!("type:{}", item.ident),
        syn::Item::Const(item) => format!("const:{}", item.ident),
        syn::Item::Struct(item) => format!("struct:{}", item.ident),
        syn::Item::Impl(item) => {
            let own = type_ident(&item.self_ty)?;
            item.trait_.as_ref().map_or_else(
                || format!("impl:{own}"),
                |(path, _)| format!("impl:{}:{own}", path.segments.last().unwrap().ident),
            )
        }
        syn::Item::Fn(item) => format!("fn:{}", item.sig.ident),
        _ => return None,
    })
}
fn visibility(visibility: &syn::Visibility) -> &'static str {
    if matches!(visibility, syn::Visibility::Public(_)) {
        "pub"
    } else {
        "private"
    }
}
fn type_ident(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}
fn expression_ident(expression: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(path) = expression else {
        return None;
    };
    Some(path.path.segments.last()?.ident.to_string())
}
fn literal(expression: &syn::Expr) -> Option<String> {
    let syn::Expr::Lit(expression) = expression else {
        return None;
    };
    let syn::Lit::Str(value) = &expression.lit else {
        return None;
    };
    Some(value.value())
}
fn diagnostic(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7
        && bytes[0..2] == *b"BX"
        && bytes[2].is_ascii_uppercase()
        && bytes[3..].iter().all(u8::is_ascii_digit)
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
