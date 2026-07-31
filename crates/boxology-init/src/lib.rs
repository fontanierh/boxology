//! Pure assembly of the deterministic Boxology project tree.
//!
//! The initializer emits the root platform files and the embedded ping contract artifacts from
//! S2's pure generator. It performs no filesystem, runtime environment, network, clock, or process
//! access; the writer and later project packages are separate work.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::BoxId;
use boxology_generator::generate;
use boxology_generator_model::GenerationRequest;
use std::{
    fmt,
    path::{Component, Path, PathBuf},
};

const RULE_SOURCE: &str = "specs/s6-installer-and-generated-project.md D1";
const REQUEST_PATH: &str = "<request>";
const TOOLCHAIN: &[u8] = include_bytes!("../../../rust-toolchain.toml");
const PING_MANIFEST: &[u8] = include_bytes!("../../fixtures/ping/boxology.toml");
const PING_IMPLEMENTATION: &[u8] = include_bytes!("../../fixtures/ping/implementation/src/lib.rs");
const DIAGNOSTICS: [(&str, &str, &str); 4] = [
    (
        "BXI0001",
        "project name",
        "project name must match [a-z][a-z0-9-]*",
    ),
    (
        "BXI0002",
        "dependency source",
        "dependency source must not be empty",
    ),
    (
        "BXI0003",
        "embedded generation",
        "the pinned ping contract must generate without diagnostics",
    ),
    (
        "BXI0004",
        "generated path",
        "embedded generated paths must be confined relative paths",
    ),
];
const DEPENDENCIES: [(&str, &str); 4] = [
    ("boxology", "crates/boxology"),
    ("boxology-contract", "crates/boxology-contract"),
    ("boxology-http", "crates/boxology-http"),
    ("boxology-runtime", "crates/boxology-runtime"),
];

/// One stable coded initializer diagnostic.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Diagnostic(usize);

impl Diagnostic {
    /// Returns the stable initializer code.
    pub const fn code(&self) -> &'static str {
        DIAGNOSTICS[self.0].0
    }

    /// Returns the request diagnostic's source path.
    pub const fn path(&self) -> &'static str {
        REQUEST_PATH
    }

    /// Returns a static, payload-safe description of the offending construct.
    pub const fn offending_construct(&self) -> &'static str {
        DIAGNOSTICS[self.0].1
    }

    /// Returns the violated rule.
    pub const fn rule(&self) -> &'static str {
        DIAGNOSTICS[self.0].2
    }

    /// Returns the normative rule source.
    pub const fn rule_source(&self) -> &'static str {
        RULE_SOURCE
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}:1:1-1:1 offending={:?} rule={:?} source={:?}",
            self.code(),
            self.path(),
            self.offending_construct(),
            self.rule(),
            self.rule_source()
        )
    }
}

/// A nonempty, deterministically sorted initializer diagnostic collection.
#[derive(Debug, Eq, PartialEq)]
pub struct Diagnostics(Vec<Diagnostic>);

impl Diagnostics {
    /// Sorts diagnostics into report order, returning `None` for an empty collection.
    pub fn new(mut diagnostics: Vec<Diagnostic>) -> Option<Self> {
        diagnostics.sort();
        (!diagnostics.is_empty()).then_some(Self(diagnostics))
    }

    /// Returns the sorted diagnostics.
    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.0
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, diagnostic) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str("\n")?;
            }
            diagnostic.fmt(formatter)?;
        }
        Ok(())
    }
}

/// The smallest request needed by the root-platform initializer.
#[derive(Debug, Eq, PartialEq)]
pub struct InitRequest {
    project_name: String,
    dependency_source: String,
}

impl InitRequest {
    /// Validates and constructs an initializer request.
    pub fn new(
        project_name: impl Into<String>,
        dependency_source: impl Into<String>,
    ) -> Result<Self, Diagnostics> {
        let project_name = project_name.into();
        let dependency_source = dependency_source.into();
        let mut diagnostics = Vec::new();
        if !valid_project_name(&project_name) {
            diagnostics.push(Diagnostic(0));
        }
        if dependency_source.is_empty() {
            diagnostics.push(Diagnostic(1));
        }
        match Diagnostics::new(diagnostics) {
            Some(diagnostics) => Err(diagnostics),
            None => Ok(Self {
                project_name,
                dependency_source,
            }),
        }
    }

