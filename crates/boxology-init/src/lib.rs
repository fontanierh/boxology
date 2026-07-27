//! Pure assembly of the deterministic Boxology project tree.
//!
//! This first slice emits only the root platform files. It performs no filesystem, environment,
//! network, clock, or process access; the writer and the later project packages are separate work.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::fmt;

const RULE_SOURCE: &str = "specs/s6-installer-and-generated-project.md D1";
const REQUEST_PATH: &str = "<request>";
const PROVENANCE: &str = env!("CARGO_PKG_VERSION");
const TOOLCHAIN: &[u8] = include_bytes!("../../../rust-toolchain.toml");
const DIAGNOSTICS: [(&str, &str, &str); 2] = [
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
#[derive(Debug, Eq, PartialEq)]
pub struct GeneratedTree(Vec<GeneratedFile>);

impl GeneratedTree {
    /// Returns generated files in canonical relative-path order.
    pub fn files(&self) -> &[GeneratedFile] {
        &self.0
    }
}

/// Emits the deterministic root-platform subset for a validated request.
pub fn initialize(request: &InitRequest) -> Result<GeneratedTree, Diagnostics> {
    let mut files = vec![
        file(".gitignore", b"/target\n".to_vec()),
        file(
            "Cargo.toml",
            cargo_manifest(request.dependency_source()).into_bytes(),
        ),
        file(
            "boxology-generator.toml",
            generator_config(request.dependency_source()).into_bytes(),
        ),
        file(
            "boxology.toml",
            platform_manifest(request.project_name()).into_bytes(),
        ),
        file("rust-toolchain.toml", TOOLCHAIN.to_vec()),
    ];
    files.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(GeneratedTree(files))
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

fn platform_manifest(project_name: &str) -> String {
    format!(
        "schema = 1\nid = {}\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"rust-toolchain.toml\", \"boxology.toml\", \"boxology-generator.toml\", \".gitignore\", \".github/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"**/Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        toml_string(project_name)
    )
}

fn generator_config(source: &str) -> String {
    format!(
        "schema = 1\nboxology_version = {}\ndependency_source = {}\n",
        toml_string(PROVENANCE),
        toml_string(source)
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
    const GOLDEN: [(&str, &[u8]); 5] = [
        (
            ".gitignore",
            include_bytes!("../../../goldens/generated-project/.gitignore"),
        ),
        (
            "Cargo.toml",
            include_bytes!("../../../goldens/generated-project/Cargo.toml"),
        ),
        (
            "boxology-generator.toml",
            include_bytes!("../../../goldens/generated-project/boxology-generator.toml"),
        ),
        (
            "boxology.toml",
            include_bytes!("../../../goldens/generated-project/boxology.toml"),
        ),
        (
            "rust-toolchain.toml",
            include_bytes!("../../../goldens/generated-project/rust-toolchain.toml"),
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

    fn normalize(path: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
        if path.ends_with(".rs") {
            return normalize_rust(bytes);
        }
        if path == "boxology-generator.toml" {
            return normalize_generator_config(bytes);
        }
        Ok(bytes.to_vec())
    }

    fn normalize_rust(bytes: &[u8]) -> Result<Vec<u8>, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "Rust output is not UTF-8")?;
        let marker = "// Generated by boxology-generator ";
        if !text.starts_with(marker) || text.matches(marker).count() != 1 {
            return Err("Rust provenance header must be the unique first-line marker".into());
        }
        let (_, body) = text
            .split_once('\n')
            .ok_or("Rust output must have a first-line provenance header")?;
        Ok(format!("{marker}@PROVENANCE@\n{body}").into_bytes())
    }

    fn normalize_generator_config(bytes: &[u8]) -> Result<Vec<u8>, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "generator config is not UTF-8")?;
        let prefix = "boxology_version = \"";
        let mut anchor = None;
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if line.contains("boxology_version") {
                let value = line
                    .strip_prefix(prefix)
                    .and_then(|value| value.strip_suffix("\"\n"))
                    .filter(|value| {
                        !value.is_empty()
                            && value.bytes().all(|byte| {
                                !byte.is_ascii_control() && !matches!(byte, b'"' | b'\\')
                            })
                    })
                    .ok_or("boxology_version assignment must be exact")?;
                if anchor
                    .replace((offset + prefix.len(), value.len()))
                    .is_some()
                {
                    return Err("generator config must have one boxology version".into());
                }
            }
            offset += line.len();
        }
        let (start, length) = anchor.ok_or("generator config must have one boxology version")?;
        let mut normalized = bytes.to_vec();
        normalized.splice(start..start + length, b"@PROVENANCE@".iter().copied());
        Ok(normalized)
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
            let actual_bytes = generated(tree, path);
            if normalize(path, actual_bytes)? != normalize(path, expected_bytes)? {
                return Err(format!("generated bytes differ for {path}"));
            }
        }
        Ok(())
    }

    #[test]
    fn canonical_request_emits_the_root_platform_subset() {
        let request = request();
        assert_eq!(request.project_name(), "example");
        assert_eq!(request.dependency_source(), "../boxology");
        let tree = initialize(&request).expect("canonical initialization succeeds");
        compare_golden(&tree).expect("the generated tree matches its golden");
        for forbidden in ["ping", "composition"] {
            assert!(
                tree.files()
                    .iter()
                    .all(|file| !file.path().contains(forbidden))
            );
        }
        assert_eq!(generated(&tree, "rust-toolchain.toml"), TOOLCHAIN);
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
        assert!(manifest.crates().is_empty());
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
        assert!(text.contains("members = []"));
        for (name, path) in DEPENDENCIES {
            let expected =
                format!("{name} = {{ version = \"=0.0.0\", path = \"../boxology/{path}\" }}");
            assert!(text.lines().any(|line| line == expected), "{expected}");
        }
        let config = std::str::from_utf8(generated(&tree, "boxology-generator.toml")).unwrap();
        assert!(config.contains(&format!("boxology_version = \"{PROVENANCE}\"")));
        assert!(config.contains("dependency_source = \"../boxology\""));
    }

    #[test]
    fn request_validation_catalog_is_exact_and_payload_safe() {
        for ((project, dependency), expected) in [("Bad.Project\n", "../boxology"), ("example", "")]
            .into_iter()
            .zip(CODE_GOLDEN.lines())
        {
            let diagnostics = InitRequest::new(project, dependency).unwrap_err();
            assert_eq!(diagnostics.as_slice().len(), 1);
            let diagnostic = &diagnostics.as_slice()[0];
            assert_eq!(diagnostic.path(), "<request>");
            let rendered = diagnostics.to_string();
            assert_eq!(rendered, expected);
            assert!(!rendered.contains("Bad.Project"));
            assert!(!rendered.contains(['\n', '\r']));
        }
        let both = InitRequest::new("Bad", "").unwrap_err();
        assert_eq!(
            both.as_slice()
                .iter()
                .map(Diagnostic::code)
                .collect::<Vec<_>>(),
            golden_codes()
        );
        assert_eq!(both.to_string(), CODE_GOLDEN.trim_end());
        for valid in ["a", "example", "a0-b9", "box-"] {
            assert!(InitRequest::new(valid, "source").is_ok(), "{valid}");
        }
    }

    #[test]
    fn production_inventory_and_code_catalog_fail_closed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        files.sort();
        assert_eq!(files, ["lib.rs"]);
        files.push("stray.rs".into());
        assert_ne!(files, ["lib.rs"]);

        let expected = golden_codes();
        assert_eq!(DIAGNOSTICS.map(|entry| entry.0).as_slice(), expected);
        assert_eq!(source_codes(SOURCE), expected);
        assert_eq!(modules(SOURCE), ["mod tests {"]);
        let mut mutated = SOURCE.to_owned();
        mutated.push_str(&format!(
            "\nconst STRAY: &str = \"{}{}\";\nmod stray;\n",
            "BX", "I9999"
        ));
        assert_ne!(source_codes(&mutated), expected);
        assert_ne!(modules(&mutated), ["mod tests {"]);
    }

    #[test]
    fn golden_inventory_and_comparison_fail_closed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../goldens/generated-project");
        let mut found = fs::read_dir(root)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                assert!(entry.file_type().unwrap().is_file());
                entry.file_name().into_string().unwrap()
            })
            .collect::<Vec<_>>();
        found.sort();
        let expected: Vec<String> = GOLDEN
            .iter()
            .map(|(path, _)| (*path).into())
            .collect::<Vec<_>>();
        assert_eq!(found, expected);

        let mut altered = initialize(&request()).unwrap();
        altered.0[0].bytes.push(b'x');
        assert!(compare_golden(&altered).is_err());
    }

    #[test]
    fn provenance_normalizers_reject_mutated_anchors() {
        for malformed in [
            &b"schema = 1\ndependency_source = \"x\"\n"[..],
            &b"schema = 1\n boxology_version = \"a\"\n"[..],
            &b"schema = 1\nboxology_version = \"a\" suffix\n"[..],
            &b"schema = 1\nboxology_version = \"a\"\nboxology_version = \"b\"\n"[..],
        ] {
            assert!(normalize_generator_config(malformed).is_err());
        }
        let tree = initialize(&request()).unwrap();
        let original = generated(&tree, "boxology-generator.toml");
        let mut unrelated = original.to_vec();
        unrelated.extend_from_slice(b"# unrelated\n");
        assert_ne!(
            normalize_generator_config(original).unwrap(),
            normalize_generator_config(&unrelated).unwrap()
        );
        assert!(normalize_rust(b"fn main() {}\n").is_err());
    }
}
