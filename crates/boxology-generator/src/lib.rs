//! Pure generation of deterministic Boxology artifacts from validated logical inputs.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_generator_model::{Diagnostics, GenerationRequest, ParsedRustInputs};

const OUTPUTS: [&str; 2] = [
    "generated/contract/Cargo.toml",
    "generated/contract/src/lib.rs",
];

/// One generated logical file.
#[derive(Debug, Eq, PartialEq)]
pub struct GeneratedFile {
    path: String,
    bytes: Vec<u8>,
}

impl GeneratedFile {
    /// Returns the generated file's logical path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the generated file's exact bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// A generated tree sorted by logical-path bytes.
#[derive(Debug, Eq, PartialEq)]
pub struct GeneratedTree(Vec<GeneratedFile>);

impl GeneratedTree {
    /// Returns all generated files in canonical logical-path order.
    pub fn files(&self) -> &[GeneratedFile] {
        &self.0
    }
}

/// Generates the current controlled contract-package scaffold without external I/O.
///
/// # Errors
/// Returns sorted model diagnostics when the request, source topology, controlled contract, or
/// complete output declaration is invalid.
pub fn generate(request: &GenerationRequest) -> Result<GeneratedTree, Diagnostics> {
    request.require_exact_outputs(&OUTPUTS)?;
    let parsed = ParsedRustInputs::parse(request)?;
    let contract = parsed.controlled_contract()?;
    let manifest = format!(
        "[package]\nname = \"{}-contract\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\nboxology-contract = {{ workspace = true }}\n",
        request.box_id().as_str()
    );
    let error = &contract.model().error;
    let error_attrs = attributes(&error.docs, &error.deprecation);
    let variants = error
        .variants
        .iter()
        .map(|variant| {
            format!(
                "{}{},",
                attributes(&variant.docs, &variant.deprecation),
                variant.name
            )
        })
        .collect::<String>();
    let encode_arms = error
        .variants
        .iter()
        .map(|variant| {
            format!(
                "Self::{} => ({:?}.into(), ::boxology_contract::SlotValue::Null),",
                variant.name, variant.name
            )
        })
        .collect::<String>();
    let decode_arms = error
        .variants
        .iter()
        .map(|variant| {
            format!(
                r#"
                {name:?} if matches!(payload, ::boxology_contract::SlotValue::Null) => Ok(Self::{name}),
                {name:?} => Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::UnexpectedPayload).under(::boxology_contract::PathSegment::Variant(tag.into()))),
                "#,
                name = variant.name
            )
        })
        .collect::<String>();
    let tag_arms = error
        .variants
        .iter()
        .map(|variant| format!("Self::{} => {:?},", variant.name, variant.name))
        .collect::<String>();
    let error_abi = format!(
        r#"
        impl ::boxology_contract::ContractType for {error} {{
            fn encode_value(&self) -> ::core::result::Result<::boxology_contract::ContractValue, ::boxology_contract::EncodeError> {{
                let (tag, payload) = match self {{
                    {encode_arms}
                    Self::Unknown {{ tag, payload }} => (tag.clone(), ::boxology_contract::SlotValue::Value(::boxology_contract::ContractValue::opaque(payload.forward()))),
                }};
                Ok(::boxology_contract::ContractValue::enum_value(tag, payload))
            }}
            fn decode_value(value: &::boxology_contract::ContractValue) -> ::core::result::Result<Self, ::boxology_contract::DecodeError> {{
                let ::boxology_contract::ValueRef::Enum {{ tag, payload }} = value.view() else {{
                    return Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::KindMismatch));
                }};
                match tag {{
                    {decode_arms}
                    _ => match payload {{
                        ::boxology_contract::SlotValue::Value(value) => match value.view() {{
                            ::boxology_contract::ValueRef::Opaque(payload) => Ok(Self::Unknown {{ tag: tag.into(), payload: payload.forward() }}),
                            _ => Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::UnknownVariant(tag.into())).under(::boxology_contract::PathSegment::Variant(tag.into()))),
                        }},
                        _ => Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::UnknownVariant(tag.into())).under(::boxology_contract::PathSegment::Variant(tag.into()))),
                    }},
                }}
            }}
        }}
        impl ::boxology_contract::ContractError for {error} {{
            fn error_tag(&self) -> &str {{
                match self {{ {tag_arms} Self::Unknown {{ tag, .. }} => tag }}
            }}
        }}
        "#,
        error = error.name
    );
    let digest = contract
        .semantic_digest()
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let checker = r#"
        #[doc(hidden)]
        #[macro_export]
        macro_rules! __boxology_check_implementation {
            ($receiver:ty; $($method:ident $validity:ident;)*) => {
                $crate::__boxology_check_implementation!(@find $receiver; $($method $validity;)*);
            };
            (@find $receiver:ty; __CAPABILITY__ valid; $($rest:tt)*) => {
                const _: () = {
                    fn require_service<T: ::core::marker::Send + ::core::marker::Sync + 'static>() {}
                    fn require_future<F: ::core::future::Future<Output = ::core::result::Result<::std::string::String, $crate::__ERROR__>> + ::core::marker::Send>(_: F) {}
                    fn check(receiver: &$receiver, context: ::boxology::CallContext, input: ::std::string::String) {
                        require_service::<$receiver>();
                        require_future(receiver.__CAPABILITY__(context, input));
                    }
                };
            };
            (@find $receiver:ty; __CAPABILITY__ invalid; $($rest:tt)*) => {
                compile_error!("Boxology capability has an invalid structural signature");
            };
            (@find $receiver:ty; $other:ident $validity:ident; $($rest:tt)*) => {
                $crate::__boxology_check_implementation!(@find $receiver; $($rest)*);
            };
            (@find $receiver:ty;) => {
                compile_error!("Boxology capability implementation is missing");
            };
        }
    "#
    .replace("__CAPABILITY__", &contract.model().capability.name)
    .replace("__ERROR__", &error.name);
    let syntax = syn::parse_file(&format!(
        "{error_attrs}#[derive(Debug, Clone, PartialEq)] pub enum {} {{{variants} Unknown {{ tag: ::std::string::String, payload: ::boxology_contract::OpaquePayload }}}} {error_abi} #[doc(hidden)] pub const __BOXOLOGY_SEMANTIC_DIGEST: [u8; 32] = [{digest}]; {checker}",
        error.name
    ))
    .expect("validated names and fixed generator template must parse");
    let rust = format!(
        "// Generated by boxology-generator {}\n{}",
        env!("CARGO_PKG_VERSION"),
        prettyplease::unparse(&syntax)
    );
    let mut files = OUTPUTS
        .iter()
        .zip([manifest.into_bytes(), rust.into_bytes()])
        .map(|(path, bytes)| GeneratedFile {
            path: (*path).to_owned(),
            bytes,
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(GeneratedTree(files))
}

fn attributes(docs: &[String], deprecation: &Option<String>) -> String {
    let mut attributes = docs
        .iter()
        .map(|doc| format!("#[doc = {doc:?}]"))
        .collect::<String>();
    if let Some(note) = deprecation {
        if note.is_empty() {
            attributes.push_str("#[deprecated]");
        } else {
            attributes.push_str(&format!("#[deprecated(note = {note:?})]"));
        }
    }
    attributes
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::BoxId;

    const MANIFEST: &[u8] = b"schema = 1\nid = \"hello\"\nkind = \"box\"\n";
    const CONTRACT: &str = "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
    const CARGO: &[u8] = b"[package]\nname = \"hello-contract\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\nboxology-contract = { workspace = true }\n";
    const RUST: &[u8] = br#"// Generated by boxology-generator 0.0.0
#[derive(Debug, Clone, PartialEq)]
pub enum GreetError {
    EmptyName,
    Unknown { tag: ::std::string::String, payload: ::boxology_contract::OpaquePayload },
}
impl ::boxology_contract::ContractType for GreetError {
    fn encode_value(
        &self,
    ) -> ::core::result::Result<
        ::boxology_contract::ContractValue,
        ::boxology_contract::EncodeError,
    > {
        let (tag, payload) = match self {
            Self::EmptyName => ("EmptyName".into(), ::boxology_contract::SlotValue::Null),
            Self::Unknown { tag, payload } => {
                (
                    tag.clone(),
                    ::boxology_contract::SlotValue::Value(
                        ::boxology_contract::ContractValue::opaque(payload.forward()),
                    ),
                )
            }
        };
        Ok(::boxology_contract::ContractValue::enum_value(tag, payload))
    }
    fn decode_value(
        value: &::boxology_contract::ContractValue,
    ) -> ::core::result::Result<Self, ::boxology_contract::DecodeError> {
        let ::boxology_contract::ValueRef::Enum { tag, payload } = value.view() else {
            return Err(
                ::boxology_contract::DecodeError::new(
                    ::boxology_contract::DecodeErrorKind::KindMismatch,
                ),
            );
        };
        match tag {
            "EmptyName" if matches!(payload, ::boxology_contract::SlotValue::Null) => {
                Ok(Self::EmptyName)
            }
            "EmptyName" => {
                Err(
                    ::boxology_contract::DecodeError::new(
                            ::boxology_contract::DecodeErrorKind::UnexpectedPayload,
                        )
                        .under(::boxology_contract::PathSegment::Variant(tag.into())),
                )
            }
            _ => {
                match payload {
                    ::boxology_contract::SlotValue::Value(value) => {
                        match value.view() {
                            ::boxology_contract::ValueRef::Opaque(payload) => {
                                Ok(Self::Unknown {
                                    tag: tag.into(),
                                    payload: payload.forward(),
                                })
                            }
                            _ => {
                                Err(
                                    ::boxology_contract::DecodeError::new(
                                            ::boxology_contract::DecodeErrorKind::UnknownVariant(
                                                tag.into(),
                                            ),
                                        )
                                        .under(
                                            ::boxology_contract::PathSegment::Variant(tag.into()),
                                        ),
                                )
                            }
                        }
                    }
                    _ => {
                        Err(
                            ::boxology_contract::DecodeError::new(
                                    ::boxology_contract::DecodeErrorKind::UnknownVariant(
                                        tag.into(),
                                    ),
                                )
                                .under(
                                    ::boxology_contract::PathSegment::Variant(tag.into()),
                                ),
                        )
                    }
                }
            }
        }
    }
}
impl ::boxology_contract::ContractError for GreetError {
    fn error_tag(&self) -> &str {
        match self {
            Self::EmptyName => "EmptyName",
            Self::Unknown { tag, .. } => tag,
        }
    }
}
#[doc(hidden)]
pub const __BOXOLOGY_SEMANTIC_DIGEST: [u8; 32] = [
    84, 95, 20, 43, 12, 237, 118, 112, 227, 249, 239, 199, 188, 170, 243, 183, 162, 160,
    178, 183, 144, 229, 180, 138, 202, 168, 94, 73, 1, 200, 155, 24,
];
#[doc(hidden)]
#[macro_export]
macro_rules! __boxology_check_implementation {
    ($receiver:ty; $($method:ident $validity:ident;)*) => {
        $crate::__boxology_check_implementation!(@ find $receiver; $($method
        $validity;)*);
    };
    (@ find $receiver:ty; greet valid; $($rest:tt)*) => {
        const _ : () = { fn require_service < T : ::core::marker::Send +
        ::core::marker::Sync + 'static > () {} fn require_future < F :
        ::core::future::Future < Output = ::core::result::Result <::std::string::String,
        $crate::GreetError >> + ::core::marker::Send > (_ : F) {} fn check(receiver :
        &$receiver, context : ::boxology::CallContext, input : ::std::string::String) {
        require_service::<$receiver > (); require_future(receiver.greet(context, input));
        } };
    };
    (@ find $receiver:ty; greet invalid; $($rest:tt)*) => {
        compile_error!("Boxology capability has an invalid structural signature");
    };
    (@ find $receiver:ty; $other:ident $validity:ident; $($rest:tt)*) => {
        $crate::__boxology_check_implementation!(@ find $receiver; $($rest)*);
    };
    (@ find $receiver:ty;) => {
        compile_error!("Boxology capability implementation is missing");
    };
}
"#;

    fn request(source: &str, reverse: bool, outputs: Vec<&str>) -> GenerationRequest {
        let mut inputs = vec![
            ("boxology.toml".into(), MANIFEST.to_vec()),
            ("src/lib.rs".into(), source.as_bytes().to_vec()),
        ];
        if reverse {
            inputs.reverse();
        }
        GenerationRequest::new(
            BoxId::new("hello").unwrap(),
            "src/lib.rs".into(),
            inputs,
            vec![],
            outputs.into_iter().map(str::to_owned).collect(),
        )
        .unwrap()
    }

    fn tree(source: &str, reverse: bool) -> GeneratedTree {
        generate(&request(source, reverse, OUTPUTS.to_vec())).unwrap()
    }

    fn marker_parts(bytes: &[u8]) -> (&str, &str, &str) {
        let text = std::str::from_utf8(bytes).unwrap();
        let start = text.find("= [").unwrap() + 3;
        let end = text.find("];\n").unwrap();
        (&text[..start], &text[start..end], &text[end..])
    }

    #[test]
    fn cold_hello_bytes_are_exact_and_parseable() {
        let tree = tree(CONTRACT, false);
        assert_eq!(
            tree.files()
                .iter()
                .map(GeneratedFile::path)
                .collect::<Vec<_>>(),
            OUTPUTS
        );
        assert_eq!(tree.files()[0].bytes(), CARGO);
        assert_eq!(tree.files()[1].bytes(), RUST);
        let rust = std::str::from_utf8(tree.files()[1].bytes()).unwrap();
        assert!(rust.starts_with("// Generated by boxology-generator 0.0.0\n"));
        syn::parse_file(rust).unwrap();
    }

    #[test]
    fn spelling_and_order_do_not_change_artifacts() {
        let decorated = "// ignored\nboxology::contract! { #[error] pub enum GreetError { EmptyName } /* ignored */ #[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, GreetError>; }";
        assert_eq!(tree(CONTRACT, false), tree(decorated, true));
        assert_eq!(
            generate(&request(
                CONTRACT,
                true,
                OUTPUTS.iter().rev().copied().collect()
            ))
            .unwrap(),
            tree(CONTRACT, false)
        );
    }

    #[test]
    fn reserved_unknown_variant_fails_before_artifact_generation() {
        let source = CONTRACT.replace("EmptyName", "Unknown");
        let result = generate(&request(&source, false, OUTPUTS.to_vec()));
        let diagnostics = result.expect_err("reserved input must not return an artifact tree");
        assert_eq!(
            diagnostics.to_string(),
            "BXG0038 src/lib.rs:1:54-1:61 offending=\"invalid controlled contract syntax\" rule=\"contract tokens must satisfy the controlled v0 grammar\" source=\"specs/s2-contract-generator.md D3\""
        );
    }

    #[test]
    fn semantic_change_only_changes_digest_marker() {
        let changed = CONTRACT.replace("EmptyName", "MissingName");
        let before = tree(CONTRACT, false);
        let after = tree(&changed, false);
        assert_eq!(before.files()[0], after.files()[0]);
        let before = marker_parts(before.files()[1].bytes());
        let after = marker_parts(after.files()[1].bytes());
        assert!(before.0.contains("EmptyName"));
        assert!(after.0.contains("MissingName"));
        assert_eq!(before.2, after.2);
        assert_ne!(before.1, after.1);
    }

    #[test]
    fn public_error_preserves_decoded_metadata() {
        let source = "boxology::contract! { #[doc = \"failure\"] #[deprecated(note = \"old\")] #[error] pub enum GreetError { #[doc = \"empty\"] #[deprecated] EmptyName } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
        let generated = tree(source, false);
        let rust = std::str::from_utf8(generated.files()[1].bytes()).unwrap();
        for expected in [
            "///failure",
            "#[deprecated(note = \"old\")]",
            "///empty",
            "#[deprecated]",
            "#[derive(Debug, Clone, PartialEq)]",
        ] {
            assert!(rust.contains(expected), "missing {expected} in {rust}");
        }
    }

    #[test]
    fn cargo_proves_generated_boundary_and_stale_failure() {
        use std::{fs, process::Command};
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let generated = tree(CONTRACT, false);
        let root = std::env::temp_dir().join(format!(
            "boxology-boundary-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        for file in generated.files() {
            let path = root.join(file.path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file.bytes()).unwrap();
        }
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[workspace]\nmembers=[\"generated/contract\",\"consumer\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology-contract={{version=\"=0.0.0\",path={:?}}}\n",
                workspace.join("boxology-contract")
            ),
        ).unwrap();
        let consumer = root.join("consumer");
        fs::create_dir_all(consumer.join("src")).unwrap();
        fs::write(
            consumer.join("Cargo.toml"),
            format!(
                "[package]\nname=\"consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology={{path={:?}}}\nboxology-contract={{workspace=true}}\nboxology_generated_contract={{package=\"hello-contract\",path=\"../generated/contract\"}}\n",
                workspace.join("boxology")
            ),
        )
        .unwrap();
        let source = |error, variants, body| {
            format!(
                "boxology::contract! {{ /* spelling is irrelevant */\n#[error] pub enum {error} {{ {variants} }}\n#[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, {error}>; }}\nuse boxology_contract::{{ContractError, ContractType, ContractValue, DecodeErrorKind, OpaquePayload, OpaqueTree, PathSegment, SlotValue}};\nfn main() {{ {body} }}\n"
            )
        };
        let abi = r#"
            let known = GreetError::EmptyName;
            let encoded = known.encode_value().unwrap();
            assert_eq!(encoded, ContractValue::enum_value("EmptyName", SlotValue::Null));
            assert_eq!(GreetError::decode_value(&encoded).unwrap(), known);
            assert_eq!(known.error_tag(), "EmptyName");
            let malformed = ContractValue::enum_value("EmptyName", SlotValue::Value(ContractValue::string("secret")));
            let error = GreetError::decode_value(&malformed).unwrap_err();
            assert_eq!(error.kind(), &DecodeErrorKind::UnexpectedPayload);
            assert_eq!(error.path(), &[PathSegment::Variant("EmptyName".into())]);
            assert!(!format!("{error}").contains("secret"));
            let mismatch = GreetError::decode_value(&ContractValue::string("secret")).unwrap_err();
            assert_eq!(mismatch.kind(), &DecodeErrorKind::KindMismatch);
            let invalid = ContractValue::enum_value("Future", SlotValue::Value(ContractValue::string("secret")));
            assert!(matches!(GreetError::decode_value(&invalid).unwrap_err().kind(), DecodeErrorKind::UnknownVariant(tag) if tag == "Future"));
            let unknown = GreetError::Unknown { tag: "Future".into(), payload: OpaquePayload::new(OpaqueTree::String("secret".into())) };
            let forwarded = GreetError::decode_value(&unknown.encode_value().unwrap()).unwrap();
            assert_eq!(forwarded, unknown);
            assert_eq!(unknown.error_tag(), "Future");
            assert!(!format!("{unknown:?}").contains("secret"));
            let _: boxology_generated_contract::GreetError = known;
        "#;
        fs::write(
            consumer.join("src/main.rs"),
            source("GreetError", "EmptyName", abi),
        )
        .unwrap();
        let cargo = |verb, manifest: &std::path::Path, target: &str| {
            Command::new("cargo")
                .args([verb, "--offline", "--manifest-path"])
                .arg(manifest)
                .env("CARGO_TARGET_DIR", root.join(target))
                .output()
                .unwrap()
        };
        assert!(
            cargo(
                "check",
                &root.join("generated/contract/Cargo.toml"),
                "generated-target"
            )
            .status
            .success()
        );
        let manifest = consumer.join("Cargo.toml");
        assert!(cargo("run", &manifest, "consumer-target").status.success());
        fs::write(
            consumer.join("src/main.rs"),
            source(
                "GreetError",
                "MissingName",
                "let _ = GreetError::EmptyName;",
            ),
        )
        .unwrap();
        let stale = cargo("check", &manifest, "consumer-target");
        assert!(!stale.status.success());
        assert!(
            String::from_utf8_lossy(&stale.stderr).contains("Boxology generated contract is stale")
        );
        let renamed = CONTRACT
            .replace(
                "GreetError { EmptyName }",
                "HelloFailure { EmptyName, Busy }",
            )
            .replace("GreetError", "HelloFailure");
        for file in tree(&renamed, false).files() {
            fs::write(root.join(file.path()), file.bytes()).unwrap();
        }
        fs::write(
            consumer.join("src/main.rs"),
            source("HelloFailure", "EmptyName, Busy", "let value = HelloFailure::Busy; assert_eq!(value.error_tag(), \"Busy\"); assert_eq!(HelloFailure::decode_value(&value.encode_value().unwrap()).unwrap(), value); let _: boxology_generated_contract::HelloFailure = value;"),
        )
        .unwrap();
        assert!(cargo("run", &manifest, "consumer-target").status.success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn output_set_fails_closed_with_one_stable_code() {
        let cases = [
            (
                vec![OUTPUTS[0]],
                r#"BXG0039 <request>:1:1-1:1 offending="declared outputs [\"generated/contract/Cargo.toml\"]" rule="declared outputs must equal the generator's complete output set without duplicates" source="specs/s2-contract-generator.md D1""#,
            ),
            (
                vec![OUTPUTS[0], OUTPUTS[1], "generated/extra"],
                r#"BXG0039 <request>:1:1-1:1 offending="declared outputs [\"generated/contract/Cargo.toml\", \"generated/contract/src/lib.rs\", \"generated/extra\"]" rule="declared outputs must equal the generator's complete output set without duplicates" source="specs/s2-contract-generator.md D1""#,
            ),
            (
                vec![OUTPUTS[0], OUTPUTS[1], OUTPUTS[1]],
                r#"BXG0039 <request>:1:1-1:1 offending="declared outputs [\"generated/contract/Cargo.toml\", \"generated/contract/src/lib.rs\", \"generated/contract/src/lib.rs\"]" rule="declared outputs must equal the generator's complete output set without duplicates" source="specs/s2-contract-generator.md D1""#,
            ),
        ];
        for (outputs, expected) in cases {
            let diagnostics = generate(&request(CONTRACT, false, outputs)).unwrap_err();
            assert_eq!(diagnostics.as_slice().len(), 1);
            assert_eq!(diagnostics.as_slice()[0].code(), "BXG0039");
            assert_eq!(diagnostics.to_string(), expected);
        }
    }

    #[test]
    fn generator_inventory_duplicates_fail_closed() {
        let request = request(CONTRACT, false, OUTPUTS.to_vec());
        let diagnostics = request
            .require_exact_outputs(&[OUTPUTS[0], OUTPUTS[1], OUTPUTS[1]])
            .unwrap_err();
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0039");
    }
}