    /// Returns the validated project name.
    pub fn project_name(&self) -> &str {
        &self.project_name
    }

    /// Returns the dependency source with its exact caller-provided spelling.
    pub fn dependency_source(&self) -> &str {
        &self.dependency_source
    }
}

/// One generated project file.
#[derive(Debug, Eq, PartialEq)]
pub struct GeneratedFile {
    path: String,
    bytes: Vec<u8>,
}

impl GeneratedFile {
    /// Returns the generated file's sorted relative path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the generated file's exact bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A generated project tree sorted by relative-path bytes.
///
/// This slice uses literal-only output paths for the root platform files. Embedded generator
/// paths are confined before the `ping/` prefix is applied. A writer must pass every path through
/// [`confined_destination`] before joining it to its destination.
#[derive(Debug, Eq, PartialEq)]
pub struct GeneratedTree(Vec<GeneratedFile>);

impl GeneratedTree {
    /// Returns generated files in canonical relative-path order.
    pub fn files(&self) -> &[GeneratedFile] {
        &self.0
    }
}

/// Emits the deterministic project tree for a validated request.
pub fn initialize(request: &InitRequest) -> Result<GeneratedTree, Diagnostics> {
    let mut files = vec![
        file(".gitignore", b"/target\n".to_vec()),
        file(
            "Cargo.toml",
            cargo_manifest(request.dependency_source()).into_bytes(),
        ),
        file(
            "boxology-generator.toml",
            generator_manifest(request.dependency_source()).into_bytes(),
        ),
        file(
            "boxology.toml",
            platform_manifest(request.project_name()).into_bytes(),
        ),
        file("rust-toolchain.toml", TOOLCHAIN.to_vec()),
    ];
    files.extend(embed_ping()?);
    files.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(GeneratedTree(files))
}

/// Resolves a generated logical path beneath `root`.
///
/// Empty, absolute, rooted, prefixed, dot-segment, backslash, and NUL paths are rejected.
pub fn confined_destination(root: &Path, logical: &str) -> Result<PathBuf, &'static str> {
    confined_logical(logical).map(|logical| root.join(logical))
}

fn confined_logical(logical: &str) -> Result<&str, &'static str> {
    let bytes = logical.as_bytes();
    let portable_prefix = matches!(bytes, [letter, b':', ..] if letter.is_ascii_alphabetic());
    let invalid_segment = logical
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | ".."));
    let invalid_component = Path::new(logical)
        .components()
        .any(|part| !matches!(part, Component::Normal(_)));
    if bytes.contains(&0)
        || bytes.contains(&b'\\')
        || portable_prefix
        || invalid_segment
        || invalid_component
    {
        Err("generated path is not a confined relative path")
    } else {
        Ok(logical)
    }
}

fn embed_ping() -> Result<Vec<GeneratedFile>, Diagnostics> {
    embed(PING_MANIFEST, PING_IMPLEMENTATION)
}

fn embed(manifest: &[u8], implementation: &[u8]) -> Result<Vec<GeneratedFile>, Diagnostics> {
    let box_id = BoxId::new("ping").map_err(|_| embedded_generation())?;
    let request = GenerationRequest::new(
        box_id,
        "implementation/src/lib.rs".into(),
        vec![
            ("boxology.toml".into(), manifest.to_vec()),
            ("implementation/src/lib.rs".into(), implementation.to_vec()),
        ],
        vec![],
        boxology_generator::OUTPUTS
            .iter()
            .map(|path| (*path).to_owned())
            .collect(),
    )
    .map_err(|_| embedded_generation())?;
    let generated = generate(&request).map_err(|_| embedded_generation())?;
    prefixed(
        generated
            .files()
            .iter()
            .map(|file| (file.path(), file.bytes())),
    )
}

