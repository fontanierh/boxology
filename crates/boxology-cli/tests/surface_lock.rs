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
const NAMES: &str = "lib.rs walk.rs generate.rs execute.rs classify.rs main.rs";
const FILES: &str = "Cargo.toml src/classify.rs src/execute.rs src/generate.rs src/lib.rs src/main.rs src/walk.rs tests/bxw.golden tests/classify.rs tests/cli.rs tests/execute.rs tests/generation_plan.rs tests/surface_lock.rs";
const PACKAGE: &str = include_str!("../Cargo.toml");
const PACKAGE_HASH: u64 = 4_400_377_577_107_281_090;
const LIB: &str = include_str!("../src/lib.rs");
const WALK: &str = include_str!("../src/walk.rs");
const GENERATE: &str = include_str!("../src/generate.rs");
const EXECUTE: &str = include_str!("../src/execute.rs");
const CLASSIFY: &str = include_str!("../src/classify.rs");
const MAIN: &str = include_str!("../src/main.rs");
const SOURCES: &[(&str, &str)] = &[
    ("lib.rs", LIB),
    ("walk.rs", WALK),
    ("generate.rs", GENERATE),
    ("execute.rs", EXECUTE),
    ("classify.rs", CLASSIFY),
    ("main.rs", MAIN),
];
const GOLDEN: &str = include_str!("bxw.golden");
const CODES: &str = "BXW0061 BXW0062 BXW0063 BXW0064 BXW0065 BXW0066 BXW0067 BXW0068 BXW0069 BXW0070 BXW0071 BXW0072 BXW0073 BXW0075 BXW0076 BXW0077 BXW0078 BXW0079";
const LIB_HASH: u64 = 7_206_800_631_454_075_744;
const WALK_HASH: u64 = 12_408_747_065_446_683_334;
const GENERATE_HASH: u64 = 2_437_200_502_410_785_768;
const EXECUTE_HASH: u64 = 7_195_906_979_600_889_935;
const CLASSIFY_HASH: u64 = 11_819_106_377_568_339_839;
const MAIN_ANCHORS: &str = "env::args_os()\ncollect::<Result<Vec<_>, _>>()\ncargo_metadata_command(root)\nstatus.success()\nString::from_utf8(stdout)\nWorkspaceInputs::new\ninputs.check()\nplan(&workspace, selection.as_ref())\nexecute(root, generation)\nBXW0075\nif error.is_unknown_package() { 2 } else { 1 }\n_ => Err(())";
const ARGV_SHAPE: &str = "pub const CARGO_METADATA_ARGS: [&str; 5] =\n    [\"metadata\", \"--format-version\", \"1\", \"--locked\", \"--no-deps\"];";
const MAIN_HASH: u64 = 2_327_141_899_214_887_966;
const HASHES: [u64; 6] = [
    LIB_HASH,
    WALK_HASH,
    GENERATE_HASH,
    EXECUTE_HASH,
    CLASSIFY_HASH,
    MAIN_HASH,
];
const ANCHORS: &str = "symlink_metadata(root).is_ok_and\nsymlink_metadata(&cargo).is_ok_and\nentry.file_name() == \".git\"\nentry.file_name() == \"target\"\nlogical_path(root, &physical)?\nkind.is_symlink()\nfs::read_link(&physical)\nentry.file_name() == MANIFEST\nread_manifest(&physical, |path| fs::read(path))?\nfiles.sort_unstable_by\nmanifests.sort_unstable_by";
const GENERATE_ANCHORS: &str = "output.generator() == CARGO_GENERATOR\noutput.generator() == CONTRACT_GENERATOR\nclassification.package() == package.id()\nclassification.derived_output().is_none()\nentry.role() == CrateRole::BoxImplementation\npackage.relative(classification.path())?";
const EXECUTE_ANCHORS: &str = "fs::symlink_metadata(&path)\npattern.matches(&output)\nOUTPUTS.iter().map(|path| (*path).to_owned()).collect()\nboxology_generator_writer::write(&package_dir, &tree, plan.outputs())\nconst SCHEMA: &str = \"generated/schema.json\";\nfile.path() == SCHEMA";
const EXECUTE_PUBLIC: &str = "Outcome written removed is_unchanged base_schema submitted_schema ExecuteError code location path detail diagnostics write_error execute";
const CLASSIFY_ANCHORS: &str = "map_err(ClassifyError::base)\nmap_err(ClassifyError::submitted)\nmap_err(ClassifyError::pairing)\nboxology_classifier::classify(base.as_ref(), Some(&submitted))";
const CLASSIFY_PUBLIC: &str = "ClassifyError code side detail diagnostics classify render";
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
    let root = fixture.0.join("cargo-link-root");
    fs::create_dir(&root).unwrap();
    symlink(external.join("Cargo.toml"), root.join("Cargo.toml")).unwrap();
    error_is(
        walk(&root).expect_err("symlink root manifest must fail"),
        "BXW0061",
        &root.join("Cargo.toml"),
        "workspace root must be a real directory containing a regular Cargo.toml",
    );
}
#[test]
fn walk_is_opaque_sorted_and_exact() {
    let fixture = fixture();
    put(&fixture.0, "Zed.txt", b"zed");
    put(&fixture.0, "nested/boxology.toml", b"nested");
    put(&fixture.0, "apple.txt", b"apple");
    put(&fixture.0, "a/boxology.toml", b"a");
    put(&fixture.0, "boxology.toml", b"root");
    put(&fixture.0, "docs/not-boxology.toml", b"near miss");
    put(&fixture.0, "boxology.toml.bak", b"near miss");
    put(&fixture.0, "Cargo.toml", b"cargo");
    put(&fixture.0, ".git", b"gitdir: ../linked-worktree");
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
        "Zed.txt",
        "a/boxology.toml",
        "apple.txt",
        "boxology.toml",
        "boxology.toml.bak",
        "docs/not-boxology.toml",
        "nested/boxology.toml",
    ]);
    #[cfg(unix)]
    {
        expected.insert(3, FileEntry::symlink(path("alias"), "real".to_owned()));
        expected.insert(9, FileEntry::file(path("real/boxology.toml")));
        expected.insert(10, FileEntry::file(path("real/hidden.txt")));
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
    let (production, module) = WALK.split_once("\n#[cfg(test)]").unwrap();
    let cases = vec![
        format!("{WALK}\nconst AFTER_TESTS: u8 = 0;"),
        format!("{production}\nconst BAD: &str = \"BXC9999\";\n#[cfg(test)]{module}"),
        WALK.replace("PR1 task authority", "mutant authority"),
        WALK.replace(
            "read_manifest(&physical, |path| fs::read(path))?",
            "Vec::new() /* unreachable IO */",
        ),
        format!("{production}\n#[cfg(test)]\nmod tests {{}}\n"),
        format!("#[cfg(test)]{module}\n{production}"),
        format!("{WALK}\n// {}", ANCHORS.lines().next().unwrap()),
    ];
    for source in cases {
        rejects(
            LIB,
            &source,
            GENERATE,
            EXECUTE,
            CLASSIFY,
            MAIN,
            [
                LIB_HASH,
                hash(&source),
                GENERATE_HASH,
                EXECUTE_HASH,
                CLASSIFY_HASH,
                hash(MAIN),
            ],
        );
    }
    let mutant = format!("{LIB}// hash mutant\n");
    rejects(&mutant, WALK, GENERATE, EXECUTE, CLASSIFY, MAIN, HASHES);
    let mutant = format!("{WALK}// hash mutant\n");
    rejects(LIB, &mutant, GENERATE, EXECUTE, CLASSIFY, MAIN, HASHES);
    let mutant = format!("{GENERATE}// hash mutant\n");
    rejects(LIB, WALK, &mutant, EXECUTE, CLASSIFY, MAIN, HASHES);
    let mutant = format!("{EXECUTE}\n/// Mutant.\npub fn mutant() {{}}\n");
    let hashes = [
        LIB_HASH,
        WALK_HASH,
        GENERATE_HASH,
        hash(&mutant),
        CLASSIFY_HASH,
        hash(MAIN),
    ];
    rejects(LIB, WALK, GENERATE, &mutant, CLASSIFY, MAIN, hashes);
    let mutant = EXECUTE.replace("BXW0070", "BXW9999");
    rejects(
        LIB,
        WALK,
        GENERATE,
        &mutant,
        CLASSIFY,
        MAIN,
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            hash(&mutant),
            CLASSIFY_HASH,
            hash(MAIN),
        ],
    );
    for (anchor, replacement) in [
        ("fs::symlink_metadata(&path)", "fs::metadata(&path)"),
        (
            "pattern.matches(&output)",
            "pattern.as_str() == output.as_str()",
        ),
        (
            "OUTPUTS.iter().map(|path| (*path).to_owned()).collect()",
            "Vec::new()",
        ),
        (
            "boxology_generator_writer::write(&package_dir, &tree, plan.outputs())",
            "boxology_generator_writer::write(root, &tree, plan.outputs())",
        ),
        (
            "const SCHEMA: &str = \"generated/schema.json\";",
            "const SCHEMA: &str = OUTPUTS[3];",
        ),
        ("file.path() == SCHEMA", "file.path() == OUTPUTS[3]"),
    ] {
        let changed = EXECUTE.replace(anchor, replacement);
        rejects(
            LIB,
            WALK,
            GENERATE,
            &changed,
            CLASSIFY,
            MAIN,
            [
                LIB_HASH,
                WALK_HASH,
                GENERATE_HASH,
                hash(&changed),
                CLASSIFY_HASH,
                hash(MAIN),
            ],
        );
    }
    for (anchor, replacement) in [
        (
            "output.generator() == CARGO_GENERATOR",
            "output.generator() == CONTRACT_GENERATOR",
        ),
        (
            "classification.package() == package.id()",
            "classification.package() != package.id()",
        ),
        (
            "classification.derived_output().is_none()",
            "classification.derived_output().is_some()",
        ),
    ] {
        let changed = GENERATE.replace(anchor, replacement);
        rejects(LIB, WALK, &changed, EXECUTE, CLASSIFY, MAIN, HASHES);
    }
    for (needle, replacement) in [
        ("\"--locked\", ", ""),
        ("\"--no-deps\"", "\"--no-deps\", \"--offline\""),
    ] {
        let changed = LIB.replace(needle, replacement);
        rejects(
            &changed,
            WALK,
            GENERATE,
            EXECUTE,
            CLASSIFY,
            MAIN,
            [
                hash(&changed),
                WALK_HASH,
                GENERATE_HASH,
                EXECUTE_HASH,
                CLASSIFY_HASH,
                MAIN_HASH,
            ],
        );
    }
    let duplicate = format!("{MAIN}\nconst DUPLICATE: &str = \"BXW0075\";\n");
    rejects(
        LIB,
        WALK,
        GENERATE,
        EXECUTE,
        CLASSIFY,
        &duplicate,
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            EXECUTE_HASH,
            CLASSIFY_HASH,
            hash(&duplicate),
        ],
    );
    let reworded = MAIN.replace(
        "cargo metadata could not be executed or did not return valid workspace metadata",
        "metadata failed",
    );
    rejects(
        LIB,
        WALK,
        GENERATE,
        EXECUTE,
        CLASSIFY,
        &reworded,
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            EXECUTE_HASH,
            CLASSIFY_HASH,
            hash(&reworded),
        ],
    );
    let second_exit = MAIN.replace(
        "if error.is_unknown_package() { 2 } else { 1 }",
        "if error.is_unknown_package() { 2 } else { 2 }",
    );
    rejects(
        LIB,
        WALK,
        GENERATE,
        EXECUTE,
        CLASSIFY,
        &second_exit,
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            EXECUTE_HASH,
            CLASSIFY_HASH,
            hash(&second_exit),
        ],
    );
    let fallthrough = MAIN.replace("_ => Err(())", "_ => Ok(None)");
    rejects(
        LIB,
        WALK,
        GENERATE,
        EXECUTE,
        CLASSIFY,
        &fallthrough,
        [
            LIB_HASH,
            WALK_HASH,
            GENERATE_HASH,
            EXECUTE_HASH,
            CLASSIFY_HASH,
            hash(&fallthrough),
        ],
    );
    let extra = [
        ("lib.rs", LIB),
        ("walk.rs", WALK),
        ("generate.rs", GENERATE),
        ("execute.rs", EXECUTE),
        ("classify.rs", CLASSIFY),
        ("main.rs", MAIN),
        ("extra.rs", ""),
    ];
    assert!(!locked_sources(&extra, HASHES, GOLDEN));
    let golden_mutant = format!("{GOLDEN}mutant");
    assert!(!locked_sources(SOURCES, HASHES, &golden_mutant));
    let files: Vec<_> = FILES.split_whitespace().map(str::to_owned).collect();
    for extra in ["build.rs", "src/bin/hidden.rs"] {
        let mut mutant = files.clone();
        mutant.push(extra.to_owned());
        mutant.sort_unstable();
        assert!(!package_is(PACKAGE, &mutant));
    }
    for mutant in [
        format!("{PACKAGE}\n[lib]\npath = \"src/walk.rs\"\n"),
        format!("{PACKAGE}\n[[example]]\nname = \"escape\"\npath = \"src/lib.rs\"\n"),
    ] {
        assert!(!package_is(&mutant, &files));
    }
}
fn rejects(
    lib: &str,
    walk: &str,
    generate: &str,
    execute: &str,
    classify: &str,
    main: &str,
    hashes: [u64; 6],
) {
    let sources = [
        ("lib.rs", lib),
        ("walk.rs", walk),
        ("generate.rs", generate),
        ("execute.rs", execute),
        ("classify.rs", classify),
        ("main.rs", main),
    ];
    assert!(!locked_sources(&sources, hashes, GOLDEN));
}
#[derive(Default)]
struct Lock {
    codes: Vec<String>,
    constants: BTreeMap<String, String>,
    rules: Vec<(String, String, String)>,
    public: Vec<String>,
    next_public: bool,
    bad: bool,
    tests: usize,
}
impl<'ast> Visit<'ast> for Lock {
    fn visit_visibility(&mut self, visibility: &'ast syn::Visibility) {
        self.next_public = matches!(visibility, syn::Visibility::Public(_));
    }
    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        if std::mem::take(&mut self.next_public) {
            self.public.push(ident.to_string());
        }
    }
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
        self.bad |= !called.path.is_ident("write")
            && !called.path.is_ident("writeln")
            && !called.path.is_ident("format");
        syn::visit::visit_macro(self, called);
    }
    fn visit_use_glob(&mut self, _: &'ast syn::UseGlob) {
        self.bad = true;
    }
}
fn locked() -> bool {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    let inventory = package_files(root, root, &mut files);
    files.sort_unstable();
    inventory
        && package_is(PACKAGE, &files)
        && fs::read_to_string(root.join("Cargo.toml")).is_ok_and(|text| text == PACKAGE)
        && SOURCES.iter().all(|(name, source)| {
            fs::read_to_string(root.join("src").join(name)).is_ok_and(|text| text == *source)
        })
        && locked_sources(SOURCES, HASHES, GOLDEN)
}
fn package_files(root: &Path, directory: &Path, files: &mut Vec<String>) -> bool {
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            return false;
        };
        if kind.is_dir() {
            if !package_files(root, &path, files) {
                return false;
            }
        } else if kind.is_file() {
            let Some(name) = path.strip_prefix(root).ok().and_then(Path::to_str) else {
                return false;
            };
            files.push(name.replace(std::path::MAIN_SEPARATOR, "/"));
        } else {
            return false;
        }
    }
    true
}
fn package_is(manifest: &str, files: &[String]) -> bool {
    hash(manifest) == PACKAGE_HASH && files.join(" ") == FILES
}
fn locked_sources(sources: &[(&str, &str)], hashes: [u64; 6], golden: &str) -> bool {
    if !sources
        .iter()
        .map(|(name, _)| *name)
        .eq(NAMES.split_whitespace())
    {
        return false;
    }
    if sources.len() != hashes.len()
        || sources
            .iter()
            .zip(hashes)
            .any(|((_, source), expected)| hash(source) != expected)
    {
        return false;
    }
    let mut lock = Lock::default();
    let mut execute_public = Vec::new();
    let mut classify_public = Vec::new();
    for &(name, source) in sources.iter().skip(1) {
        lock.public.clear();
        let Ok(file) = syn::parse_file(source) else {
            return false;
        };
        for attr in &file.attrs {
            lock.visit_attribute(attr);
        }
        for (position, item) in file.items.iter().enumerate() {
            if name == "walk.rs" && test_module(item) {
                lock.tests += usize::from(position + 1 == file.items.len());
            } else {
                lock.visit_item(item);
            }
        }
        if name == "execute.rs" {
            execute_public = lock.public.clone();
        }
        if name == "classify.rs" {
            classify_public = lock.public.clone();
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
        && lock.codes == codes
        && execute_public.join(" ") == EXECUTE_PUBLIC
        && classify_public.join(" ") == CLASSIFY_PUBLIC
        && render(&lock).is_some_and(|rendered| rendered == golden)
        && ANCHORS
            .lines()
            .all(|anchor| sources[1].1.matches(anchor).count() == 1)
        && GENERATE_ANCHORS
            .lines()
            .all(|anchor| sources[2].1.matches(anchor).count() == 1)
        && EXECUTE_ANCHORS
            .lines()
            .all(|anchor| sources[3].1.matches(anchor).count() == 1)
        && CLASSIFY_ANCHORS
            .lines()
            .all(|anchor| sources[4].1.matches(anchor).count() == 1)
        && MAIN_ANCHORS
            .lines()
            .all(|anchor| sources[5].1.matches(anchor).count() == 1)
        && sources[0].1.matches(ARGV_SHAPE).count() == 1
        && sources[0].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[0].1.matches("#![forbid(unsafe_code)]").count() == 1
        && sources[3].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[3].1.matches("#![forbid(unsafe_code)]").count() == 1
        && sources[4].1.matches("#![deny(missing_docs)]").count() == 1
        && sources[4].1.matches("#![forbid(unsafe_code)]").count() == 1
}
fn render(lock: &Lock) -> Option<String> {
    let mut output = format!("sources={}\n", NAMES.replace(' ', ","));
    for (code, text, source) in &lock.rules {
        output.push_str(&format!(
            "{code}|{}|{}\n",
            lock.constants.get(text)?,
            lock.constants.get(source)?
        ));
    }
    Some(output)
}
fn type_ident(ty: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    Some(path.path.segments.last()?.ident.to_string())
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
        && bytes.starts_with(b"BX")
        && bytes[2].is_ascii_uppercase()
        && bytes[3..].iter().all(u8::is_ascii_digit)
}
fn hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
fn test_module(item: &syn::Item) -> bool {
    let syn::Item::Mod(module) = item else {
        return false;
    };
    let Some((_, items)) = &module.content else {
        return false;
    };
    let [syn::Item::Use(_), syn::Item::Use(_), syn::Item::Fn(test)] = items.as_slice() else {
        return false;
    };
    module.ident == "tests"
        && module.attrs.len() == 1
        && matches!(&module.attrs[0].meta, syn::Meta::List(meta)
            if meta.path.is_ident("cfg") && meta.tokens.to_string() == "test")
        && test.sig.ident == "refused_manifest_read_is_stable_and_payload_safe"
        && test.attrs.len() == 1
        && test.attrs[0].path().is_ident("test")
}
