//! Pure assembly of the deterministic Boxology project tree.
//!
//! The initializer emits the root platform files, the ping box package (manifest, implementation
//! crate, and embedded S2-generated contract artifacts), the application composition package, and a
//! Cargo workspace listing those members. It performs no filesystem, runtime environment, network,
//! clock, or process access; filesystem publication remains a separate boundary.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::BoxId;
use boxology_generator::generate;
use boxology_generator_model::GenerationRequest;
use boxology_workspace::CHECK_WORKFLOW;
use std::{
    fmt,
    path::{Component, Path, PathBuf},
};

const RULE_SOURCE: &str = "specs/s6-installer-and-generated-project.md D1";
const REQUEST_PATH: &str = "<request>";
const TOOLCHAIN: &[u8] = include_bytes!("../../../rust-toolchain.toml");
const PING_MANIFEST: &[u8] = include_bytes!("../../fixtures/ping/boxology.toml");
const PING_IMPLEMENTATION_MANIFEST: &[u8] =
    include_bytes!("../../fixtures/ping/implementation/Cargo.toml");
const PING_IMPLEMENTATION: &[u8] = include_bytes!("../../fixtures/ping/implementation/src/lib.rs");
const PING_CONTRACT: &[u8] = include_bytes!("../../fixtures/ping/implementation/src/contract.rs");
const APP_MANIFEST: &[u8] = include_bytes!("../../fixtures/ping-app/boxology.toml");
const APP_COMPOSITION_MANIFEST: &[u8] =
    include_bytes!("../../fixtures/ping-app/composition/Cargo.toml");
const APP_COMPOSITION: &[u8] = include_bytes!("../../fixtures/ping-app/composition/src/lib.rs");
const WORKSPACE_MEMBERS: &[&str] = &[
    "app/composition",
    "ping/generated/contract",
    "ping/implementation",
];
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
const DEPENDENCIES: [(&str, &str); 5] = [
    ("boxology", "crates/boxology"),
    ("boxology-contract", "crates/boxology-contract"),
    ("boxology-http", "crates/boxology-http"),
    ("boxology-manifest", "crates/boxology-manifest"),
    ("boxology-runtime", "crates/boxology-runtime"),
];
const TOKIO_WORKSPACE_DEPENDENCY: &str = "tokio = { version = \"=1.53.0\", default-features = false, features = [\"io-util\", \"macros\", \"net\", \"rt\", \"time\"] }\n";

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
/// This slice uses literal-only output paths for the root platform files, the ping box package
/// files, and the application composition package. Embedded generator paths are confined before the
/// `ping/` prefix is applied. A writer must pass every path through [`confined_destination`] before
/// joining it to its destination.
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
            ".github/workflows/check.yml",
            CHECK_WORKFLOW.as_bytes().to_vec(),
        ),
        file(
            "Cargo.toml",
            cargo_manifest(request.dependency_source()).into_bytes(),
        ),
        file("README.md", readme(request.project_name()).into_bytes()),
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
    files.extend(embed_app());
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
    let mut files = vec![
        file("ping/boxology.toml", PING_MANIFEST.to_vec()),
        file(
            "ping/implementation/Cargo.toml",
            PING_IMPLEMENTATION_MANIFEST.to_vec(),
        ),
        file(
            "ping/implementation/src/lib.rs",
            PING_IMPLEMENTATION.to_vec(),
        ),
        file(
            "ping/implementation/src/contract.rs",
            PING_CONTRACT.to_vec(),
        ),
    ];
    files.extend(embed(PING_MANIFEST, PING_IMPLEMENTATION, PING_CONTRACT)?);
    Ok(files)
}

fn embed_app() -> Vec<GeneratedFile> {
    vec![
        file("app/boxology.toml", APP_MANIFEST.to_vec()),
        file(
            "app/composition/Cargo.toml",
            APP_COMPOSITION_MANIFEST.to_vec(),
        ),
        file("app/composition/src/lib.rs", APP_COMPOSITION.to_vec()),
    ]
}