fn prefixed<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<Vec<GeneratedFile>, Diagnostics> {
    let mut out = Vec::new();
    for (path, bytes) in files {
        let logical = confined_logical(path).map_err(|_| path_not_confined())?;
        out.push(GeneratedFile {
            path: format!("ping/{logical}"),
            bytes: bytes.to_vec(),
        });
    }
    Ok(out)
}

fn embedded_generation() -> Diagnostics {
    Diagnostics::new(vec![Diagnostic(2)]).expect("embedded-generation diagnostic is nonempty")
}

fn path_not_confined() -> Diagnostics {
    Diagnostics::new(vec![Diagnostic(3)]).expect("path-confinement diagnostic is nonempty")
}

fn file(path: &str, bytes: Vec<u8>) -> GeneratedFile {
    GeneratedFile {
        path: path.into(),
        bytes,
    }
}

fn valid_project_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn toml_string(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\u{08}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{0c}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04X}", character as u32))
            }
            character => escaped.push(character),
        }
    }
    format!("\"{escaped}\"")
}

fn cargo_manifest(source: &str) -> String {
    let mut text = String::from(
        "[workspace]\nresolver = \"3\"\nmembers = []\n\n[workspace.package]\nedition = \"2024\"\n\n[workspace.dependencies]\n",
    );
    for (name, path) in DEPENDENCIES {
        text.push_str(&format!(
            "{name} = {{ version = \"=0.0.0\", path = {} }}\n",
            toml_string(&format!("{source}/{path}"))
        ));
    }
    text
}

fn generator_manifest(dependency_source: &str) -> String {
    format!(
        "boxology-version = {}\ndependency-source = {}\n",
        toml_string(env!("CARGO_PKG_VERSION")),
        toml_string(dependency_source)
    )
}

fn platform_manifest(project_name: &str) -> String {
    format!(
        "schema = 1\nid = {}\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"rust-toolchain.toml\", \"boxology.toml\", \"boxology-generator.toml\", \".gitignore\", \".github/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"**/Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        toml_string(project_name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_manifest::{Kind, Manifest, RelativePath};
    use std::fs;
    use std::path::Path;

    const SOURCE: &str = include_str!("lib.rs");
    const CODE_GOLDEN: &str = include_str!("../test/bxi.golden");
    const PROVENANCE_ANCHOR: &[u8] = b"  \"provenance\": ";
    const PROVENANCE_TOKEN: &[u8] = b"\"@PROVENANCE@\"";
    const GENERATOR_HEADER: &str = "// Generated by boxology-generator ";
    const STD_IMPORT: &str = "use std::{\n    fmt,\n    path::{Component, Path, PathBuf},\n};";
    #[rustfmt::skip]
    const GOLDEN: [(&str, &[u8]); 9] = [
        (".gitignore", include_bytes!("../../../goldens/generated-project/.gitignore")),
        ("Cargo.toml", include_bytes!("../../../goldens/generated-project/Cargo.toml")),
        (
            "boxology-generator.toml",
            include_bytes!("../../../goldens/generated-project/boxology-generator.toml"),
        ),
        ("boxology.toml", include_bytes!("../../../goldens/generated-project/boxology.toml")),
        (
            "ping/generated/adapter/adapter.rs",
            include_bytes!("../../../goldens/generated-project/ping/generated/adapter/adapter.rs"),
        ),
        (
            "ping/generated/contract/Cargo.toml",
            include_bytes!("../../../goldens/generated-project/ping/generated/contract/Cargo.toml"),
        ),
        (
            "ping/generated/contract/src/lib.rs",
            include_bytes!("../../../goldens/generated-project/ping/generated/contract/src/lib.rs"),
        ),
        (
            "ping/generated/schema.json",
            include_bytes!("../../../goldens/generated-project/ping/generated/schema.json"),
        ),
        ("rust-toolchain.toml", include_bytes!("../../../goldens/generated-project/rust-toolchain.toml")),
    ];
    const FIXTURE_GENERATED: [(&str, &[u8]); 4] = [
        (
            "generated/adapter/adapter.rs",
            include_bytes!("../../fixtures/ping/generated/adapter/adapter.rs"),
        ),
        (
            "generated/contract/Cargo.toml",
            include_bytes!("../../fixtures/ping/generated/contract/Cargo.toml"),
        ),
        (
            "generated/contract/src/lib.rs",
            include_bytes!("../../fixtures/ping/generated/contract/src/lib.rs"),
        ),
        (
            "generated/schema.json",
            include_bytes!("../../fixtures/ping/generated/schema.json"),
        ),
    ];

    fn source_codes(source: &str) -> Vec<&str> {
        source
            .match_indices("BXI")
            .filter_map(|(start, _)| {
                let code = source.get(start..start + 7)?;
                code[3..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit())
                    .then_some(code)
            })
            .collect()
    }

    fn modules(source: &str) -> Vec<&str> {
        source
            .lines()
            .map(str::trim_start)
            .filter(|line| {
                line.starts_with("mod ") || line.starts_with("pub mod ") || line.starts_with("pub(")
            })
            .collect()
    }

    fn golden_codes() -> Vec<&'static str> {
        CODE_GOLDEN.lines().map(|line| &line[..7]).collect()
    }

    fn globs(patterns: &[boxology_manifest::GlobPattern]) -> Vec<&str> {
        patterns.iter().map(|pattern| pattern.as_str()).collect()
    }

    fn request() -> InitRequest {
        InitRequest::new("example", "../boxology").expect("canonical request is valid")
    }

    fn generated<'a>(tree: &'a GeneratedTree, path: &str) -> &'a [u8] {
        tree.files()
            .iter()
            .find(|file| file.path() == path)
            .unwrap_or_else(|| panic!("missing generated file {path}"))
            .bytes()
    }

    fn occurrence_count(bytes: &[u8], needle: &[u8]) -> usize {
        bytes
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count()
    }

    fn normalize_rust(bytes: &[u8]) -> Result<Vec<u8>, String> {
        let text =
            std::str::from_utf8(bytes).map_err(|_| String::from("Rust output must be UTF-8"))?;
        if text.matches(GENERATOR_HEADER).count() != 1 {
            return Err(String::from(
                "Rust output must have exactly one generator header",
            ));
        }
        let (header, body) = text
            .split_once('\n')
            .ok_or_else(|| String::from("generated Rust has a header line"))?;
        if !header.starts_with(GENERATOR_HEADER) {
            return Err(String::from(
                "Rust header must start with the generator prefix",
            ));
        }
        Ok(format!("{GENERATOR_HEADER}@PROVENANCE@\n{body}").into_bytes())
    }

    fn normalize_live_schema(bytes: &[u8]) -> Result<Vec<u8>, String> {
        if occurrence_count(bytes, PROVENANCE_ANCHOR) != 1 {
            return Err("live schema must have exactly one provenance anchor".into());
        }
        let anchor = bytes
            .windows(PROVENANCE_ANCHOR.len())
            .position(|window| window == PROVENANCE_ANCHOR)
            .ok_or_else(|| "schema has one top-level provenance anchor".to_owned())?;
        let value_start = anchor + PROVENANCE_ANCHOR.len();
        if bytes.get(value_start) != Some(&b'{') {
            return Err("live provenance is an object".into());
        }

        let mut depth = 0;
        let mut in_string = false;
        let mut escaped = false;
        let mut value_end = None;
        for (offset, byte) in bytes[value_start..].iter().copied().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth -= 1;
                    if depth == 0 {
                        value_end = Some(value_start + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let value_end = value_end.ok_or_else(|| "live provenance object is complete".to_owned())?;
        let mut normalized =
            Vec::with_capacity(bytes.len() - (value_end - value_start) + PROVENANCE_TOKEN.len());
        normalized.extend_from_slice(&bytes[..value_start]);
        normalized.extend_from_slice(PROVENANCE_TOKEN);
        normalized.extend_from_slice(&bytes[value_end..]);
        Ok(normalized)
    }

    fn assert_checked_in_schema_provenance(bytes: &[u8]) -> Result<(), String> {
        if occurrence_count(bytes, PROVENANCE_ANCHOR) != 1 {
            return Err("golden schema must have exactly one provenance anchor".into());
        }
        if occurrence_count(bytes, PROVENANCE_TOKEN) != 1 {
            return Err("golden schema must carry exactly one provenance token".into());
        }
        let anchor = bytes
            .windows(PROVENANCE_ANCHOR.len())
            .position(|window| window == PROVENANCE_ANCHOR)
            .ok_or_else(|| "golden schema has one provenance anchor".to_owned())?;
        if !bytes[anchor + PROVENANCE_ANCHOR.len()..].starts_with(PROVENANCE_TOKEN) {
            return Err("golden provenance token must sit at the anchor".into());
        }
        Ok(())
    }

    fn compare_kind(path: &str, actual: &[u8], expected: &[u8]) -> Result<(), String> {
        match Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("rs") => {
                let actual = normalize_rust(actual)?;
                let expected = normalize_rust(expected)?;
                if actual != expected {
                    return Err(format!("generated bytes differ for {path}"));
                }
            }
            Some("json") => {
                assert_checked_in_schema_provenance(expected)?;
                let actual = normalize_live_schema(actual)?;
                if actual != expected {
                    return Err(format!("generated bytes differ for {path}"));
                }
            }
            _ => {
                if actual != expected {
                    return Err(format!("generated bytes differ for {path}"));
                }
            }
        }
        Ok(())
    }

    fn compare_golden(tree: &GeneratedTree) -> Result<(), String> {
        let actual: Vec<_> = tree.files().iter().map(GeneratedFile::path).collect();
        let expected: Vec<_> = GOLDEN.iter().map(|(path, _)| *path).collect();
        if actual
            .windows(2)
            .any(|pair| pair[0].as_bytes() >= pair[1].as_bytes())
        {
            return Err("generated paths are not unique and sorted".into());
        }
        if actual.len() != expected.len()
            || actual.iter().any(|path| !expected.contains(path))
            || expected.iter().any(|path| !actual.contains(path))
        {
            return Err(format!(
                "generated paths {actual:?} differ from golden {expected:?}"
            ));
        }
        for (path, expected_bytes) in GOLDEN {
            compare_kind(path, generated(tree, path), expected_bytes)?;
        }
        Ok(())
    }

    fn collect_golden_files(root: &Path) -> Vec<String> {
        fn visit(base: &Path, directory: &Path, found: &mut Vec<String>) {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    visit(base, &path, found);
                } else {
                    assert!(
                        file_type.is_file(),
                        "golden inventory entry must be a regular file: {}",
                        path.display()
                    );
                    found.push(
                        path.strip_prefix(base)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }

        let mut found = Vec::new();
        visit(root, root, &mut found);
        found.sort();
        found
    }

    #[test]
    fn canonical_request_emits_the_root_platform_subset() {
        let request = request();
        assert_eq!(request.project_name(), "example");
        assert_eq!(request.dependency_source(), "../boxology");
        let tree = initialize(&request).expect("canonical initialization succeeds");
        compare_golden(&tree).expect("the generated tree matches its golden");
    }

    #[test]
    fn root_manifest_parses_and_asserts_platform_ownership() {
        let tree = initialize(&request()).unwrap();
        let path = RelativePath::new("boxology.toml").unwrap();
        let manifest = Manifest::parse(path, generated(&tree, "boxology.toml"))
            .expect("emitted platform manifest parses");
        assert_eq!(manifest.id().as_str(), "example");
        assert_eq!(manifest.kind(), Kind::Platform);
        assert_eq!(
            globs(manifest.owned()),
            [
                "Cargo.toml",
                "rust-toolchain.toml",
                "boxology.toml",
                "boxology-generator.toml",
                ".gitignore",
                ".github/**",
            ]
        );
        let derived = manifest.derived();
        assert_eq!(derived.len(), 1);
        assert_eq!(derived[0].id().as_str(), "lockfile");
        assert_eq!(derived[0].generator(), "cargo");
        assert_eq!(globs(derived[0].inputs()), ["**/Cargo.toml"]);
        assert_eq!(globs(derived[0].outputs()), ["Cargo.lock"]);
    }

    #[test]
    fn workspace_dependencies_are_exact_and_pinned() {
        let tree = initialize(&request()).unwrap();
        let text = std::str::from_utf8(generated(&tree, "Cargo.toml")).unwrap();
        for (name, path) in DEPENDENCIES {
            let expected =
                format!("{name} = {{ version = \"=0.0.0\", path = \"../boxology/{path}\" }}");
            assert!(text.lines().any(|line| line == expected), "{expected}");
        }
    }

    #[test]
    fn noncanonical_dependency_source_is_escaped_in_both_manifests() {
        let source = r#"../boxology/"quoted"\checkout"#;
        let request = InitRequest::new("example", source).unwrap();
        let tree = initialize(&request).unwrap();

        assert_eq!(
            generated(&tree, "boxology-generator.toml"),
            b"boxology-version = \"0.0.0\"\ndependency-source = \"../boxology/\\\"quoted\\\"\\\\checkout\"\n"
        );
        let cargo = std::str::from_utf8(generated(&tree, "Cargo.toml")).unwrap();
        let expected = [
            r#"boxology = { version = "=0.0.0", path = "../boxology/\"quoted\"\\checkout/crates/boxology" }"#,
            r#"boxology-contract = { version = "=0.0.0", path = "../boxology/\"quoted\"\\checkout/crates/boxology-contract" }"#,
            r#"boxology-http = { version = "=0.0.0", path = "../boxology/\"quoted\"\\checkout/crates/boxology-http" }"#,
            r#"boxology-runtime = { version = "=0.0.0", path = "../boxology/\"quoted\"\\checkout/crates/boxology-runtime" }"#,
        ];
        for expected in expected {
            assert_eq!(
                cargo.lines().filter(|line| *line == expected).count(),
                1,
                "{expected}"
            );
        }
    }

    #[test]
    fn request_validation_catalog_is_exact_and_payload_safe() {
        let request_lines: Vec<_> = CODE_GOLDEN.lines().take(2).collect();
        for ((project, dependency), expected) in [("Bad.Project\n", "../boxology"), ("example", "")]
            .into_iter()
            .zip(request_lines)
        {
            let diagnostics = InitRequest::new(project, dependency).unwrap_err();
            assert_eq!(diagnostics.as_slice().len(), 1);
            let diagnostic = &diagnostics.as_slice()[0];
            assert_eq!(diagnostic.path(), "<request>");
            let rendered = diagnostics.to_string();
            assert_eq!(rendered, expected);
        }
        let catalog = [Diagnostic(0), Diagnostic(1), Diagnostic(2), Diagnostic(3)]
            .map(|diagnostic| diagnostic.to_string())
            .join("\n");
        assert_eq!(catalog, CODE_GOLDEN.trim_end());
        let both = InitRequest::new("Bad", "").unwrap_err();
        assert_eq!(
            both.as_slice()
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            ["BXI0001", "BXI0002"]
        );
        for valid in ["a", "example", "a0-b9", "box-"] {
            assert!(InitRequest::new(valid, "source").is_ok(), "{valid}");
        }
    }

    #[test]
    #[rustfmt::skip]
    fn production_inventory_and_code_catalog_fail_closed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = fs::read_dir(root).unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        files.sort();
        assert_eq!(files, ["lib.rs"]);
        files.push("stray.rs".into());
        assert_ne!(files, ["lib.rs"]);
        let production = production_source(SOURCE).expect("locked production/test boundary");
        let expected = golden_codes();
        assert_eq!(expected, ["BXI0001", "BXI0002", "BXI0003", "BXI0004"]);
        assert_eq!(DIAGNOSTICS.map(|entry| entry.0).as_slice(), expected);
        assert_eq!(source_codes(production), expected);
        assert_eq!(modules(SOURCE), ["mod tests {"]);
        assert_eq!(production.matches("include!(").count(), 0);
        assert_eq!(production.matches("fn prefixed").count(), 1);
        assert_eq!(production.matches("std::").count(), 1);
        assert!(production.contains(STD_IMPORT));
        let mut impure = production.replacen(
            STD_IMPORT,
            "use std::{\n    fmt,\n    fs,\n    path::{Component, Path, PathBuf},\n};",
            1,
        );
        assert!(!impure.contains(STD_IMPORT));
        impure.push_str("\nstd::env::var(\"PATH\");\n");
        assert_ne!(impure.matches("std::").count(), 1);
        let mut mutated = production.to_owned();
        mutated.push_str(&format!("\nconst STRAY: &str = \"{}{}\";\nmod stray;\n", "BX", "I9999"));
        assert_ne!(source_codes(&mutated), expected);
        assert_ne!(modules(&mutated), ["mod tests {"]);
        let cut = ["#[cfg(test)]", "\nmod tests {"].concat();
        let anchor = ["\n    // source inventory ", "ends here"].concat();
        assert_eq!(production_source(&format!("{SOURCE}{cut}")), Err("test cut must occur once"));
        assert_eq!(production_source(&format!("{SOURCE}{anchor}")), Err("test anchor must occur once"));
        for appended in ["/// documented production\npub fn appended() {}\n", "include!(\"post_test.rs\");\n"] {
            assert_eq!(production_source(&format!("{SOURCE}{appended}")), Err("test module must terminate source"));
        }
    }

    #[rustfmt::skip]
    fn production_source(source: &str) -> Result<&str, &'static str> {
        let cut = ["#[cfg(test)]", "\nmod tests {"].concat();
        let anchor = ["\n    // source inventory ", "ends here"].concat();
        if source.matches(&cut).count() != 1 { return Err("test cut must occur once"); }
        if source.matches(&anchor).count() != 1 { return Err("test anchor must occur once"); }
        let (production, tests) = source.split_once(&cut).unwrap();
        let (_, suffix) = tests.split_once(&anchor).unwrap();
        (suffix == "\n}\n").then_some(production).ok_or("test module must terminate source")
    }

    #[test]
    #[rustfmt::skip]
    fn generated_destinations_are_confined() {
        let root = Path::new("root");
        assert_eq!(confined_destination(root, "nested/file"), Ok(root.join("nested/file")));
        let invalid = ["", "/escape", "C:/escape", "C:\\escape", ".", "..", "a/./b", "a/../b", "a//b", "a/", "a\\b", "a\0b"];
        for path in invalid {
            assert_eq!(confined_destination(root, path), Err("generated path is not a confined relative path"), "{path:?}");
        }
    }

    #[test]
    #[rustfmt::skip]
    fn embedded_generated_paths_are_confined() {
        for path in boxology_generator::OUTPUTS {
            assert_eq!(confined_logical(path), Ok(path));
        }
        let invalid = ["", "/escape", "C:/escape", "a/../b", "a//b", "a/b/", "a\\b", "a\0b"];
        for path in invalid {
            assert_eq!(confined_logical(path), Err("generated path is not a confined relative path"), "{path:?}");
        }
    }

    #[test]
    fn path_prefixing_applies_confinement() {
        let diagnostics = prefixed([("../escape", b"x".as_slice())]).unwrap_err();
        assert_eq!(
            diagnostics
                .as_slice()
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            ["BXI0004"]
        );
    }

    #[test]
    fn empty_embedded_inputs_yield_bxi0003() {
        let diagnostics = embed(&[], &[]).unwrap_err();
        assert_eq!(
            diagnostics
                .as_slice()
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            ["BXI0003"]
        );
    }

    #[test]
    fn golden_inventory_and_comparison_fail_closed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../goldens/generated-project");
        let found = collect_golden_files(&root);
        let expected: Vec<String> = GOLDEN
            .iter()
            .map(|(path, _)| (*path).into())
            .collect::<Vec<_>>();
        assert_eq!(found, expected);

        for path in [
            ".gitignore",
            "ping/generated/adapter/adapter.rs",
            "ping/generated/schema.json",
        ] {
            let mut altered = initialize(&request()).unwrap();
            let file = altered
                .0
                .iter_mut()
                .find(|file| file.path() == path)
                .unwrap_or_else(|| panic!("missing generated file {path}"));
            file.bytes.push(b'x');
            assert!(
                compare_golden(&altered).is_err(),
                "compare_kind must reject a byte change under {path}"
            );
        }
    }

    #[test]
    fn provenance_normalization_fail_closed() {
        let header = format!("{GENERATOR_HEADER}0.0.0\nfn main() {{}}\n");
        assert!(normalize_rust(header.as_bytes()).is_ok());
        // Isolates the Rust header exactly-once count (not starts_with).
        assert_eq!(
            normalize_rust(format!("{header}{GENERATOR_HEADER}0.0.0\n").as_bytes()).unwrap_err(),
            "Rust output must have exactly one generator header"
        );
        assert_eq!(
            normalize_rust(b"fn main() {}\n").unwrap_err(),
            "Rust output must have exactly one generator header"
        );
        // Isolates the Rust header starts_with check (header contains the prefix, not at column 0).
        assert_eq!(
            normalize_rust(format!("kept {GENERATOR_HEADER}0.0.0\nfn main() {{}}\n").as_bytes())
                .unwrap_err(),
            "Rust header must start with the generator prefix"
        );

        let live = br#"{
  "box_id": "ping",
  "provenance": {"generator": "boxology-generator", "generator_version": "0.0.0", "semantic_digest": "sha256:dead"},
  "revision": "sha256:cafe"
}
"#;
        assert!(normalize_live_schema(live).is_ok());
        // Isolates the live provenance-anchor exactly-once count.
        let two_anchors = br#"{
  "provenance": {"generator": "boxology-generator"},
  "provenance": {"generator": "boxology-generator"}
}
"#;
        assert_eq!(
            normalize_live_schema(two_anchors).unwrap_err(),
            "live schema must have exactly one provenance anchor"
        );
        let zero_anchors = br#"{
  "box_id": "ping",
  "revision": "sha256:cafe"
}
"#;
        assert_eq!(
            normalize_live_schema(zero_anchors).unwrap_err(),
            "live schema must have exactly one provenance anchor"
        );
        // Isolates the live provenance-value object shape check.
        let not_object = br#"{
  "provenance": "@PROVENANCE@",
  "revision": "sha256:cafe"
}
"#;
        assert_eq!(
            normalize_live_schema(not_object).unwrap_err(),
            "live provenance is an object"
        );

        let golden = br#"{
  "provenance": "@PROVENANCE@",
  "revision": "sha256:cafe"
}
"#;
        assert!(assert_checked_in_schema_provenance(golden).is_ok());
        // Isolates the golden provenance-anchor exactly-once count.
        let golden_two_anchors = br#"{
  "provenance": "@PROVENANCE@",
  "provenance": "@PROVENANCE@"
}
"#;
        assert_eq!(
            assert_checked_in_schema_provenance(golden_two_anchors).unwrap_err(),
            "golden schema must have exactly one provenance anchor"
        );
        let golden_zero_anchors = br#"{
  "revision": "sha256:cafe"
}
"#;
        assert_eq!(
            assert_checked_in_schema_provenance(golden_zero_anchors).unwrap_err(),
            "golden schema must have exactly one provenance anchor"
        );
        // Isolates the golden provenance-token exactly-once count (not starts_with).
        let token_less = br#"{
  "provenance": {"generator": "boxology-generator"},
  "revision": "sha256:cafe"
}
"#;
        assert_eq!(
            assert_checked_in_schema_provenance(token_less).unwrap_err(),
            "golden schema must carry exactly one provenance token"
        );
        // Isolates the golden provenance-token-at-anchor starts_with check.
        let token_elsewhere = br#"{
  "other": "@PROVENANCE@",
  "provenance": {"generator": "boxology-generator"},
  "revision": "sha256:cafe"
}
"#;
        assert_eq!(
            assert_checked_in_schema_provenance(token_elsewhere).unwrap_err(),
            "golden provenance token must sit at the anchor"
        );
    }

    #[test]
    fn embedded_artifacts_match_fixture_corpus() {
        let tree = initialize(&request()).unwrap();
        for (relative, expected) in FIXTURE_GENERATED {
            let path = format!("ping/{relative}");
            let actual = generated(&tree, &path);
            compare_kind(&path, actual, expected).unwrap_or_else(|error| panic!("{path}: {error}"));
        }
    }

    // source inventory ends here
}