fn embed(
    manifest: &[u8],
    implementation: &[u8],
    contract: &[u8],
) -> Result<Vec<GeneratedFile>, Diagnostics> {
    let box_id = BoxId::new("ping").map_err(|_| embedded_generation())?;
    let request = GenerationRequest::new(
        box_id,
        "implementation/src/lib.rs".into(),
        vec![
            ("boxology.toml".into(), manifest.to_vec()),
            ("implementation/src/lib.rs".into(), implementation.to_vec()),
            ("implementation/src/contract.rs".into(), contract.to_vec()),
        ],
        vec![],
        boxology_generator::OUTPUTS
            .iter()
            .map(|path| (*path).to_owned())
            .collect(),
    )
    .map_err(|_| embedded_generation())?;
    let generated = generate(request).map_err(|_| embedded_generation())?;
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
    let mut members = WORKSPACE_MEMBERS.to_vec();
    members.sort_unstable();
    let mut text = String::from("[workspace]\nresolver = \"3\"\nmembers = [\n");
    for member in members {
        text.push_str(&format!("    {},\n", toml_string(member)));
    }
    text.push_str("]\n\n[workspace.package]\nedition = \"2024\"\n\n[workspace.dependencies]\n");
    for (name, _) in DEPENDENCIES {
        text.push_str(&format!("{name} = \"=0.0.0\"\n"));
    }
    text.push_str(TOKIO_WORKSPACE_DEPENDENCY);
    text.push_str("\n[patch.crates-io]\n");
    for (name, path) in DEPENDENCIES {
        text.push_str(&format!(
            "{name} = {{ path = {} }}\n",
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
        "schema = 1\nid = {}\nkind = \"platform\"\nowned = [\"Cargo.toml\", \"README.md\", \"rust-toolchain.toml\", \"boxology.toml\", \"boxology-generator.toml\", \".gitignore\", \".github/**\"]\n\n[[derived]]\nid = \"lockfile\"\ngenerator = \"cargo\"\ninputs = [\"**/Cargo.toml\"]\noutputs = [\"Cargo.lock\"]\n",
        toml_string(project_name)
    )
}

fn readme(project_name: &str) -> String {
    format!(
        "# {project_name}\n\nThis generated Boxology project contains the `ping` box and the `ping-app` composition, with the same capability bound in-process and over HTTP.\n\n## Build\n\n```sh\ncargo build --workspace\n```\n\nThis first ordinary Cargo build creates the derived `Cargo.lock`; the initializer deliberately does not emit it.\n\n## Invoke through Rust and HTTP\n\n```sh\ncargo test -p ping-app assembled_ping_answers_in_process_and_over_real_http\n```\n\nThe test starts the composition, invokes `ping.ping` through its Rust binding, then sends a real HTTP request to `/rpc/ping/ping`.\n\n## Validate\n\n```sh\nboxology check\n```\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_manifest::{CrateRole, Exposure, Kind, Manifest, RelativePath, Transport};
    use std::fs;
    use std::path::Path;

    const SOURCE: &str = include_str!("lib.rs");
    const CODE_GOLDEN: &str = include_str!("../test/bxi.golden");
    const PROVENANCE_ANCHOR: &[u8] = b"  \"provenance\": ";
    const PROVENANCE_TOKEN: &[u8] = b"\"@PROVENANCE@\"";
    const GENERATOR_HEADER: &str = "// Generated by boxology-generator ";
    const STD_IMPORT: &str = "use std::{\n    fmt,\n    path::{Component, Path, PathBuf},\n};";
    #[rustfmt::skip]
    const GOLDEN: [(&str, &[u8]); 18] = [
        (
            ".github/workflows/check.yml",
            include_bytes!("../../../goldens/generated-project/.github/workflows/check.yml"),
        ),
        (".gitignore", include_bytes!("../../../goldens/generated-project/.gitignore")),
        ("Cargo.toml", include_bytes!("../../../goldens/generated-project/Cargo.toml")),
        ("README.md", include_bytes!("../../../goldens/generated-project/README.md")),
        (
            "app/boxology.toml",
            include_bytes!("../../../goldens/generated-project/app/boxology.toml"),
        ),
        (
            "app/composition/Cargo.toml",
            include_bytes!("../../../goldens/generated-project/app/composition/Cargo.toml"),
        ),
        (
            "app/composition/src/lib.rs",
            include_bytes!("../../../goldens/generated-project/app/composition/src/lib.rs"),
        ),
        (
            "boxology-generator.toml",
            include_bytes!("../../../goldens/generated-project/boxology-generator.toml"),
        ),
        ("boxology.toml", include_bytes!("../../../goldens/generated-project/boxology.toml")),
        (
            "ping/boxology.toml",
            include_bytes!("../../../goldens/generated-project/ping/boxology.toml"),
        ),
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
        (
            "ping/implementation/Cargo.toml",
            include_bytes!("../../../goldens/generated-project/ping/implementation/Cargo.toml"),
        ),
        (
            "ping/implementation/src/contract.rs",
            include_bytes!("../../../goldens/generated-project/ping/implementation/src/contract.rs"),
        ),
        (
            "ping/implementation/src/lib.rs",
            include_bytes!("../../../goldens/generated-project/ping/implementation/src/lib.rs"),
        ),
        ("rust-toolchain.toml", include_bytes!("../../../goldens/generated-project/rust-toolchain.toml")),
    ];
    const FIXTURE_CORPUS: [(&str, &[u8]); 8] = [
        (
            "boxology.toml",
            include_bytes!("../../fixtures/ping/boxology.toml"),
        ),
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
        (
            "implementation/Cargo.toml",
            include_bytes!("../../fixtures/ping/implementation/Cargo.toml"),
        ),
        (
            "implementation/src/contract.rs",
            include_bytes!("../../fixtures/ping/implementation/src/contract.rs"),
        ),
        (
            "implementation/src/lib.rs",
            include_bytes!("../../fixtures/ping/implementation/src/lib.rs"),
        ),
    ];
    const FIXTURE_APP_CORPUS: [(&str, &[u8]); 3] = [
        (
            "boxology.toml",
            include_bytes!("../../fixtures/ping-app/boxology.toml"),
        ),
        (
            "composition/Cargo.toml",
            include_bytes!("../../fixtures/ping-app/composition/Cargo.toml"),
        ),
        (
            "composition/src/lib.rs",
            include_bytes!("../../fixtures/ping-app/composition/src/lib.rs"),
        ),
    ];
    const PING_PACKAGE_PATHS: [&str; 4] = [
        "ping/boxology.toml",
        "ping/implementation/Cargo.toml",
        "ping/implementation/src/contract.rs",
        "ping/implementation/src/lib.rs",
    ];
    const APP_PACKAGE_PATHS: [&str; 3] = [
        "app/boxology.toml",
        "app/composition/Cargo.toml",
        "app/composition/src/lib.rs",
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
        let extension = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str());
        match extension {
            Some("rs") if path.starts_with("ping/generated/") => {
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
                "README.md",
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
        let mut members = WORKSPACE_MEMBERS.to_vec();
        members.sort_unstable();
        let mut expected_members = String::from("members = [\n");
        for member in members {
            expected_members.push_str(&format!("    \"{member}\",\n"));
        }
        expected_members.push(']');
        assert!(
            text.contains(&expected_members),
            "workspace members must be exactly {expected_members:?}"
        );
        assert_eq!(
            WORKSPACE_MEMBERS,
            [
                "app/composition",
                "ping/generated/contract",
                "ping/implementation",
            ]
        );
        for (name, path) in DEPENDENCIES {
            let dependency = format!("{name} = \"=0.0.0\"");
            let patch = format!("{name} = {{ path = \"../boxology/{path}\" }}");
            assert!(text.lines().any(|line| line == dependency), "{dependency}");
            assert!(text.lines().any(|line| line == patch), "{patch}");
        }
        assert!(
            text.lines()
                .any(|line| line == TOKIO_WORKSPACE_DEPENDENCY.trim_end()),
            "workspace must declare the exact tokio dependency"
        );
    }

    #[test]
    fn workflow_and_readme_are_exact_and_request_dependent_only_at_the_h1() {
        let tree = initialize(&request()).unwrap();
        assert_eq!(
            generated(&tree, ".github/workflows/check.yml"),
            CHECK_WORKFLOW.as_bytes()
        );

        let readme = std::str::from_utf8(generated(&tree, "README.md")).unwrap();
        assert_eq!(
            readme
                .lines()
                .filter(|line| line.starts_with("# "))
                .collect::<Vec<_>>(),
            ["# example"]
        );
        for anchor in [
            "cargo build --workspace",
            "derived `Cargo.lock`",
            "cargo test -p ping-app assembled_ping_answers_in_process_and_over_real_http",
            "/rpc/ping/ping",
            "boxology check",
        ] {
            assert!(readme.contains(anchor), "missing README anchor {anchor:?}");
        }

        let other = initialize(&InitRequest::new("demo-2", "../boxology").unwrap()).unwrap();
        let other = std::str::from_utf8(generated(&other, "README.md")).unwrap();
        assert_eq!(
            other
                .lines()
                .filter(|line| line.starts_with("# "))
                .collect::<Vec<_>>(),
            ["# demo-2"]
        );
        assert_eq!(
            readme.strip_prefix("# example").unwrap(),
            other.strip_prefix("# demo-2").unwrap()
        );
    }

    #[test]
    fn app_manifest_parses_with_exact_composition_shape() {
        let tree = initialize(&request()).unwrap();
        let path = RelativePath::new("app/boxology.toml").unwrap();
        let manifest = Manifest::parse(path, generated(&tree, "app/boxology.toml"))
            .expect("emitted composition manifest parses");
        assert_eq!(manifest.id().as_str(), "ping-app");
        assert_eq!(manifest.kind(), Kind::Composition);
        assert_eq!(globs(manifest.owned()), ["boxology.toml", "composition/**"]);
        assert_eq!(
            manifest.quality_commands(),
            &[
                "cargo test -p ping-app tests::assembled_ping_answers_in_process_and_over_real_http -- --exact"
            ]
        );
        assert_eq!(manifest.crates().len(), 1);
        let crate_entry = &manifest.crates()[0];
        assert_eq!(crate_entry.cargo_package(), "ping-app");
        assert_eq!(crate_entry.path().as_str(), "composition");
        assert_eq!(crate_entry.role(), CrateRole::Composition);

        let composition = manifest.composition().expect("composition section exists");
        assert_eq!(
            composition
                .boxes()
                .iter()
                .map(|box_id| box_id.as_str())
                .collect::<Vec<_>>(),
            ["ping"]
        );
        assert_eq!(composition.bindings().len(), 2);
        let bindings = composition.bindings();
        assert_eq!(bindings[0].capability().to_string(), "ping.*");
        assert_eq!(bindings[0].transport(), Transport::InProcess);
        assert_eq!(bindings[0].exposure(), None);
        assert_eq!(bindings[1].capability().to_string(), "ping.*");
        assert_eq!(bindings[1].transport(), Transport::Http);
        assert_eq!(bindings[1].exposure(), Some(Exposure::External));
    }

    #[test]
    fn ping_manifest_parses_with_exact_box_roles_and_derived() {
        let tree = initialize(&request()).unwrap();
        let path = RelativePath::new("ping/boxology.toml").unwrap();
        let manifest = Manifest::parse(path, generated(&tree, "ping/boxology.toml"))
            .expect("emitted ping manifest parses");
        assert_eq!(manifest.id().as_str(), "ping");
        assert_eq!(manifest.kind(), Kind::Box);
        assert_eq!(
            manifest
                .crates()
                .iter()
                .map(|entry| { (entry.cargo_package(), entry.path().as_str(), entry.role(),) })
                .collect::<Vec<_>>(),
            [
                (
                    "ping-implementation",
                    "implementation",
                    CrateRole::BoxImplementation,
                ),
                (
                    "ping-contract",
                    "generated/contract",
                    CrateRole::BoxContract
                ),
            ]
        );
        let derived = manifest.derived();
        assert_eq!(derived.len(), 1);
        assert_eq!(derived[0].id().as_str(), "contract");
        assert_eq!(derived[0].generator(), "boxology-contract");
        assert_eq!(
            globs(derived[0].inputs()),
            ["boxology.toml", "implementation/src/**"]
        );
        assert_eq!(
            globs(derived[0].outputs()),
            [
                "generated/contract/**",
                "generated/adapter/**",
                "generated/schema.json",
            ]
        );
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
            r#"boxology = { path = "../boxology/\"quoted\"\\checkout/crates/boxology" }"#,
            r#"boxology-contract = { path = "../boxology/\"quoted\"\\checkout/crates/boxology-contract" }"#,
            r#"boxology-http = { path = "../boxology/\"quoted\"\\checkout/crates/boxology-http" }"#,
            r#"boxology-manifest = { path = "../boxology/\"quoted\"\\checkout/crates/boxology-manifest" }"#,
            r#"boxology-runtime = { path = "../boxology/\"quoted\"\\checkout/crates/boxology-runtime" }"#,
        ];
        for expected in expected {
            assert_eq!(
                cargo.lines().filter(|line| *line == expected).count(),
                1,
                "{expected}"
            );
        }
        assert_eq!(
            cargo
                .lines()
                .filter(|line| *line == TOKIO_WORKSPACE_DEPENDENCY.trim_end())
                .count(),
            1
        );
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
        let lib_golden = CODE_GOLDEN.lines().take(4).collect::<Vec<_>>().join("\n");
        assert_eq!(catalog, lib_golden);
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
        assert_eq!(files, ["lib.rs", "main.rs", "write.rs"]);
        files.push("stray.rs".into());
        assert_ne!(files, ["lib.rs", "main.rs", "write.rs"]);
        let production = production_source(SOURCE).expect("locked production/test boundary");
        let expected = golden_codes();
        assert_eq!(
            expected,
            [
                "BXI0001", "BXI0002", "BXI0003", "BXI0004", "BXI0005", "BXI0006", "BXI0007",
                "BXI0008", "BXI0009",
            ]
        );
        assert_eq!(DIAGNOSTICS.map(|entry| entry.0).as_slice(), &expected[..4]);
        let mut scanned = source_codes(production);
        scanned.extend(source_codes(include_str!("main.rs")));
        scanned.extend(source_codes(include_str!("write.rs")));
        assert_eq!(scanned, expected);
        assert_eq!(expected, scanned);
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
        assert_ne!(source_codes(&mutated), &expected[..4]);
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
        for path in [".github/workflows/check.yml", "README.md"] {
            assert_eq!(confined_destination(root, path), Ok(root.join(path)));
        }
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
        for path in PING_PACKAGE_PATHS {
            assert_eq!(confined_logical(path), Ok(path), "{path}");
        }
        for path in APP_PACKAGE_PATHS {
            assert_eq!(confined_logical(path), Ok(path), "{path}");
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
        let diagnostics = embed(&[], &[], &[]).unwrap_err();
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
        assert!(!expected.iter().any(|path| path == "Cargo.lock"));

        let mut mutation_paths = vec![
            ".gitignore",
            ".github/workflows/check.yml",
            "README.md",
            "ping/generated/adapter/adapter.rs",
            "ping/generated/schema.json",
        ];
        mutation_paths.extend(PING_PACKAGE_PATHS);
        mutation_paths.extend(APP_PACKAGE_PATHS);
        for path in mutation_paths {
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
    fn rust_normalization_scopes_only_to_generated_output() {
        let left = format!("{GENERATOR_HEADER}0.0.0\nfn main() {{}}\n");
        let right = format!("{GENERATOR_HEADER}9.9.9\nfn main() {{}}\n");
        assert!(
            compare_kind(
                "ping/generated/contract/src/lib.rs",
                left.as_bytes(),
                right.as_bytes()
            )
            .is_ok(),
            "generated Rust under ping/generated/** must normalize provenance"
        );
        assert_eq!(
            compare_kind(
                "ping/generated/contract/src/lib.rs",
                b"fn main() {}\n",
                b"fn main() {}\n"
            )
            .unwrap_err(),
            "Rust output must have exactly one generator header"
        );
        assert!(
            compare_kind(
                "ping/implementation/src/lib.rs",
                b"fn main() {}\n",
                b"fn main() {}\n"
            )
            .is_ok(),
            "handwritten implementation Rust must compare byte-exactly without a generator header"
        );
        assert!(
            compare_kind(
                "ping/implementation/src/lib.rs",
                left.as_bytes(),
                right.as_bytes()
            )
            .is_err(),
            "handwritten implementation Rust must not normalize away provenance differences"
        );
        assert!(
            compare_kind(
                "app/composition/src/lib.rs",
                b"fn main() {}\n",
                b"fn main() {}\n"
            )
            .is_ok(),
            "composition Rust must compare byte-exactly without a generator header"
        );
        assert!(
            compare_kind(
                "app/composition/src/lib.rs",
                left.as_bytes(),
                right.as_bytes()
            )
            .is_err(),
            "composition Rust must not normalize away provenance differences"
        );
    }

    #[test]
    fn embedded_artifacts_match_fixture_corpus() {
        let tree = initialize(&request()).unwrap();
        for (relative, expected) in FIXTURE_CORPUS {
            let path = format!("ping/{relative}");
            let actual = generated(&tree, &path);
            compare_kind(&path, actual, expected).unwrap_or_else(|error| panic!("{path}: {error}"));
            if matches!(
                relative,
                "boxology.toml"
                    | "implementation/Cargo.toml"
                    | "implementation/src/contract.rs"
                    | "implementation/src/lib.rs"
            ) {
                assert_eq!(
                    actual, expected,
                    "{path} must match the fixture corpus byte-exactly"
                );
            }
        }
        for (relative, expected) in FIXTURE_APP_CORPUS {
            let path = format!("app/{relative}");
            let actual = generated(&tree, &path);
            assert_eq!(
                actual, expected,
                "{path} must match the fixture-app corpus byte-exactly"
            );
            compare_kind(&path, actual, expected).unwrap_or_else(|error| panic!("{path}: {error}"));
        }
    }

    // source inventory ends here
}
