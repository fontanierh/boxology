//! Pure generation of deterministic Boxology artifacts from validated logical inputs.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract_syntax::{
    CanonicalType, CapabilityDeclaration, Contract, DataDeclaration, DataShape, TypeExpression,
    VariantPayload,
};
use boxology_generator_model::{Diagnostics, GenerationRequest, ImportModel, ParsedRustInputs};

mod schema;

/// Package version pinned in source so generation cannot read the process environment.
const GENERATOR_VERSION: &str = "0.0.0";

/// The generator's complete declared output set, in emission order.
///
/// [`generate`] requires declared outputs to equal this set exactly — as a *set*, so this order is
/// not [`GeneratedTree::files`] order, which is sorted by path bytes. Callers should consume this
/// rather than a drifting copy; today's are tests and a determinism subject. Goldens keep literal
/// copies on purpose: a test pinning the set must not be editable by editing what it pins.
pub const OUTPUTS: [&str; 4] = [
    "generated/contract/Cargo.toml",
    "generated/contract/src/lib.rs",
    "generated/adapter/adapter.rs",
    "generated/schema.json",
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
pub fn generate(request: GenerationRequest) -> Result<GeneratedTree, Diagnostics> {
    request.require_exact_outputs(&OUTPUTS)?;
    let parsed = ParsedRustInputs::parse(&request)?;
    let contract = parsed.controlled_contract()?;
    contract.require_v0_emittable()?;
    // Hydrate and fail closed on any declared import, then thread the returned models into the
    // adapter's implementation descriptor. A box with no imports emits `[]` — the pre-import token
    // — so every existing box stays byte-identical; imports are implementation-local and never
    // affect the outward contract, schema, revision, or semantic digest.
    let imports = ImportModel::parse_all(&request)?;
    let revision = schema::revision(request.box_id().as_str(), contract.model());
    let release_contract = matches!(request.box_id().as_str(), "check" | "classifier");
    let manifest = if release_contract {
        let description = if request.box_id().as_str() == "check" {
            "Generated workspace check contract"
        } else {
            "Generated compatibility classifier contract"
        };
        format!(
            "[package]\nname = \"{}-contract\"\nversion.workspace = true\nedition.workspace = true\nrust-version.workspace = true\nlicense.workspace = true\nrepository.workspace = true\nhomepage.workspace = true\nreadme.workspace = true\ndescription = {description:?}\npublish = true\n\n[features]\ndefault = []\ntest-support = []\n\n[dependencies]\nboxology-contract = {{ version = \"=0.1.0\", path = \"../../../boxology-contract\" }}\n",
            request.box_id().as_str()
        )
    } else {
        format!(
            "[package]\nname = \"{}-contract\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[features]\ndefault = []\ntest-support = []\n\n[dependencies]\nboxology-contract = {{ workspace = true }}\n",
            request.box_id().as_str()
        )
    };
    let data_types = structured_types_source(contract.model());
    let error = &contract.model().error;
    let error_attrs = attributes(&error.docs, &error.deprecation);
    let variants = error
        .variants
        .iter()
        .map(|variant| match &variant.payload {
            VariantPayload::Unit => format!(
                "{}{},",
                attributes(&variant.docs, &variant.deprecation),
                variant.name
            ),
            VariantPayload::Value(value) => format!(
                "{}{}({}{}),",
                attributes(&variant.docs, &variant.deprecation),
                variant.name,
                attributes(&value.docs, &value.deprecation),
                rust_value_type(value.ty, true)
            ),
            VariantPayload::Named(_) => {
                unreachable!("named payloads remain BXG0048-gated and must not reach emission")
            }
        })
        .collect::<String>();
    let encode_arms = error
        .variants
        .iter()
        .map(|variant| match &variant.payload {
            VariantPayload::Unit => format!(
                "Self::{} => ({:?}.into(), ::boxology_contract::SlotValue::Null),",
                variant.name, variant.name
            ),
            VariantPayload::Value(_) => format!(
                "Self::{name}(value) => ({name:?}.into(), value.encode().map_err(|error| error.under(::boxology_contract::PathSegment::Variant({name:?}.into())))?),",
                name = variant.name
            ),
            VariantPayload::Named(_) => {
                unreachable!("named payloads remain BXG0048-gated and must not reach emission")
            }
        })
        .collect::<String>();
    let decode_arms = error
        .variants
        .iter()
        .map(|variant| match &variant.payload {
            VariantPayload::Unit => format!(
                r#"
                {name:?} if matches!(payload, ::boxology_contract::SlotValue::Null) => Ok(Self::{name}),
                {name:?} => Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::UnexpectedPayload).under(::boxology_contract::PathSegment::Variant(tag.into()))),
                "#,
                name = variant.name
            ),
            VariantPayload::Value(value) => format!(
                "{name:?} => {leaf}::decode(payload).map(Self::{name}).map_err(|error| error.under(::boxology_contract::PathSegment::Variant(tag.into()))),",
                name = variant.name,
                leaf = rust_value_type(value.ty, true)
            ),
            VariantPayload::Named(_) => {
                unreachable!("named payloads remain BXG0048-gated and must not reach emission")
            }
        })
        .collect::<String>();
    let tag_arms = error
        .variants
        .iter()
        .map(|variant| match &variant.payload {
            VariantPayload::Unit => format!("Self::{} => {:?},", variant.name, variant.name),
            VariantPayload::Value(_) => {
                format!("Self::{}(..) => {:?},", variant.name, variant.name)
            }
            VariantPayload::Named(_) => {
                unreachable!("named payloads remain BXG0048-gated and must not reach emission")
            }
        })
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
    let checker = checker_source(request.box_id().as_str(), contract.model());
    let descriptor =
        schema::descriptor_source(request.box_id().as_str(), contract.model(), &revision);
    let dispatch = dispatch_source(request.box_id().as_str(), contract.model());
    let test_support = test_support_source(request.box_id().as_str(), contract.model());
    let adapter = adapter_source(request.box_id().as_str(), contract.model(), &imports);
    let syntax = syn::parse_file(&format!(
        "{descriptor} {dispatch} {data_types} {error_attrs}#[derive(Debug, Clone, PartialEq)] pub enum {} {{{variants} Unknown {{ tag: ::std::string::String, payload: ::boxology_contract::OpaquePayload }}}} {error_abi} {test_support} #[doc(hidden)] pub const __BOXOLOGY_SEMANTIC_DIGEST: [u8; 32] = [{digest}]; {checker}",
        error.name
    ))
    .expect("validated names and fixed generator template must parse");
    let rust = format!(
        "// Generated by boxology-generator {}\n{}",
        GENERATOR_VERSION,
        prettyplease::unparse(&syntax)
    );
    let adapter_syntax =
        syn::parse_file(&adapter).expect("validated names and fixed adapter template must parse");
    let adapter_rust = format!(
        "// Generated by boxology-generator {}\n{}",
        GENERATOR_VERSION,
        prettyplease::unparse(&adapter_syntax)
    );
    let schema = schema::document(
        request.box_id().as_str(),
        contract.model(),
        &revision,
        contract.semantic_digest(),
        GENERATOR_VERSION,
    );
    let mut files = OUTPUTS
        .iter()
        .zip([
            manifest.into_bytes(),
            rust.into_bytes(),
            adapter_rust.into_bytes(),
            schema,
        ])
        .map(|(path, bytes)| GeneratedFile {
            path: (*path).to_owned(),
            bytes,
        })
        .collect::<Vec<_>>();
    if release_contract {
        files.extend([
            GeneratedFile {
                path: "generated/contract/LICENSE-APACHE".into(),
                bytes: include_bytes!("../LICENSE-APACHE").to_vec(),
            },
            GeneratedFile {
                path: "generated/contract/LICENSE-MIT".into(),
                bytes: include_bytes!("../LICENSE-MIT").to_vec(),
            },
        ]);
    }
    files.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(GeneratedTree(files))
}

/// Emits the public definitions and exact transport-neutral codecs for the accepted structured
/// declaration subset. `require_v0_emittable` still gates residual `Blob` and named error payload
/// shapes before this template runs.
fn structured_types_source(contract: &Contract) -> String {
    contract.data.iter().map(structured_type_source).collect()
}

fn structured_type_source(declaration: &DataDeclaration) -> String {
    let attrs = attributes(&declaration.docs, &declaration.deprecation);
    let name = &declaration.name;
    match &declaration.shape {
        DataShape::Struct(fields) => {
            let definitions = fields
                .iter()
                .map(|field| {
                    format!(
                        "{}pub {}: {},",
                        attributes(&field.docs, &field.deprecation),
                        field.name,
                        rust_type_expression(&field.ty, "", true)
                    )
                })
                .collect::<String>();
            let encoders = fields
                .iter()
                .map(|field| format!(
                    "if let Some(value) = ::boxology_contract::ContractType::encode_field(&self.{field}).map_err(|error| error.under(::boxology_contract::PathSegment::Field({field:?}.into())))? {{ fields.push(({field:?}.into(), value)); }}",
                    field = field.name
                ))
                .collect::<String>();
            let known_fields = fields
                .iter()
                .map(|field| format!("{:?}", field.name))
                .collect::<Vec<_>>()
                .join(" | ");
            let known_arm = if known_fields.is_empty() {
                String::new()
            } else {
                format!("{known_fields} => {{}},")
            };
            let unknown_field = "return Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::UnknownField(field.into())).under(::boxology_contract::PathSegment::Field(field.into())))";
            let field_validation = if fields.is_empty() {
                format!("if let Some((field, _)) = fields.entries().next() {{ {unknown_field}; }}")
            } else {
                format!(
                    "for (field, _) in fields.entries() {{ match field {{ {known_arm}_ => {unknown_field} }} }}"
                )
            };
            let decoders = fields
                .iter()
                .map(|field| format!(
                    "{field}: <{ty} as ::boxology_contract::ContractType>::decode_field(fields.get({field:?})).map_err(|error| error.under(::boxology_contract::PathSegment::Field({field:?}.into())))?,",
                    field = field.name,
                    ty = rust_type_expression(&field.ty, "", true)
                ))
                .collect::<String>();
            let field_binding = if fields.is_empty() {
                "let fields = ::std::vec::Vec::new();"
            } else {
                "let mut fields = ::std::vec::Vec::new();"
            };
            format!(
                r#"
                {attrs}#[derive(Debug, Clone, PartialEq)] pub struct {name} {{ {definitions} }}
                impl ::boxology_contract::ContractType for {name} {{
                    fn encode_value(&self) -> ::core::result::Result<::boxology_contract::ContractValue, ::boxology_contract::EncodeError> {{
                        {field_binding}
                        {encoders}
                        ::boxology_contract::ContractValue::object(fields).map_err(|_| unreachable!("validated generated field identities are unique"))
                    }}
                    fn decode_value(value: &::boxology_contract::ContractValue) -> ::core::result::Result<Self, ::boxology_contract::DecodeError> {{
                        let ::boxology_contract::ValueRef::Object(fields) = value.view() else {{
                            return Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::KindMismatch));
                        }};
                        {field_validation}
                        Ok(Self {{ {decoders} }})
                    }}
                }}
            "#
            )
        }
        DataShape::Enum(variants) => {
            let definitions = variants
                .iter()
                .map(|variant| {
                    format!(
                        "{}{},",
                        attributes(&variant.docs, &variant.deprecation),
                        variant.name
                    )
                })
                .collect::<String>();
            let encoders = variants
                .iter()
                .map(|variant| {
                    format!(
                        "Self::{name} => ({name:?}.into(), ::boxology_contract::SlotValue::Null),",
                        name = variant.name
                    )
                })
                .collect::<String>();
            let decoders = variants
                .iter()
                .map(|variant| format!(r#"
                    {name:?} if matches!(payload, ::boxology_contract::SlotValue::Null) => Ok(Self::{name}),
                    {name:?} => Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::UnexpectedPayload).under(::boxology_contract::PathSegment::Variant(tag.into()))),
                "#, name = variant.name))
                .collect::<String>();
            format!(
                r#"
                {attrs}#[derive(Debug, Clone, PartialEq)] pub enum {name} {{ {definitions} Unknown {{ tag: ::std::string::String, payload: ::boxology_contract::OpaquePayload }} }}
                impl ::boxology_contract::ContractType for {name} {{
                    fn encode_value(&self) -> ::core::result::Result<::boxology_contract::ContractValue, ::boxology_contract::EncodeError> {{
                        let (tag, payload) = match self {{
                            {encoders}
                            Self::Unknown {{ tag, payload }} => (tag.clone(), ::boxology_contract::SlotValue::Value(::boxology_contract::ContractValue::opaque(payload.forward()))),
                        }};
                        Ok(::boxology_contract::ContractValue::enum_value(tag, payload))
                    }}
                    fn decode_value(value: &::boxology_contract::ContractValue) -> ::core::result::Result<Self, ::boxology_contract::DecodeError> {{
                        let ::boxology_contract::ValueRef::Enum {{ tag, payload }} = value.view() else {{
                            return Err(::boxology_contract::DecodeError::new(::boxology_contract::DecodeErrorKind::KindMismatch));
                        }};
                        match tag {{
                            {decoders}
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
            "#
            )
        }
    }
}

fn rust_type_expression(
    expression: &TypeExpression,
    local_prefix: &str,
    qualified_leaves: bool,
) -> String {
    match expression {
        TypeExpression::Leaf(leaf) => rust_value_type(*leaf, qualified_leaves).into(),
        TypeExpression::Local(name) => format!("{local_prefix}{name}"),
        TypeExpression::Option(inner) => {
            format!(
                "::core::option::Option<{}>",
                rust_type_expression(inner, local_prefix, qualified_leaves)
            )
        }
        TypeExpression::Vec(inner) => {
            format!(
                "::std::vec::Vec<{}>",
                rust_type_expression(inner, local_prefix, qualified_leaves)
            )
        }
    }
}

/// Emits a decode call without changing the frozen scalar spelling. Structured values use UFCS so
/// nested generic and qualified local paths are unambiguous at every generated site.
fn decode_call(
    expression: &TypeExpression,
    rust_type: &str,
    contract_type: &str,
    value: &str,
) -> String {
    if expression.leaf().is_some() {
        format!("{rust_type}::decode({value})")
    } else {
        format!("<{rust_type} as {contract_type}>::decode({value})")
    }
}

/// Emits the generated implementation-checker `macro_rules! __boxology_check_implementation`.
///
/// The macro validates a user's `#[boxology::implementation]` impl against the contract and emits
/// the `impl HelloDispatch` bridge. At a single capability it reproduces the pre-generalization
/// macro byte-for-byte so the Hello golden stays pinned. Beyond one capability it emits one disjoint
/// `@find_{capability}` recursion per capability plus a single combined `impl HelloDispatch` — N
/// separate impls of the same trait for the same receiver would be a coherence conflict (E0119).
/// The multi-capability shape is exercised both directly and end-to-end through `generate()`, which
/// now emits contracts holding any number of capabilities.
fn checker_source(box_id: &str, contract: &Contract) -> String {
    let prefix = pascal_case(box_id);
    let error = &contract.error;
    if contract.capabilities.len() == 1 {
        let capability = &contract.capabilities[0];
        return r#"
        #[doc(hidden)]
        #[macro_export]
        macro_rules! __boxology_check_implementation {
            ($receiver:ty; $($method:ident $validity:ident;)*) => {
                $crate::__boxology_check_implementation!(@find $receiver; $($method $validity;)*);
            };
            (@find $receiver:ty; __CAPABILITY__ valid; $($rest:tt)*) => {
                const _: () = {
                    fn require_service<T: ::core::marker::Send + ::core::marker::Sync + 'static>() {}
                    fn require_future<F: ::core::future::Future<Output = ::core::result::Result<__OUTPUT_TY__, $crate::__ERROR__>> + ::core::marker::Send>(_: F) {}
                    fn check(receiver: &$receiver, context: ::boxology::CallContext, input: __INPUT_TY__) {
                        require_service::<$receiver>();
                        require_future(receiver.__CAPABILITY__(context, input));
                    }
                };
                impl $crate::__DISPATCH__ for $receiver {
                    fn __CAPABILITY__<'a>(
                        &'a self,
                        context: ::boxology::CallContext,
                        input: __INPUT_TY__,
                    ) -> ::std::pin::Pin<
                        ::std::boxed::Box<
                            dyn ::core::future::Future<
                                    Output = ::core::result::Result<
                                        __OUTPUT_TY__,
                                        $crate::__ERROR__,
                                    >,
                                > + ::core::marker::Send
                                + 'a,
                        >,
                    > {
                        ::std::boxed::Box::pin(self.__CAPABILITY__(context, input))
                    }
                }
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
        .replace("__CAPABILITY__", &capability.name)
        .replace("__ERROR__", &error.name)
        .replace(
            "__INPUT_TY__",
            &rust_type_expression(&capability.input_type, "$crate::", true),
        )
        .replace(
            "__OUTPUT_TY__",
            &rust_type_expression(&capability.output_type, "$crate::", true),
        )
        .replace("__DISPATCH__", &format!("{prefix}Dispatch"));
    }
    // Beyond one capability the macro body is almost entirely braces, so it is assembled by
    // `.replace()` on raw templates rather than `format!` (whose `{{`/`}}` escaping would be
    // error-prone). `$receiver`/`$method`/`$validity`/`$other`/`$rest` stay literal macro
    // metavariables in the templates; only the boundary placeholders are substituted.
    let error_name = &error.name;
    let per_capability = |template: &str, capability: &CapabilityDeclaration| -> String {
        template
            .replace("__CAP__", &capability.name)
            .replace(
                "__INPUT_TY__",
                &rust_type_expression(&capability.input_type, "$crate::", true),
            )
            .replace(
                "__OUTPUT_TY__",
                &rust_type_expression(&capability.output_type, "$crate::", true),
            )
            .replace("__ERROR__", error_name)
    };
    // One `@find_{capability}` invocation per capability, all forwarded the full method list.
    let invocations = contract
        .capabilities
        .iter()
        .map(|capability| {
            per_capability(
                "                $crate::__boxology_check_implementation!(@find___CAP__ $receiver; $($method $validity;)*);\n",
                capability,
            )
        })
        .collect::<String>();
    // One bridge method per capability inside the single combined `impl HelloDispatch`.
    let bridges = contract
        .capabilities
        .iter()
        .map(|capability| {
            per_capability(
                r#"                    fn __CAP__<'a>(
                        &'a self,
                        context: ::boxology::CallContext,
                        input: __INPUT_TY__,
                    ) -> ::std::pin::Pin<
                        ::std::boxed::Box<
                            dyn ::core::future::Future<
                                    Output = ::core::result::Result<__OUTPUT_TY__, $crate::__ERROR__>,
                                > + ::core::marker::Send
                                + 'a,
                        >,
                    > {
                        ::std::boxed::Box::pin(self.__CAP__(context, input))
                    }
"#,
                capability,
            )
        })
        .collect::<String>();
    // Four disjoint `@find_{capability}` arms per capability: valid, invalid, recurse-skip, missing.
    let arms = contract
        .capabilities
        .iter()
        .map(|capability| {
            per_capability(
                r#"            (@find___CAP__ $receiver:ty; __CAP__ valid; $($rest:tt)*) => {
                const _: () = {
                    fn require_service<T: ::core::marker::Send + ::core::marker::Sync + 'static>() {}
                    fn require_future<F: ::core::future::Future<Output = ::core::result::Result<__OUTPUT_TY__, $crate::__ERROR__>> + ::core::marker::Send>(_: F) {}
                    fn check(receiver: &$receiver, context: ::boxology::CallContext, input: __INPUT_TY__) {
                        require_service::<$receiver>();
                        require_future(receiver.__CAP__(context, input));
                    }
                };
            };
            (@find___CAP__ $receiver:ty; __CAP__ invalid; $($rest:tt)*) => {
                compile_error!("Boxology capability has an invalid structural signature");
            };
            (@find___CAP__ $receiver:ty; $other:ident $validity:ident; $($rest:tt)*) => {
                $crate::__boxology_check_implementation!(@find___CAP__ $receiver; $($rest)*);
            };
            (@find___CAP__ $receiver:ty;) => {
                compile_error!("Boxology capability implementation is missing");
            };
"#,
                capability,
            )
        })
        .collect::<String>();
    r#"
        #[doc(hidden)]
        #[macro_export]
        macro_rules! __boxology_check_implementation {
            ($receiver:ty; $($method:ident $validity:ident;)*) => {
__INVOCATIONS__                impl $crate::__DISPATCH__ for $receiver {
__BRIDGES__                }
            };
__ARMS__        }
    "#
    .replace("__INVOCATIONS__", &invocations)
    .replace("__BRIDGES__", &bridges)
    .replace("__ARMS__", &arms)
    .replace("__DISPATCH__", &format!("{prefix}Dispatch"))
}

fn test_support_source(box_id: &str, contract: &Contract) -> String {
    let prefix = pascal_case(box_id);
    let error_name = &contract.error.name;
    let routing_statics_csv = contract
        .capabilities
        .iter()
        .map(|capability| capability_static_name(box_id, &capability.name))
        .collect::<Vec<_>>()
        .join(", ");
    let type_aliases = contract
        .capabilities
        .iter()
        .map(|capability| {
            format!(
                r#"            type {pascal}Future =
                Pin<Box<dyn Future<Output = Result<{output_bare}, {error_name}>> + Send + 'static>>;
            type {pascal}Responder =
                dyn Fn(CallContext, {input_bare}) -> {pascal}Future + Send + Sync + 'static;
"#,
                pascal = pascal_case(&capability.name),
                output_bare = rust_type_expression(&capability.output_type, "super::", false),
                error_name = error_name,
                input_bare = rust_type_expression(&capability.input_type, "super::", false),
            )
        })
        .collect::<String>();
    let struct_fields = contract
        .capabilities
        .iter()
        .map(|capability| {
            format!(
                "                {name}: Option<Arc<{pascal}Responder>>,\n",
                name = capability.name,
                pascal = pascal_case(&capability.name),
            )
        })
        .collect::<String>();
    let builders = contract
        .capabilities
        .iter()
        .map(|capability| {
            format!(
                r#"                pub fn with_{name}<F, Fut>(mut self, responder: F) -> Self
                where
                    F: Fn(CallContext, {input_bare}) -> Fut + Send + Sync + 'static,
                    Fut: Future<Output = Result<{output_bare}, {error_name}>> + Send + 'static,
                {{
                    self.{name} = Some(Arc::new(move |context, {input_name}| {{
                        Box::pin(responder(context, {input_name}))
                    }}));
                    self
                }}

"#,
                name = capability.name,
                input_bare = rust_type_expression(&capability.input_type, "super::", false),
                output_bare = rust_type_expression(&capability.output_type, "super::", false),
                error_name = error_name,
                input_name = capability.input_name,
            )
        })
        .collect::<String>();
    // The per-capability decode/dispatch/encode body is identical between the single- and
    // multi-capability `call` shapes; only the routing envelope around it differs.
    let async_body = |capability: &CapabilityDeclaration| -> String {
        let input_bare = rust_type_expression(&capability.input_type, "super::", false);
        let input_decode = decode_call(
            &capability.input_type,
            &input_bare,
            "ContractType",
            "&input",
        );
        format!(
            r#"Box::pin(async move {{
                let input = {input_descriptor}
                    .conform(DecodeRole::ProviderInput, input)
                    .map_err(|error| {{
                        ErasedCallError::ContractViolation(conversion_detail("input_decode", error))
                    }})?;
                let {input_name} = {input_decode}.map_err(|error| {{
                    ErasedCallError::ContractViolation(conversion_detail("input_decode", error))
                }})?;
                match responder(context, {input_name}).await {{
                    Ok(output) => output.encode().map_err(|error| {{
                        ErasedCallError::InvalidResponse(conversion_detail("output_encode", error))
                    }}),
                    Err(error) => Err(ErasedCallError::from_domain(&error)),
                }}
            }})"#,
            input_descriptor = schema::type_descriptor_source(contract, &capability.input_type, ""),
            input_name = capability.input_name,
            input_decode = input_decode,
        )
    };
    // At a single capability the fake keeps today's exact routing so the Hello golden stays
    // byte-identical; beyond one it matches each capability's routing static in source order and
    // falls through to `unprogrammed`.
    let call_body = if contract.capabilities.len() == 1 {
        let capability = &contract.capabilities[0];
        format!(
            r#"if capability != &*{static_name} {{
                    return Box::pin(ready(Err(unprogrammed())));
                }}
                let Some(responder) = self.{name}.clone() else {{
                    return Box::pin(ready(Err(unprogrammed())));
                }};
                {async_body}"#,
            static_name = capability_static_name(box_id, &capability.name),
            name = capability.name,
            async_body = async_body(capability),
        )
    } else {
        let branches = contract
            .capabilities
            .iter()
            .map(|capability| {
                format!(
                    r#"if capability == &*{static_name} {{
                    let Some(responder) = self.{name}.clone() else {{
                        return Box::pin(ready(Err(unprogrammed())));
                    }};
                    return {async_body};
                }}
                "#,
                    static_name = capability_static_name(box_id, &capability.name),
                    name = capability.name,
                    async_body = async_body(capability),
                )
            })
            .collect::<String>();
        format!("{branches}Box::pin(ready(Err(unprogrammed())))")
    };
    format!(
        r#"
        #[cfg(feature = "test-support")]
        pub mod test_support {{
            use std::future::{{Future, ready}};
            use std::pin::Pin;
            use std::sync::Arc;

            use ::boxology_contract::{{
                CallContext, CapabilityId, ContractType, DecodeRole, Detail, ErasedCallError,
                ErasedCallTarget, SlotValue, TypeDescriptor,
            }};

            use super::{{{error_name}, {routing_statics_csv}, {prefix}Handle, conversion_detail}};

{type_aliases}
            #[derive(Clone, Default)]
            pub struct {prefix}Fake {{
{struct_fields}            }}

            impl {prefix}Fake {{
                pub fn new() -> Self {{
                    Self::default()
                }}

{builders}                pub fn handle(&self) -> {prefix}Handle {{
                    {prefix}Handle::from_erased(Arc::new(self.clone()))
                }}
            }}

            impl ErasedCallTarget for {prefix}Fake {{
                fn call<'a>(
                    &'a self,
                    capability: &'a CapabilityId,
                    context: CallContext,
                    input: SlotValue,
                ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>
                {{
                    {call_body}
                }}
            }}

            fn unprogrammed() -> ErasedCallError {{
                ErasedCallError::Internal(Detail::new("unprogrammed_capability"))
            }}
        }}
        "#,
        prefix = prefix,
        error_name = error_name,
        routing_statics_csv = routing_statics_csv,
        type_aliases = type_aliases,
        struct_fields = struct_fields,
        builders = builders,
        call_body = call_body,
    )
}

fn adapter_source(
    box_id: &str,
    contract: &Contract,
    imports: &[boxology_generator_model::ImportModel],
) -> String {
    let prefix = pascal_case(box_id);
    // The per-capability decode/dispatch/encode body is identical between the single- and
    // multi-capability `call` shapes; only the routing envelope around it differs.
    let async_body = |capability: &CapabilityDeclaration| -> String {
        let input_qualified = rust_type_expression(
            &capability.input_type,
            "::boxology_generated_contract::",
            true,
        );
        let input_decode = decode_call(
            &capability.input_type,
            &input_qualified,
            "::boxology_contract::ContractType",
            "&input",
        );
        format!(
            r#"let input = {input_descriptor}
                        .conform(
                            ::boxology_contract::DecodeRole::ProviderInput,
                            input,
                        )
                        .map_err(|error| {{
                            ::boxology_contract::ErasedCallError::ContractViolation(
                                conversion_detail("input_decode", error),
                            )
                        }})?;
                    let input = {input_decode}.map_err(|error| {{
                        ::boxology_contract::ErasedCallError::ContractViolation(
                            conversion_detail("input_decode", error),
                        )
                    }})?;
                    match ::boxology_generated_contract::{prefix}Dispatch::{capability_name}(
                        &self.service,
                        context,
                        input,
                    )
                    .await
                    {{
                        Ok(output) => output.encode().map_err(|error| {{
                            ::boxology_contract::ErasedCallError::InvalidResponse(
                                conversion_detail("output_encode", error),
                            )
                        }}),
                        Err(error) => Err(::boxology_contract::ErasedCallError::from_domain(
                            &error,
                        )),
                    }}"#,
            prefix = prefix,
            input_descriptor = schema::type_descriptor_source(
                contract,
                &capability.input_type,
                "::boxology_contract::",
            ),
            input_decode = input_decode,
            capability_name = capability.name,
        )
    };
    // At a single capability the adapter keeps today's exact routing so the Hello golden stays
    // byte-identical; beyond one it matches each capability's descriptor id in source order and
    // falls through to `unknown_capability`.
    let call_body = if contract.capabilities.len() == 1 {
        let capability = &contract.capabilities[0];
        format!(
            r#"let expected = ::boxology_generated_contract::contract_descriptor()
                    .capabilities()
                    .first()
                    .expect("generated {prefix} contract has one capability")
                    .id();
                if capability != expected {{
                    return Box::pin(::std::future::ready(Err(unknown_capability())));
                }}
                Box::pin(async move {{
                    {async_body}
                }})"#,
            prefix = prefix,
            async_body = async_body(capability),
        )
    } else {
        let branches = contract
            .capabilities
            .iter()
            .enumerate()
            .map(|(index, capability)| {
                format!(
                    r#"if capability == capabilities[{index}].id() {{
                    return Box::pin(async move {{
                        {async_body}
                    }});
                }}
                "#,
                    index = index,
                    async_body = async_body(capability),
                )
            })
            .collect::<String>();
        format!(
            r#"let capabilities = ::boxology_generated_contract::contract_descriptor().capabilities();
                {branches}Box::pin(::std::future::ready(Err(unknown_capability())))"#,
        )
    };
    // Zero imports emit the bare `[]` token — token-identical to the pre-import adapter, so the
    // prettyplease output stays byte-identical for every existing box. One or more imports build a
    // `[ ImportDescriptor::new(...), ... ]` literal preserving import and capability order.
    let import_list = import_descriptors(imports);
    // Purely additive: emitted only when the box has >=1 import, so zero-import boxes stay
    // byte-identical. `factory` and `{prefix}Adapter` are unchanged.
    let typed_imports = typed_imports_source(box_id, imports);
    let register = if imports.is_empty() {
        format!(
            r#"pub fn register<T>(
            composition: &mut ::boxology_runtime::CompositionBuilder,
            service: T,
        ) -> ::boxology_runtime::RegisteredBox
        where
            T: ::boxology_generated_contract::{prefix}Dispatch + Send + Sync + 'static,
        {{
            composition.register(implementation_descriptor(), move |imports| {{
                factory(service, imports)
            }})
        }}"#,
        )
    } else {
        format!(
            r#"pub fn register<T, F>(
            composition: &mut ::boxology_runtime::CompositionBuilder,
            build: F,
        ) -> ::boxology_runtime::RegisteredBox
        where
            T: ::boxology_generated_contract::{prefix}Dispatch + Send + Sync + 'static,
            F: FnOnce({prefix}Imports) -> T,
        {{
            composition.register(implementation_descriptor(), move |imports| {{
                let typed = typed_imports(&imports);
                factory(build(typed), imports)
            }})
        }}"#,
        )
    };
    format!(
        r#"
        use ::boxology_contract::ContractType;

        #[doc(hidden)]
        pub fn implementation_descriptor() -> ::boxology_contract::ImplementationDescriptor {{
            ::boxology_contract::ImplementationDescriptor::new(
                ::boxology_generated_contract::contract_descriptor(),
                {import_list},
            )
            .expect("generated adapter import descriptors are valid")
        }}

        #[doc(hidden)]
        pub struct {prefix}Adapter<T> {{
            service: T,
            _imports: ::boxology_runtime::Imports,
        }}

        #[doc(hidden)]
        pub fn factory<T>(
            service: T,
            imports: ::boxology_runtime::Imports,
        ) -> {prefix}Adapter<T>
        where
            T: ::boxology_generated_contract::{prefix}Dispatch + Send + Sync + 'static,
        {{
            {prefix}Adapter {{
                service,
                _imports: imports,
            }}
        }}

        {register}

        {typed_imports}

        impl<T> ::boxology_contract::ErasedTarget for {prefix}Adapter<T>
        where
            T: ::boxology_generated_contract::{prefix}Dispatch + Send + Sync + 'static,
        {{
            fn call<'a>(
                &'a self,
                capability: &'a ::boxology_contract::CapabilityId,
                context: ::boxology_contract::CallContext,
                input: ::boxology_contract::SlotValue,
            ) -> ::std::pin::Pin<
                Box<
                    dyn ::std::future::Future<
                            Output = Result<
                                ::boxology_contract::SlotValue,
                                ::boxology_contract::ErasedCallError,
                            >,
                        > + Send
                        + 'a,
                >,
            > {{
                {call_body}
            }}
        }}

        fn conversion_detail(
            code: &'static str,
            error: impl ::std::fmt::Display,
        ) -> ::boxology_contract::Detail {{
            ::boxology_contract::Detail::new(code).with_message(error.to_string())
        }}

        fn unknown_capability() -> ::boxology_contract::ErasedCallError {{
            ::boxology_contract::ErasedCallError::Internal(
                ::boxology_contract::Detail::new("unknown_capability"),
            )
        }}
        "#,
        prefix = prefix,
        call_body = call_body,
        import_list = import_list,
        typed_imports = typed_imports,
    )
}

/// Spells the adapter's `ImplementationDescriptor` import list from the hydrated import models.
///
/// Zero imports return the bare `[]` token so the generated adapter stays byte-identical to the
/// pre-import output for every existing box. One or more imports return a `[ ImportDescriptor::new(
/// package, revision, [ CapabilityId, ... ]), ... ]` literal that preserves import order and, within
/// each import, capability order. Imports are implementation-local: they land only in the adapter,
/// never in the outward contract, schema, revision, or semantic digest.
fn import_descriptors(imports: &[boxology_generator_model::ImportModel]) -> String {
    if imports.is_empty() {
        return "[]".to_owned();
    }
    let entries = imports
        .iter()
        .map(|import| {
            let package = import.package().as_str();
            let capabilities = import
                .capabilities()
                .iter()
                .map(|capability| {
                    format!(
                        "::boxology_contract::CapabilityId::new(::boxology_contract::BoxId::new({package:?}).expect(\"generated import package id is valid\"), ::boxology_contract::CapabilityName::new({name:?}).expect(\"generated import capability name is valid\"))",
                        package = package,
                        name = capability.name(),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "::boxology_contract::ImportDescriptor::new(::boxology_contract::BoxId::new({package:?}).expect(\"generated import package id is valid\"), ::boxology_contract::ContractRevision::new({revision:?}).expect(\"generated import revision is valid\"), [{capabilities}]).expect(\"generated import descriptor is valid\")",
                package = package,
                revision = import.expected_revision(),
                capabilities = capabilities,
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{entries}]")
}

/// Emits the adapter's typed import surface: a `{BPascal}Import` wrapper per imported box, a
/// `{APascal}Imports` bundle, and a `typed_imports(&Imports)` converter, so box A can call an
/// imported capability with typed I/O from the provider contract crate; foreign errors stay
/// erased as `ErasedCallError`.
///
/// Zero imports return `String::new()`: the adapter is reprinted through prettyplease from a parsed
/// AST, so an empty interpolation is byte-identical to the pre-import output — the same mechanism
/// `import_descriptors` uses for its bare `[]`. The surface is purely additive; imports are
/// implementation-local and land only in the adapter, never in the outward contract, schema,
/// revision, or semantic digest.
///
/// The bundle field name maps the box_id `-` to `_`; the map is injective because `_` is outside the
/// `[a-z][a-z0-9-]*` box-id grammar (identity.rs). Adversarial identifiers — a Rust-keyword box id,
/// or a digit-boundary pascal collision such as `a-1b` and `a1b` both pascal-casing to `A1b` — fail
/// closed loudly: the bad adapter fails `syn::parse_file` or produces a duplicate-definition rustc
/// error. A generator-side diagnostic is deferred, mirroring the routing-static precedent at
/// `capability_static_name`.
fn typed_imports_source(box_id: &str, imports: &[boxology_generator_model::ImportModel]) -> String {
    if imports.is_empty() {
        return String::new();
    }
    let prefix = pascal_case(box_id);
    let wrappers = imports
        .iter()
        .map(|import| {
            let package = import.package().as_str();
            let import_prefix = pascal_case(package);
            let methods = import
                .capabilities()
                .iter()
                .map(|capability| {
                    format!(
                        r#"            pub async fn {name}(
                &self,
                context: ::boxology_contract::CallContext,
                input: {input_qualified},
            ) -> Result<{output_qualified}, ::boxology_contract::ErasedCallError> {{
                let capability = ::boxology_contract::CapabilityId::new(
                    ::boxology_contract::BoxId::new({package:?})
                        .expect("generated import package id is valid"),
                    ::boxology_contract::CapabilityName::new({name:?})
                        .expect("generated import capability name is valid"),
                );
                let input = input.encode().map_err(|error| {{
                    ::boxology_contract::ErasedCallError::ContractViolation(
                        conversion_detail("input_encode", error),
                    )
                }})?;
                let output = self.handle.call(&capability, context, input).await?;
                let output = {output_descriptor}
                    .conform(::boxology_contract::DecodeRole::ConsumerOutput, output)
                    .map_err(|error| {{
                        ::boxology_contract::ErasedCallError::InvalidResponse(
                            conversion_detail("output_decode", error),
                        )
                    }})?;
                {output_decode}.map_err(|error| {{
                    ::boxology_contract::ErasedCallError::InvalidResponse(
                        conversion_detail("output_decode", error),
                    )
                }})
            }}
"#,
                        name = capability.name(),
                        input_qualified = imported_rust_type(import, capability.input_type()),
                        output_qualified = imported_rust_type(import, capability.output_type()),
                        output_descriptor =
                            imported_type_descriptor_source(import, capability.output_type(),),
                        output_decode = imported_decode_call(
                            capability.output_type(),
                            &imported_rust_type(import, capability.output_type()),
                        ),
                        package = package,
                    )
                })
                .collect::<String>();
            format!(
                r#"        pub struct {import_prefix}Import {{
            handle: ::boxology_runtime::ImportHandle,
        }}

        impl {import_prefix}Import {{
{methods}        }}

"#,
                import_prefix = import_prefix,
                methods = methods,
            )
        })
        .collect::<String>();
    let fields = imports
        .iter()
        .map(|import| {
            format!(
                "            pub {field}: {import_prefix}Import,\n",
                field = import.package().as_str().replace('-', "_"),
                import_prefix = pascal_case(import.package().as_str()),
            )
        })
        .collect::<String>();
    let conversions = imports
        .iter()
        .map(|import| {
            format!(
                r#"                {field}: {import_prefix}Import {{
                    handle: imports
                        .handle(
                            &::boxology_contract::BoxId::new({package:?})
                                .expect("generated import package id is valid"),
                        )
                        .expect("declared import handle is present")
                        .clone(),
                }},
"#,
                field = import.package().as_str().replace('-', "_"),
                import_prefix = pascal_case(import.package().as_str()),
                package = import.package().as_str(),
            )
        })
        .collect::<String>();
    format!(
        r#"
{wrappers}        pub struct {prefix}Imports {{
{fields}        }}

        pub fn typed_imports(imports: &::boxology_runtime::Imports) -> {prefix}Imports {{
            {prefix}Imports {{
{conversions}            }}
        }}
"#,
        wrappers = wrappers,
        prefix = prefix,
        fields = fields,
        conversions = conversions,
    )
}

/// Spells imported locals through their deterministic implementation dependency alias. Provider
/// declarations remain owned by the provider contract crate and are never copied into the adapter.
fn imported_rust_type(
    import: &boxology_generator_model::ImportModel,
    expression: &TypeExpression,
) -> String {
    match expression {
        TypeExpression::Leaf(leaf) => rust_value_type(*leaf, true).into(),
        TypeExpression::Local(name) => format!(
            "::boxology_import_{}::{name}",
            import.package().as_str().replace('-', "_")
        ),
        TypeExpression::Option(inner) => format!(
            "::core::option::Option<{}>",
            imported_rust_type(import, inner)
        ),
        TypeExpression::Vec(inner) => {
            format!("::std::vec::Vec<{}>", imported_rust_type(import, inner))
        }
    }
}

fn imported_decode_call(expression: &TypeExpression, rust_type: &str) -> String {
    if expression.leaf().is_some() {
        format!("{rust_type}::decode(&output)")
    } else {
        format!("<{rust_type} as ::boxology_contract::ContractType>::decode(&output)")
    }
}

/// Lowers a hydrated provider expression to the structural descriptor used to conform responses.
fn imported_type_descriptor_source(
    import: &boxology_generator_model::ImportModel,
    expression: &TypeExpression,
) -> String {
    match expression {
        TypeExpression::Leaf(leaf) => format!(
            "::boxology_contract::TypeDescriptor::{}()",
            schema::descriptor_constructor(*leaf)
        ),
        TypeExpression::Local(name) => {
            let declaration = import
                .declarations()
                .iter()
                .find(|declaration| declaration.name == *name)
                .expect("strict imported local names a provider declaration");
            match &declaration.shape {
                DataShape::Struct(fields) => {
                    let fields = fields
                        .iter()
                        .map(|field| format!(
                            "::boxology_contract::FieldDescriptor::new({name:?}, {descriptor}, {deprecation}),",
                            name = field.name,
                            descriptor = imported_type_descriptor_source(import, &field.ty),
                            deprecation = import_deprecation(&field.deprecation),
                        ))
                        .collect::<String>();
                    format!(
                        "::boxology_contract::TypeDescriptor::structure([{fields}]).expect(\"generated imported struct descriptor is valid\")"
                    )
                }
                DataShape::Enum(variants) => {
                    let variants = variants
                        .iter()
                        .map(|variant| format!(
                            "::boxology_contract::VariantDescriptor::new({name:?}, ::boxology_contract::VariantPayload::Unit, {deprecation}),",
                            name = variant.name,
                            deprecation = import_deprecation(&variant.deprecation),
                        ))
                        .collect::<String>();
                    format!(
                        "::boxology_contract::TypeDescriptor::enumeration([{variants}]).expect(\"generated imported enum descriptor is valid\")"
                    )
                }
            }
        }
        TypeExpression::Option(inner) => format!(
            "::boxology_contract::TypeDescriptor::optional({}).expect(\"generated imported optional descriptor is valid\")",
            imported_type_descriptor_source(import, inner)
        ),
        TypeExpression::Vec(inner) => format!(
            "::boxology_contract::TypeDescriptor::list({}).expect(\"generated imported list descriptor is valid\")",
            imported_type_descriptor_source(import, inner)
        ),
    }
}

fn import_deprecation(note: &Option<String>) -> String {
    match note {
        None => "None".into(),
        Some(note) if note.is_empty() => "Some(::boxology_contract::Deprecation::new(None))".into(),
        Some(note) => format!("Some(::boxology_contract::Deprecation::new(Some({note:?}.into())))"),
    }
}

fn dispatch_source(box_id: &str, contract: &Contract) -> String {
    let prefix = pascal_case(box_id);
    let error_name = &contract.error.name;
    let error_static = format!("{}_DESCRIPTOR", screaming_snake(error_name));
    let error_descriptor_expect = format!(
        "generated {} descriptor is valid",
        screaming_snake(error_name).to_lowercase().replace('_', " ")
    );
    let variants = contract
        .error
        .variants
        .iter()
        .map(schema::variant_descriptor_source)
        .collect::<String>();
    let trait_methods = contract
        .capabilities
        .iter()
        .map(|capability| {
            format!(
                r#"            fn {capability_name}<'a>(
                &'a self,
                context: CallContext,
                {input_name}: {input_bare},
            ) -> Pin<Box<dyn Future<Output = Result<{output_bare}, {error_name}>> + Send + 'a>>;
"#,
                capability_name = capability.name,
                input_name = capability.input_name,
                input_bare = rust_type_expression(&capability.input_type, "", false),
                output_bare = rust_type_expression(&capability.output_type, "", false),
                error_name = error_name,
            )
        })
        .collect::<String>();
    let handle_methods = contract
        .capabilities
        .iter()
        .map(|capability| {
            let input_bare = rust_type_expression(&capability.input_type, "", false);
            let output_bare = rust_type_expression(&capability.output_type, "", false);
            let output_decode = decode_call(
                &capability.output_type,
                &output_bare,
                "ContractType",
                "&output",
            );
            format!(
                r#"            pub async fn {capability_name}(
                &self,
                context: CallContext,
                {input_name}: {input_bare},
            ) -> Result<{output_bare}, CallError<{error_name}>> {{
                let input = {input_name}
                    .encode()
                    .map_err(|error| conversion_detail("input_encode", error))
                    .map_err(CallError::ContractViolation)?;
                let output = self
                    .target
                    .call(&{capability_static}, context, input)
                    .await
                    .map_err(|error| error.into_typed::<{error_name}>(&{error_static}))?;
                let output = {output_descriptor}
                    .conform(DecodeRole::ConsumerOutput, output)
                    .map_err(|error| conversion_detail("output_decode", error))
                    .map_err(CallError::InvalidResponse)?;
                {output_decode}
                    .map_err(|error| conversion_detail("output_decode", error))
                    .map_err(CallError::InvalidResponse)
            }}
"#,
                capability_name = capability.name,
                input_name = capability.input_name,
                input_bare = input_bare,
                output_bare = output_bare,
                error_name = error_name,
                capability_static = capability_static_name(box_id, &capability.name),
                error_static = error_static,
                output_descriptor =
                    schema::type_descriptor_source(contract, &capability.output_type, ""),
                output_decode = output_decode,
            )
        })
        .collect::<String>();
    let capability_statics = contract
        .capabilities
        .iter()
        .map(|capability| {
            format!(
                r#"        static {capability_static}: LazyLock<CapabilityId> = LazyLock::new(|| {{
            CapabilityId::new(
                BoxId::new({box_id:?}).expect("generated box identity is valid"),
                CapabilityName::new({capability_name:?})
                    .expect("generated capability name is valid"),
            )
        }});
"#,
                capability_static = capability_static_name(box_id, &capability.name),
                box_id = box_id,
                capability_name = capability.name,
            )
        })
        .collect::<String>();
    format!(
        r#"
        use std::future::Future;
        use std::pin::Pin;
        use std::sync::{{Arc, LazyLock}};
        use ::boxology_contract::{{
            BoxId, CallContext, CallError, CapabilityId, CapabilityName, ContractType,
            DecodeRole, Detail, ErasedCallTarget, TypeDescriptor,
        }};

        pub trait {prefix}Dispatch: Send + Sync + 'static {{
{trait_methods}        }}

        #[derive(Clone)]
        pub struct {prefix}Handle {{
            target: Arc<dyn ErasedCallTarget>,
        }}

        impl {prefix}Handle {{
            #[doc(hidden)]
            pub fn from_erased(target: Arc<dyn ErasedCallTarget>) -> Self {{
                Self {{ target }}
            }}

{handle_methods}        }}

        impl ::boxology_contract::BoxHandle for {prefix}Handle {{
            fn from_erased(target: Arc<dyn ErasedCallTarget>) -> Self {{
                Self::from_erased(target)
            }}
        }}

{capability_statics}
        static {error_static}: LazyLock<TypeDescriptor> = LazyLock::new(|| {{
            TypeDescriptor::enumeration([
                {variants}
            ])
            .expect("{error_descriptor_expect}")
        }});

        fn conversion_detail(code: &'static str, error: impl std::fmt::Display) -> Detail {{
            Detail::new(code).with_message(error.to_string())
        }}
        "#,
        prefix = prefix,
        trait_methods = trait_methods,
        handle_methods = handle_methods,
        capability_statics = capability_statics,
        error_static = error_static,
        error_descriptor_expect = error_descriptor_expect,
        variants = variants,
    )
}

/// Names the per-capability `CapabilityId` routing static as `{BOX}_{CAP}`.
///
/// The box identity is uppercased with `-` mapped to `_` and the capability name is uppercased, so
/// box `hello` capability `greet` yields `HELLO_GREET` — the pre-generalization literal at a single
/// capability — while each capability of a larger box (e.g. `STORE_GET`, `STORE_PUT`) gets its own.
///
/// A routing static name can in principle collide with the shared error-descriptor static
/// (`{ERROR}_DESCRIPTOR`) for adversarial identifiers — e.g. box `error`, capability `descriptor`,
/// error enum `Error`. The accepted decision is that such an adversarial-identifier collision fails
/// closed as a duplicate-definition rustc error in the generated crate — two statics of the same
/// name never compile, so misrouting is impossible and the failure is loud, not silent. A
/// generator-side diagnostic that rejects the collision before emission is deferred as a future
/// hardening; it is not required now.
fn capability_static_name(box_id: &str, cap_name: &str) -> String {
    format!(
        "{}_{}",
        box_id.to_uppercase().replace('-', "_"),
        cap_name.to_uppercase()
    )
}

/// Converts a snake_case or kebab-case identifier to PascalCase for a generated type prefix.
///
/// Each segment split on `_` or `-` has its first character uppercased and the rest kept verbatim,
/// so capability `greet` -> `Greet`, `get_item` -> `GetItem`, and box id `my-box` -> `MyBox`. Box
/// ids follow the `[a-z][a-z0-9-]*` grammar, so `-` must split too; capability names never contain
/// `-`, so their derivation is unchanged.
fn pascal_case(name: &str) -> String {
    name.split(['_', '-'])
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
            }
        })
        .collect()
}

/// Converts a PascalCase identifier to SCREAMING_SNAKE_CASE (e.g. `GreetError` -> `GREET_ERROR`).
fn screaming_snake(ident: &str) -> String {
    let mut out = String::new();
    for (index, ch) in ident.char_indices() {
        if index != 0 && ch.is_uppercase() {
            out.push('_');
        }
        out.extend(ch.to_uppercase());
    }
    out
}

/// Spells a canonical boundary leaf as a Rust value type for a runtime template site.
///
/// Every scalar leaf spells identically bare and qualified (`u32` -> `u32`); only `String` differs
/// (`String` bare, `::std::string::String` qualified). `Blob` never reaches emission because
/// `require_v0_emittable` fails closed on Blob capability boundaries and value-payload leaves, but a
/// spelling is provided for completeness.
fn rust_value_type(leaf: CanonicalType, qualified: bool) -> &'static str {
    match leaf {
        CanonicalType::String if qualified => "::std::string::String",
        CanonicalType::String => "String",
        CanonicalType::Blob if qualified => "::boxology_contract::Blob",
        CanonicalType::Blob => "Blob",
        scalar => scalar.canonical_name(),
    }
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
    use boxology_contract::{BoxId, ContractRevision};
    use boxology_schema::{SchemaDocument, SchemaPayload};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    const MANIFEST: &[u8] = b"schema = 1\nid = \"hello\"\nkind = \"box\"\n";
    const CONTRACT: &str = "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
    const GREETER: &str = "boxology::contract! { #[error] pub enum GreetLoudlyError { Refused } #[capability(exposure=external)] pub async fn greet_loudly(name:String)->Result<String,GreetLoudlyError>; }";
    const CARGO: &[u8] = b"[package]\nname = \"hello-contract\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[features]\ndefault = []\ntest-support = []\n\n[dependencies]\nboxology-contract = { workspace = true }\n";
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
#[cfg(feature = "test-support")]
pub mod test_support {
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::Arc;
    use ::boxology_contract::{
        CallContext, CapabilityId, ContractType, DecodeRole, Detail, ErasedCallError,
        ErasedCallTarget, SlotValue, TypeDescriptor,
    };
    use super::{GreetError, HELLO_GREET, HelloHandle, conversion_detail};
    type GreetFuture = Pin<
        Box<dyn Future<Output = Result<String, GreetError>> + Send + 'static>,
    >;
    type GreetResponder = dyn Fn(
        CallContext,
        String,
    ) -> GreetFuture + Send + Sync + 'static;
    #[derive(Clone, Default)]
    pub struct HelloFake {
        greet: Option<Arc<GreetResponder>>,
    }
    impl HelloFake {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn with_greet<F, Fut>(mut self, responder: F) -> Self
        where
            F: Fn(CallContext, String) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = Result<String, GreetError>> + Send + 'static,
        {
            self.greet = Some(
                Arc::new(move |context, name| { Box::pin(responder(context, name)) }),
            );
            self
        }
        pub fn handle(&self) -> HelloHandle {
            HelloHandle::from_erased(Arc::new(self.clone()))
        }
    }
    impl ErasedCallTarget for HelloFake {
        fn call<'a>(
            &'a self,
            capability: &'a CapabilityId,
            context: CallContext,
            input: SlotValue,
        ) -> Pin<
            Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>,
        > {
            if capability != &*HELLO_GREET {
                return Box::pin(ready(Err(unprogrammed())));
            }
            let Some(responder) = self.greet.clone() else {
                return Box::pin(ready(Err(unprogrammed())));
            };
            Box::pin(async move {
                let input = TypeDescriptor::string()
                    .conform(DecodeRole::ProviderInput, input)
                    .map_err(|error| {
                        ErasedCallError::ContractViolation(
                            conversion_detail("input_decode", error),
                        )
                    })?;
                let name = String::decode(&input)
                    .map_err(|error| {
                        ErasedCallError::ContractViolation(
                            conversion_detail("input_decode", error),
                        )
                    })?;
                match responder(context, name).await {
                    Ok(output) => {
                        output
                            .encode()
                            .map_err(|error| {
                                ErasedCallError::InvalidResponse(
                                    conversion_detail("output_encode", error),
                                )
                            })
                    }
                    Err(error) => Err(ErasedCallError::from_domain(&error)),
                }
            })
        }
    }
    fn unprogrammed() -> ErasedCallError {
        ErasedCallError::Internal(Detail::new("unprogrammed_capability"))
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

    fn request_for(box_id: &str, source: &str) -> GenerationRequest {
        let manifest = format!("schema = 1\nid = \"{box_id}\"\nkind = \"box\"\n");
        GenerationRequest::new(
            BoxId::new(box_id).unwrap(),
            "src/lib.rs".into(),
            vec![
                ("boxology.toml".into(), manifest.into_bytes()),
                ("src/lib.rs".into(), source.as_bytes().to_vec()),
            ],
            vec![],
            OUTPUTS.iter().map(|output| (*output).to_owned()).collect(),
        )
        .unwrap()
    }

    /// The checked-in `hello` public revision; a valid import schema declares it.
    const IMPORT_REVISION: &str =
        "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176";

    #[test]
    fn import_revision_matches_the_checked_in_hello_schema() {
        let schema =
            std::str::from_utf8(include_bytes!("../../fixtures/hello/generated/schema.json"))
                .expect("checked-in hello schema is UTF-8");
        assert_eq!(schema.matches(IMPORT_REVISION).count(), 1);
    }

    /// A minimal valid `hello` import schema offering the `greet` capability over `String`.
    fn valid_hello_schema() -> String {
        import_schema("hello", &[("greet", "String", "String")])
    }

    /// Builds a full-output request for `box_id` declaring one import of `schema` at `schema_path`.
    fn request_for_with_import(
        box_id: &str,
        source: &str,
        package: &str,
        schema_path: &str,
        schema: &str,
    ) -> GenerationRequest {
        request_for_with_imports(box_id, source, &[(package, schema_path, schema)])
    }

    /// Builds a full-output request for `box_id` declaring one import per `(package, path, schema)`,
    /// in request order. The plural form drives ordering coverage for the typed import surface.
    fn request_for_with_imports(
        box_id: &str,
        source: &str,
        imports: &[(&str, &str, &str)],
    ) -> GenerationRequest {
        let manifest = format!("schema = 1\nid = \"{box_id}\"\nkind = \"box\"\n");
        let mut inputs = vec![
            ("boxology.toml".into(), manifest.into_bytes()),
            ("src/lib.rs".into(), source.as_bytes().to_vec()),
        ];
        let mut declared = Vec::new();
        for (package, schema_path, schema) in imports {
            inputs.push(((*schema_path).into(), schema.as_bytes().to_vec()));
            declared.push((BoxId::new(*package).unwrap(), (*schema_path).into()));
        }
        GenerationRequest::new(
            BoxId::new(box_id).unwrap(),
            "src/lib.rs".into(),
            inputs,
            declared,
            OUTPUTS.iter().map(|output| (*output).to_owned()).collect(),
        )
        .unwrap()
    }

    /// A minimal valid import schema for `package` offering each `(name, input, output)` capability.
    fn import_schema(package: &str, capabilities: &[(&str, &str, &str)]) -> String {
        let entries = capabilities
            .iter()
            .map(|(name, input, output)| {
                format!(
                    "{{ \"deprecation\": null, \"docs\": [], \"error\": \"ImportError\", \
                     \"id\": \"{package}.{name}\", \"idempotency\": \"none\", \
                     \"input\": {{ \"name\": \"name\", \"type\": \"{input}\" }}, \
                     \"max_exposure\": \"external\", \"name\": \"{name}\", \
                     \"output\": {{ \"type\": \"{output}\" }}, \
                     \"shape\": \"unary\" }}"
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{{ \"box_id\": \"{package}\", \"capabilities\": [ {entries} ], \
             \"provenance\": {{}}, \"revision\": \"{IMPORT_REVISION}\", \"schema_format\": 1, \
             \"types\": [ {{ \"deprecation\": null, \"docs\": [], \"kind\": \"error\", \
             \"name\": \"ImportError\", \"variants\": [ {{ \"deprecation\": null, \"docs\": [], \
             \"name\": \"Failed\", \"payload\": \"unit\" }} ] }} ] }}"
        )
    }

    fn tree(source: &str, reverse: bool) -> GeneratedTree {
        generate(request(source, reverse, OUTPUTS.to_vec())).unwrap()
    }

    fn file<'a>(tree: &'a GeneratedTree, path: &str) -> &'a GeneratedFile {
        tree.files()
            .iter()
            .find(|file| file.path() == path)
            .unwrap_or_else(|| panic!("missing generated file {path}"))
    }

    fn revision(source: &str) -> String {
        let tree = tree(source, false);
        serde_json::from_slice::<Value>(file(&tree, "generated/schema.json").bytes()).unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned()
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
        let mut expected_paths = OUTPUTS.to_vec();
        expected_paths.sort_unstable_by_key(|path| path.as_bytes());
        assert_eq!(
            tree.files()
                .iter()
                .map(GeneratedFile::path)
                .collect::<Vec<_>>(),
            expected_paths
        );
        assert_eq!(file(&tree, "generated/contract/Cargo.toml").bytes(), CARGO);
        let rust =
            std::str::from_utf8(file(&tree, "generated/contract/src/lib.rs").bytes()).unwrap();
        assert!(rust.starts_with("// Generated by boxology-generator 0.0.0\n"));
        let header_end = rust.find('\n').unwrap() + 1;
        let body_start = rust[header_end..]
            .find("#[derive(Debug, Clone, PartialEq)]")
            .map(|offset| header_end + offset)
            .unwrap();
        let mut without_descriptor = rust.as_bytes()[..header_end].to_vec();
        without_descriptor.extend_from_slice(&rust.as_bytes()[body_start..]);
        assert!(rust.contains("impl $crate::HelloDispatch for $receiver"));
        assert!(rust.contains("Box::pin(self.greet(context, input))"));
        let without_descriptor = String::from_utf8(without_descriptor).unwrap();
        let bridge_start = without_descriptor
            .find(" impl $crate::HelloDispatch")
            .expect("generated dispatch bridge");
        let bridge_end = without_descriptor
            .find("\n    };\n    (@ find $receiver:ty; greet invalid")
            .expect("generated invalid branch");
        let mut without_bridge = without_descriptor[..bridge_start].to_owned();
        without_bridge.push_str(&without_descriptor[bridge_end..]);
        assert_eq!(without_bridge.as_bytes(), RUST);
        assert!(rust.contains("static __BOXOLOGY_CONTRACT_DESCRIPTOR"));
        assert!(rust.contains("pub fn contract_descriptor()"));
        syn::parse_file(rust).unwrap();
    }

    #[test]
    fn cold_schema_has_exact_projection_revision_and_document() {
        const PROJECTION: &[u8] = b"\x62\x6f\x78\x6f\x6c\x6f\x67\x79\x2e\x70\x75\x62\x6c\x69\x63\x2d\x63\x6f\x6e\x74\x72\x61\x63\x74\x2d\x72\x65\x76\x69\x73\x69\x6f\x6e\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x05\x68\x65\x6c\x6c\x6f\x00\x00\x00\x00\x00\x00\x00\x01\x01\x00\x00\x00\x00\x00\x00\x00\x0a\x47\x72\x65\x65\x74\x45\x72\x72\x6f\x72\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x09\x45\x6d\x70\x74\x79\x4e\x61\x6d\x65\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x0b\x68\x65\x6c\x6c\x6f\x2e\x67\x72\x65\x65\x74\x00\x00\x00\x00\x00\x00\x00\x05\x67\x72\x65\x65\x74\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x04\x6e\x61\x6d\x65\x00\x00\x00\x00\x00\x00\x00\x06\x53\x74\x72\x69\x6e\x67\x00\x00\x00\x00\x00\x00\x00\x06\x53\x74\x72\x69\x6e\x67\x00\x00\x00\x00\x00\x00\x00\x0a\x47\x72\x65\x65\x74\x45\x72\x72\x6f\x72\x00\x00\x00\x00\x00\x00\x00\x05\x75\x6e\x61\x72\x79\x00\x00\x00\x00\x00\x00\x00\x08\x65\x78\x74\x65\x72\x6e\x61\x6c\x00\x00\x00\x00\x00\x00\x00\x04\x6e\x6f\x6e\x65";
        const SCHEMA_TEMPLATE: &str = r#"{
  "box_id": "hello",
  "capabilities": [
    {
      "deprecation": null,
      "docs": [],
      "error": "GreetError",
      "id": "hello.greet",
      "idempotency": "none",
      "input": {
        "name": "name",
        "type": "String"
      },
      "max_exposure": "external",
      "name": "greet",
      "output": {
        "type": "String"
      },
      "shape": "unary"
    }
  ],
  "provenance": {
    "generator": "boxology-generator",
    "generator_version": "0.0.0",
    "semantic_digest": "sha256:545f142b0ced7670e3f9efc7bcaaf3b7a2a0b2b790e5b48acaa85e4901c89b18"
  },
  "revision": "{REVISION}",
  "schema_format": 1,
  "types": [
    {
      "deprecation": null,
      "docs": [],
      "kind": "error",
      "name": "GreetError",
      "variants": [
        {
          "deprecation": null,
          "docs": [],
          "name": "EmptyName",
          "payload": "unit"
        }
      ]
    }
  ]
}
"#;
        let cold = request(CONTRACT, false, OUTPUTS.to_vec());
        let contract = ParsedRustInputs::parse(&cold)
            .and_then(|parsed| parsed.controlled_contract())
            .unwrap();
        assert_eq!(schema::projection("hello", contract.model()), PROJECTION);
        let independently_hashed = format!("sha256:{:x}", Sha256::digest(PROJECTION));
        let schema = SCHEMA_TEMPLATE.replace("{REVISION}", &independently_hashed);
        let generated = generate(cold).unwrap();
        assert_eq!(
            file(&generated, "generated/schema.json").bytes(),
            schema.as_bytes()
        );
        let value: Value = serde_json::from_slice(schema.as_bytes()).unwrap();
        assert_eq!(value["box_id"], "hello");
        assert_eq!(value["types"][0]["kind"], "error");
        assert_eq!(value["types"][0]["variants"][0]["payload"], "unit");
        assert_eq!(value["capabilities"][0]["id"], "hello.greet");
        assert_eq!(
            value["capabilities"][0]["input"],
            json!({"name":"name","type":"String"})
        );
        assert_eq!(value["capabilities"][0]["output"], json!({"type":"String"}));
        for field in ["docs", "deprecation"] {
            assert!(value["types"][0].get(field).is_some());
            assert!(value["types"][0]["variants"][0].get(field).is_some());
            assert!(value["capabilities"][0].get(field).is_some());
        }
        assert_eq!(value["revision"], independently_hashed);
        ContractRevision::new(independently_hashed.clone()).unwrap();
        assert_ne!(value["provenance"]["semantic_digest"], independently_hashed);

        let changed_provenance = schema::document(
            "hello",
            contract.model(),
            &schema::revision("hello", contract.model()),
            &[7; 32],
            "9.9.9",
        );
        assert_ne!(changed_provenance, schema.as_bytes());
        assert_eq!(
            serde_json::from_slice::<Value>(&changed_provenance).unwrap()["revision"],
            value["revision"]
        );
        let mut alternate = value.clone();
        alternate["schema_format"] = json!(1.0);
        let compact = serde_json::to_vec(&alternate).unwrap();
        assert_ne!(Sha256::digest(schema.as_bytes()), Sha256::digest(&compact));
        for document in [schema.as_bytes(), compact.as_slice()] {
            let parsed: Value = serde_json::from_slice(document).unwrap();
            let document_hash = format!("sha256:{:x}", Sha256::digest(document));
            assert_eq!(parsed["revision"], independently_hashed);
            assert_ne!(document_hash, independently_hashed);
        }
    }

    fn scalar_model(source: &str) -> boxology_generator_model::ControlledContract {
        ParsedRustInputs::parse(&request(source, false, OUTPUTS.to_vec()))
            .and_then(|parsed| parsed.controlled_contract())
            .unwrap()
    }

    #[test]
    fn scalar_boundary_document_projection_and_descriptor_are_type_aware() {
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(count:u32)->Result<bool,GreetError>; }",
        );
        let model = contract.model();
        let document: Value = serde_json::from_slice(&schema::document(
            "hello",
            model,
            &schema::revision("hello", model),
            &[0u8; 32],
            "0.0.0",
        ))
        .unwrap();
        assert_eq!(
            document["capabilities"][0]["input"],
            json!({"name": "count", "type": "u32"})
        );
        assert_eq!(
            document["capabilities"][0]["output"],
            json!({"type": "bool"})
        );
        let projection = schema::projection("hello", model);
        for slot in [
            b"\0\0\0\0\0\0\0\x03u32".as_slice(),
            b"\0\0\0\0\0\0\0\x04bool".as_slice(),
        ] {
            assert!(
                projection.windows(slot.len()).any(|bytes| bytes == slot),
                "projection missing scalar type slot"
            );
        }
        let descriptor =
            schema::descriptor_source("hello", model, &schema::revision("hello", model));
        assert!(descriptor.contains("::boxology_contract::TypeDescriptor::u32()"));
        assert!(descriptor.contains("::boxology_contract::TypeDescriptor::bool()"));
        // At a single capability the emitter keeps the pre-generalization binding name and never
        // uses the indexed `capability_0` name reserved for the multi-capability path.
        assert!(descriptor.contains("let capability = "));
        assert!(!descriptor.contains("capability_0"));
    }

    #[test]
    fn scalar_matrix_i64_input_f64_output_emits_matching_descriptors() {
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(count:i64)->Result<f64,GreetError>; }",
        );
        let model = contract.model();
        let document: Value = serde_json::from_slice(&schema::document(
            "hello",
            model,
            &schema::revision("hello", model),
            &[0u8; 32],
            "0.0.0",
        ))
        .unwrap();
        assert_eq!(document["capabilities"][0]["input"]["type"], "i64");
        assert_eq!(document["capabilities"][0]["output"]["type"], "f64");
        let projection = schema::projection("hello", model);
        for slot in [
            b"\0\0\0\0\0\0\0\x03i64".as_slice(),
            b"\0\0\0\0\0\0\0\x03f64".as_slice(),
        ] {
            assert!(
                projection.windows(slot.len()).any(|bytes| bytes == slot),
                "projection missing scalar type slot"
            );
        }
        let descriptor =
            schema::descriptor_source("hello", model, &schema::revision("hello", model));
        assert!(descriptor.contains("::boxology_contract::TypeDescriptor::i64()"));
        assert!(descriptor.contains("::boxology_contract::TypeDescriptor::f64()"));
    }

    #[test]
    fn blob_boundary_maps_to_the_lowercase_blob_descriptor_constructor() {
        // Guards the special-cased `Blob => "blob"` arm: without it, canonical_name()
        // would yield "Blob" and emit a non-existent TypeDescriptor::Blob(). Blob is
        // still fail-closed inside generate(); the emitters are exercised directly here.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(count:Blob)->Result<Blob,GreetError>; }",
        );
        let model = contract.model();
        let document: Value = serde_json::from_slice(&schema::document(
            "hello",
            model,
            &schema::revision("hello", model),
            &[0u8; 32],
            "0.0.0",
        ))
        .unwrap();
        assert_eq!(document["capabilities"][0]["input"]["type"], "Blob");
        assert_eq!(document["capabilities"][0]["output"]["type"], "Blob");
        let projection = schema::projection("hello", model);
        let slot = b"\0\0\0\0\0\0\0\x04Blob".as_slice();
        assert!(
            projection.windows(slot.len()).any(|bytes| bytes == slot),
            "projection missing Blob type slot"
        );
        let descriptor =
            schema::descriptor_source("hello", model, &schema::revision("hello", model));
        assert!(descriptor.contains("::boxology_contract::TypeDescriptor::blob()"));
        assert!(!descriptor.contains("::boxology_contract::TypeDescriptor::Blob()"));
    }

    #[test]
    fn multi_capability_document_lists_all_in_source_order() {
        // Two capabilities share one error enum; the schema document must list them in
        // source-declaration order with per-capability boundary shapes. Exercised directly
        // against the schema emitter.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }",
        );
        let model = contract.model();
        let document: Value = serde_json::from_slice(&schema::document(
            "store",
            model,
            &schema::revision("store", model),
            &[0u8; 32],
            "0.0.0",
        ))
        .unwrap();
        let capabilities = document["capabilities"].as_array().unwrap();
        assert_eq!(capabilities.len(), 2);
        assert_eq!(capabilities[0]["name"], "get");
        assert_eq!(
            capabilities[0]["input"],
            json!({"name": "key", "type": "u64"})
        );
        assert_eq!(capabilities[0]["error"], "StoreError");
        assert_eq!(capabilities[1]["name"], "put");
        assert_eq!(capabilities[1]["output"], json!({"type": "bool"}));
        assert_eq!(capabilities[1]["error"], "StoreError");
        assert!(capabilities[0]["id"].as_str().unwrap().ends_with(".get"));
        assert!(capabilities[1]["id"].as_str().unwrap().ends_with(".put"));
    }

    #[test]
    fn multi_capability_descriptor_source_lists_all_in_source_order() {
        // Two capabilities share one error enum; the Rust descriptor emitter must emit one
        // CapabilityDescriptor per capability in source order, each with its own boundary types,
        // over a single shared error descriptor. Exercised directly against the descriptor emitter.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }",
        );
        let model = contract.model();
        let descriptor =
            schema::descriptor_source("store", model, &schema::revision("store", model));
        syn::parse_file(&descriptor).expect("multi-capability descriptor source must parse");
        assert_eq!(
            descriptor
                .matches("::boxology_contract::CapabilityDescriptor::new(")
                .count(),
            2
        );
        let get_index = descriptor
            .find("::boxology_contract::CapabilityName::new(\"get\")")
            .expect("descriptor names get");
        let put_index = descriptor
            .find("::boxology_contract::CapabilityName::new(\"put\")")
            .expect("descriptor names put");
        assert!(
            get_index < put_index,
            "capabilities emitted out of source order"
        );
        let second = descriptor
            .match_indices("CapabilityDescriptor::new(")
            .nth(1)
            .expect("descriptor has a second capability")
            .0;
        let (first_capability, second_capability) = descriptor.split_at(second);
        assert!(first_capability.contains("::boxology_contract::TypeDescriptor::u64()"));
        assert!(second_capability.contains("::boxology_contract::TypeDescriptor::bool()"));
        assert!(descriptor.contains("let capability_0"));
        assert!(descriptor.contains("let capability_1"));
        assert!(descriptor.contains("[capability_0, capability_1]"));
        assert!(descriptor.contains("error.clone()"));
        assert_eq!(
            descriptor
                .matches("::boxology_contract::TypeDescriptor::enumeration(")
                .count(),
            1
        );
    }

    #[test]
    fn multi_capability_dispatch_source_lists_all_in_source_order() {
        // Two capabilities share one error enum; the dispatch trait and typed handle must emit one
        // method per capability in source order, each routed through its own capability-id static,
        // over a single shared error descriptor. Exercised directly against the dispatch emitter.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }",
        );
        let model = contract.model();
        let dispatch = dispatch_source("store", model);
        syn::parse_file(&dispatch).expect("multi-capability dispatch source must parse");
        assert!(dispatch.contains("impl ::boxology_contract::BoxHandle for StoreHandle"));
        let get_trait = dispatch.find("fn get").expect("trait names get");
        let put_trait = dispatch.find("fn put").expect("trait names put");
        assert!(get_trait < put_trait, "trait methods out of source order");
        let get_handle = dispatch.find("pub async fn get").expect("handle names get");
        let put_handle = dispatch.find("pub async fn put").expect("handle names put");
        assert!(
            get_handle < put_handle,
            "handle methods out of source order"
        );
        let get_static = dispatch
            .find("static STORE_GET")
            .expect("emits the STORE_GET capability static");
        let put_static = dispatch
            .find("static STORE_PUT")
            .expect("emits the STORE_PUT capability static");
        assert!(
            get_static < put_static,
            "capability statics out of source order"
        );
        assert_eq!(dispatch.matches("TypeDescriptor::enumeration(").count(), 1);
        assert_eq!(dispatch.matches("static STORE_ERROR_DESCRIPTOR").count(), 1);
        let get_section = &dispatch[get_handle..put_handle];
        assert!(get_section.contains("u64"), "get input spells u64");
        assert!(get_section.contains("String"), "get output spells String");
        assert!(
            !get_section.contains("bool"),
            "get section leaks put output"
        );
        let put_section = &dispatch[put_handle..];
        assert!(put_section.contains("String"), "put input spells String");
        assert!(put_section.contains("bool"), "put output spells bool");
    }

    #[test]
    fn multi_capability_test_support_source_programs_each_capability() {
        // Two capabilities share one error enum; the generated test-support fake must expose one
        // responder alias, struct field, and builder per capability and route the erased `call`
        // through each capability's own routing static in source order, decoding that capability's
        // own input type. Exercised directly against the test-support emitter.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }",
        );
        let model = contract.model();
        let fake = test_support_source("store", model);
        syn::parse_file(&fake).expect("multi-capability test-support source must parse");
        assert!(fake.contains("with_get"));
        assert!(fake.contains("with_put"));
        assert!(fake.contains("get: Option<Arc<GetResponder>>"));
        assert!(fake.contains("put: Option<Arc<PutResponder>>"));
        assert!(fake.contains("type GetFuture"));
        assert!(fake.contains("type GetResponder"));
        assert!(fake.contains("type PutFuture"));
        assert!(fake.contains("type PutResponder"));
        assert!(fake.contains(
            "use super::{StoreError, STORE_GET, STORE_PUT, StoreHandle, conversion_detail}"
        ));
        let get_branch = fake
            .find("if capability == &*STORE_GET")
            .expect("routes the STORE_GET branch");
        let put_branch = fake
            .find("if capability == &*STORE_PUT")
            .expect("routes the STORE_PUT branch");
        assert!(
            get_branch < put_branch,
            "capability branches out of source order"
        );
        // get's input is u64, put's input is String.
        assert!(fake[get_branch..put_branch].contains("TypeDescriptor::u64()"));
        assert!(fake[put_branch..].contains("TypeDescriptor::string()"));
        assert!(fake.contains("unprogrammed_capability"));
    }

    #[test]
    fn multi_capability_projection_is_deterministic_and_order_sensitive() {
        // The frozen fingerprint must be deterministic yet order-sensitive: swapping the
        // source declaration order of two capabilities under the same error must move the
        // revision, and the capability-count field must encode the total capability count.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }",
        );
        let model = contract.model();
        assert_eq!(
            schema::projection("store", model),
            schema::projection("store", model)
        );
        let swapped = scalar_model(
            "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; }",
        );
        assert_ne!(
            schema::revision("store", model),
            schema::revision("store", swapped.model())
        );
        let projection = schema::projection("store", model);
        let capability_count = b"\x00\x00\x00\x00\x00\x00\x00\x02".as_slice();
        assert!(
            projection
                .windows(capability_count.len())
                .any(|bytes| bytes == capability_count),
            "projection missing two-capability count field"
        );
    }

    #[test]
    fn scalar_adapter_source_is_type_parameterized() {
        // The generated numeric adapter is the same substitution path as the String
        // adapter that generated_adapter_registers_and_dispatches_through_stub_transport
        // compiles; this guards its input-type parameterization against regression.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(count:u32)->Result<bool,GreetError>; }",
        );
        let adapter = adapter_source("hello", contract.model(), &[]);
        assert!(adapter.contains("pub fn register<T>"));
        assert!(adapter.contains("composition.register(implementation_descriptor()"));
        assert!(adapter.contains("::boxology_contract::TypeDescriptor::u32()"));
        assert!(adapter.contains("u32::decode(&input)"));
        assert!(!adapter.contains("::std::string::String::decode"));
        assert!(!adapter.contains("::boxology_contract::TypeDescriptor::string()"));
    }

    #[test]
    fn multi_capability_adapter_source_routes_each_capability() {
        // Two capabilities share one error enum; the adapter routes each by its descriptor id in
        // source order with per-capability decode, then falls through to unknown_capability.
        // Exercised directly against the adapter emitter.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }",
        );
        let adapter = adapter_source("store", contract.model(), &[]);
        syn::parse_file(&adapter).expect("multi-capability adapter must parse");
        let get_dispatch = adapter
            .find("StoreDispatch::get(")
            .expect("adapter dispatches the get capability");
        let put_dispatch = adapter
            .find("StoreDispatch::put(")
            .expect("adapter dispatches the put capability");
        assert!(
            get_dispatch < put_dispatch,
            "capabilities route in source order"
        );
        // The get branch decodes u64 before its dispatch; the put branch decodes String between
        // the two dispatches.
        let get_branch = &adapter[..get_dispatch];
        assert!(get_branch.contains("::boxology_contract::TypeDescriptor::u64()"));
        assert!(get_branch.contains("u64::decode(&input)"));
        let put_branch = &adapter[get_dispatch..put_dispatch];
        assert!(put_branch.contains("::boxology_contract::TypeDescriptor::string()"));
        assert!(put_branch.contains("::std::string::String::decode"));
        // N>1 index routing falls through to unknown_capability and never emits the single-
        // capability `.first().expect("... has one capability")` envelope.
        assert!(adapter.contains("unknown_capability"));
        assert!(!adapter.contains("has one capability"));
    }

    #[test]
    fn import_adapter_emits_import_descriptors() {
        // A greeter box that declares an import of the hello schema emits the import into its
        // adapter's implementation descriptor: the ImportDescriptor constructor, the package, its
        // revision, and each imported capability name. The emitted adapter still parses.
        let request = request_for_with_import(
            "greeter",
            CONTRACT,
            "hello",
            "imports/hello.json",
            &valid_hello_schema(),
        );
        let tree = generate(request).unwrap();
        let adapter =
            std::str::from_utf8(file(&tree, "generated/adapter/adapter.rs").bytes()).unwrap();
        syn::parse_file(adapter).expect("import adapter must parse");
        assert!(adapter.contains("::boxology_contract::ImportDescriptor::new("));
        assert!(adapter.contains("\"hello\""));
        assert!(adapter.contains(IMPORT_REVISION));
        assert!(adapter.contains("CapabilityName::new(\"greet\")"));
    }

    #[test]
    fn import_adapter_emits_typed_import_surface() {
        // A greeter box importing two packages — hello (two capabilities) and world (one) — emits a
        // typed import handle per package into its adapter: a `{BPascal}Import` wrapper with one
        // typed leaf-I/O async method per capability, a `{APascal}Imports` bundle, and a
        // `typed_imports` converter. The surface is purely additive — the factory still parks
        // `_imports: imports` unchanged.
        let hello = import_schema(
            "hello",
            &[("greet", "String", "String"), ("count", "u64", "bool")],
        );
        let world = import_schema("world", &[("ping", "String", "String")]);
        let request = request_for_with_imports(
            "greeter",
            CONTRACT,
            &[
                ("hello", "imports/hello.json", &hello),
                ("world", "imports/world.json", &world),
            ],
        );
        let tree = generate(request).unwrap();
        let adapter =
            std::str::from_utf8(file(&tree, "generated/adapter/adapter.rs").bytes()).unwrap();
        syn::parse_file(adapter).expect("typed-import adapter must parse");

        // One wrapper per imported box, with a typed leaf-I/O method per capability.
        assert!(adapter.contains("pub struct HelloImport"));
        assert!(adapter.contains("handle: ::boxology_runtime::ImportHandle"));
        assert!(adapter.contains("pub async fn greet("));
        assert!(adapter.contains("pub async fn count("));
        assert!(adapter.contains("pub struct WorldImport"));
        assert!(adapter.contains("pub async fn ping("));
        // Level-2 typing: leaf-typed I/O, output conformed as ConsumerOutput, error stays erased.
        // `TypeDescriptor::bool()` cannot come from box A's own String contract, so it proves the
        // per-capability output descriptor constructor for the `count` method.
        assert!(adapter.contains("::boxology_contract::DecodeRole::ConsumerOutput"));
        assert!(adapter.contains("::boxology_contract::TypeDescriptor::bool()"));
        // Faithful error mapping.
        assert!(adapter.contains("ErasedCallError::ContractViolation"));
        assert!(adapter.contains("\"input_encode\""));
        assert!(adapter.contains("ErasedCallError::InvalidResponse"));
        assert!(adapter.contains("\"output_decode\""));
        // Bundle + converter; field name = box_id with `-`->`_` (here identity).
        assert!(adapter.contains("pub struct GreeterImports"));
        assert!(adapter.contains("pub hello: HelloImport"));
        assert!(adapter.contains("pub world: WorldImport"));
        assert!(adapter.contains("pub fn typed_imports("));
        assert!(adapter.contains("pub fn register<T, F>"));
        assert!(adapter.contains("F: FnOnce(GreeterImports) -> T"));
        assert!(adapter.contains("factory(build(typed), imports)"));

        // Capability order within a wrapper: greet before count (schema order).
        let greet = adapter.find("pub async fn greet(").unwrap();
        let count = adapter.find("pub async fn count(").unwrap();
        assert!(greet < count, "capabilities emitted out of schema order");
        // Import order across wrappers and bundle fields: hello before world (request order).
        let hello_struct = adapter.find("pub struct HelloImport").unwrap();
        let world_struct = adapter.find("pub struct WorldImport").unwrap();
        assert!(
            hello_struct < world_struct,
            "wrappers emitted out of request order"
        );
        let hello_field = adapter.find("pub hello: HelloImport").unwrap();
        let world_field = adapter.find("pub world: WorldImport").unwrap();
        assert!(
            hello_field < world_field,
            "bundle fields emitted out of request order"
        );

        // Additive: the factory still parks the raw imports unchanged, exactly once.
        assert_eq!(adapter.matches("_imports: imports").count(), 1);
        assert!(adapter.contains("pub fn factory"));
    }

    #[test]
    fn zero_import_and_with_import_differ_only_in_adapter() {
        // Imports are implementation-local: declaring one changes only the adapter. The outward
        // contract crate, schema, revision, and semantic digest are byte-identical with and without.
        let without = generate(request_for("greeter", CONTRACT)).unwrap();
        let with = generate(request_for_with_import(
            "greeter",
            CONTRACT,
            "hello",
            "imports/hello.json",
            &valid_hello_schema(),
        ))
        .unwrap();
        for path in [
            "generated/contract/Cargo.toml",
            "generated/contract/src/lib.rs",
            "generated/schema.json",
        ] {
            assert_eq!(
                file(&without, path).bytes(),
                file(&with, path).bytes(),
                "{path} must be identical with and without imports"
            );
        }
        assert_ne!(
            file(&without, "generated/adapter/adapter.rs").bytes(),
            file(&with, "generated/adapter/adapter.rs").bytes(),
            "the adapter must carry the declared import"
        );
        let revision = |tree: &GeneratedTree| {
            serde_json::from_slice::<Value>(file(tree, "generated/schema.json").bytes()).unwrap()
                ["revision"]
                .as_str()
                .unwrap()
                .to_owned()
        };
        assert_eq!(revision(&without), revision(&with));
        let digest = |tree: &GeneratedTree| {
            marker_parts(file(tree, "generated/contract/src/lib.rs").bytes())
                .1
                .to_owned()
        };
        assert_eq!(digest(&without), digest(&with));
        // The typed import surface is emitted only when the box has an import; the zero-import
        // adapter never carries the `typed_imports` converter (the hard mechanical guard is the
        // zero-import fixture golden in crates/fixtures/).
        let adapter = |tree: &GeneratedTree| {
            std::str::from_utf8(file(tree, "generated/adapter/adapter.rs").bytes())
                .unwrap()
                .to_owned()
        };
        assert!(adapter(&with).contains("typed_imports"));
        assert!(!adapter(&without).contains("typed_imports"));
    }

    #[test]
    fn multi_capability_checker_macro_checks_each_capability() {
        // Two capabilities share one error enum; the implementation-checker macro must emit one
        // disjoint `@find_{capability}` recursion per capability (each with valid/invalid/recurse/
        // missing arms) plus exactly one combined `impl HelloDispatch` bridging both, so N separate
        // impls of the same trait for the same receiver never collide (E0119). Exercised directly
        // against the checker emitter.
        let contract = scalar_model(
            "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }",
        );
        let checker = checker_source("store", contract.model());
        syn::parse_file(&checker).expect("multi-capability checker macro must parse");
        assert!(checker.contains("@find_get"));
        assert!(checker.contains("@find_put"));
        // Each tag carries the full valid/invalid/recurse/missing arm set.
        for tag in ["@find_get", "@find_put"] {
            let cap = tag.strip_prefix("@find_").unwrap();
            assert!(checker.contains(&format!("({tag} $receiver:ty; {cap} valid; $($rest:tt)*)")));
            assert!(checker.contains(&format!(
                "({tag} $receiver:ty; {cap} invalid; $($rest:tt)*)"
            )));
            assert!(checker.contains(&format!(
                "({tag} $receiver:ty; $other:ident $validity:ident; $($rest:tt)*)"
            )));
            assert!(checker.contains(&format!("({tag} $receiver:ty;)")));
        }
        // Per-capability require_future spells each capability's own output over the shared error.
        assert!(checker.contains("Result<::std::string::String, $crate::StoreError>"));
        assert!(checker.contains("Result<bool, $crate::StoreError>"));
        // Exactly one combined impl bridges both capabilities.
        assert_eq!(
            checker
                .matches("impl $crate::StoreDispatch for $receiver")
                .count(),
            1
        );
        assert!(checker.contains("Box::pin(self.get(context, input))"));
        assert!(checker.contains("Box::pin(self.put(context, input))"));
        // Each bridge takes its own capability's input type, in source order.
        let get_bridge = checker.find("fn get<'a>").expect("emits a get bridge");
        let put_bridge = checker.find("fn put<'a>").expect("emits a put bridge");
        assert!(get_bridge < put_bridge, "bridges out of source order");
        assert!(checker[get_bridge..put_bridge].contains("input: u64"));
        assert!(checker[put_bridge..].contains("input: ::std::string::String"));
    }

    #[test]
    fn public_revision_tracks_every_public_semantic_and_only_public_semantics() {
        let base = revision(CONTRACT);
        assert_eq!(
            hello_mutations()
                .iter()
                .map(|mutation| mutation.name)
                .collect::<Vec<_>>(),
            HELLO_MUTATION_NAMES
        );
        for mutation in hello_mutations() {
            let changed = mutation.apply(CONTRACT);
            assert_ne!(
                revision(&changed),
                base,
                "mutation `{}` did not change revision",
                mutation.name
            );
        }
        let first = CONTRACT.replace("EmptyName", "EmptyName, Busy");
        let reversed = CONTRACT.replace("EmptyName", "Busy, EmptyName");
        assert_ne!(revision(&first), revision(&reversed));
        let first_schema: Value =
            serde_json::from_slice(file(&tree(&first, false), "generated/schema.json").bytes())
                .unwrap();
        assert_eq!(
            first_schema["types"][0]["variants"],
            json!([
                {"deprecation":null,"docs":[],"name":"EmptyName","payload":"unit"},
                {"deprecation":null,"docs":[],"name":"Busy","payload":"unit"}
            ])
        );
        let metadata = "boxology::contract! { #[doc=\"é\"] #[doc=\"second\"] #[deprecated(note=\"old\")] #[error] pub enum GreetError { #[doc=\"v1\"] #[doc=\"v2\"] #[deprecated] EmptyName } #[doc=\"c1\"] #[doc=\"c2\"] #[deprecated(note=\"later\")] #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
        let parsed = ParsedRustInputs::parse(&request(metadata, false, OUTPUTS.to_vec()))
            .and_then(|parsed| parsed.controlled_contract())
            .unwrap();
        let rich = schema::projection("hello", parsed.model());
        for expected in [
            b"\0\0\0\0\0\0\0\x02\0\0\0\0\0\0\0\x02\xc3\xa9\0\0\0\0\0\0\0\x06second\x01\0\0\0\0\0\0\0\x03old".as_slice(),
            b"\0\0\0\0\0\0\0\x02\0\0\0\0\0\0\0\x02v1\0\0\0\0\0\0\0\x02v2\x01\0\0\0\0\0\0\0\0".as_slice(),
            b"\0\0\0\0\0\0\0\x02\0\0\0\0\0\0\0\x02c1\0\0\0\0\0\0\0\x02c2\x01\0\0\0\0\0\0\0\x05later".as_slice(),
        ] {
            assert!(rich.windows(expected.len()).any(|bytes| bytes == expected));
        }
        let metadata: Value =
            serde_json::from_slice(file(&tree(metadata, false), "generated/schema.json").bytes())
                .unwrap();
        assert_eq!(metadata["types"][0]["docs"], json!(["é", "second"]));
        assert_eq!(metadata["types"][0]["deprecation"], json!({"note":"old"}));
        assert_eq!(
            metadata["types"][0]["variants"][0]["docs"],
            json!(["v1", "v2"])
        );
        assert_eq!(
            metadata["types"][0]["variants"][0]["deprecation"],
            json!({"note":""})
        );
        assert_eq!(metadata["capabilities"][0]["docs"], json!(["c1", "c2"]));
        assert_eq!(
            metadata["capabilities"][0]["deprecation"],
            json!({"note":"later"})
        );
        let decorated = "\n// ignored\nboxology::contract! { #[error] pub enum GreetError { EmptyName } /* ignored */ #[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, GreetError>; }";
        assert_eq!(revision(decorated), base);
        let parsed = ParsedRustInputs::parse(&request(CONTRACT, true, OUTPUTS.to_vec()))
            .and_then(|parsed| parsed.controlled_contract())
            .unwrap();
        let relocated = GenerationRequest::new(
            BoxId::new("hello").unwrap(),
            "nested/contract.rs".into(),
            vec![
                ("boxology.toml".into(), MANIFEST.to_vec()),
                ("nested/contract.rs".into(), CONTRACT.as_bytes().to_vec()),
            ],
            vec![],
            OUTPUTS.iter().map(|path| (*path).into()).collect(),
        )
        .unwrap();
        let relocated = ParsedRustInputs::parse(&relocated)
            .and_then(|parsed| parsed.controlled_contract())
            .unwrap();
        assert_eq!(
            schema::projection("hello", parsed.model()),
            schema::projection("hello", relocated.model())
        );
        assert_ne!(
            Sha256::digest(schema::projection("other", parsed.model())),
            Sha256::digest(schema::projection("hello", parsed.model()))
        );
    }

    #[test]
    fn spelling_and_order_do_not_change_artifacts() {
        let decorated = "// ignored\nboxology::contract! { #[error] pub enum GreetError { EmptyName } /* ignored */ #[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, GreetError>; }";
        assert_eq!(tree(CONTRACT, false), tree(decorated, true));
        assert_eq!(
            generate(request(
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
        let result = generate(request(&source, false, OUTPUTS.to_vec()));
        let diagnostics = result.expect_err("reserved input must not return an artifact tree");
        assert_eq!(
            diagnostics.to_string(),
            "BXG0038 src/lib.rs:1:54-1:61 offending=\"invalid controlled contract syntax\" rule=\"contract tokens must satisfy the controlled v0 grammar\" source=\"specs/s2-contract-generator.md D3\""
        );
    }

    #[test]
    fn value_payload_generates_and_named_payload_fails_before_emission() {
        let value = CONTRACT.replace("EmptyName", "Code(u32)");
        let generated = generate(request(&value, false, OUTPUTS.to_vec())).expect("value payload");
        let rust =
            std::str::from_utf8(file(&generated, "generated/contract/src/lib.rs").bytes()).unwrap();
        assert!(rust.contains("Code(u32)"));

        let named = CONTRACT.replace("EmptyName", "Detail { message: String }");
        let diagnostics = generate(request(&named, false, OUTPUTS.to_vec())).unwrap_err();
        assert_eq!(
            diagnostics.to_string(),
            "BXG0048 src/lib.rs:1:11-1:19 offending=\"named-field error variants are not yet emittable\" rule=\"named-field payloads require contract-emitter support\" source=\"specs/s2-contract-generator.md D3\""
        );

        let value_blob = CONTRACT.replace("EmptyName", "Code(Blob)");
        let diagnostics = generate(request(&value_blob, false, OUTPUTS.to_vec())).unwrap_err();
        assert_eq!(
            diagnostics.to_string(),
            "BXG0040 src/lib.rs:1:11-1:19 offending=\"Blob capability boundary or value-payload leaf not yet emittable in v0\" rule=\"the `Blob` capability boundary or value-payload leaf is parsed and modelled but its v0 end-to-end runtime generation is not yet implemented (deferred); scalar leaves and `String` are emittable.\" source=\"specs/s2-contract-generator.md D3,D5\""
        );

        let empty_named = CONTRACT.replace("EmptyName", "EmptyNamed {}");
        let diagnostics = generate(request(&empty_named, false, OUTPUTS.to_vec())).unwrap_err();
        assert_eq!(diagnostics.as_slice().len(), 1);
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0048");

        let named_and_blob = named.replace("name:String", "name:Blob");
        let diagnostics = generate(request(&named_and_blob, false, OUTPUTS.to_vec())).unwrap_err();
        assert_eq!(
            diagnostics
                .as_slice()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            ["BXG0040", "BXG0048"]
        );
        assert!(
            diagnostics
                .as_slice()
                .iter()
                .all(|diagnostic| diagnostic.span() == diagnostics.as_slice()[0].span())
        );
    }

    const VALUE_PAYLOAD: &str = "boxology::contract! { #[error] pub enum Fault { #[doc = \"code variant\"] Code(#[doc = \"code payload\"] #[deprecated(note = \"use detail\")] u32), Empty } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,Fault>; }";

    #[test]
    fn value_payload_emission_pins_enum_abi_and_both_descriptor_sites() {
        let generated = tree(VALUE_PAYLOAD, false);
        let rust =
            std::str::from_utf8(file(&generated, "generated/contract/src/lib.rs").bytes()).unwrap();
        for expected in [
            "///code variant",
            "Code(",
            "///code payload",
            "#[deprecated(note = \"use detail\")]",
            "u32,",
            "value\n                        .encode()\n                        .map_err(|error| {\n                            error\n                                .under(\n                                    ::boxology_contract::PathSegment::Variant(\"Code\".into()),\n                                )\n                        })?,",
            "u32::decode(payload)\n                    .map(Self::Code)\n                    .map_err(|error| {\n                        error\n                            .under(::boxology_contract::PathSegment::Variant(tag.into()))\n                    })",
            "Self::Code(..) => \"Code\",",
            "Self::Empty => (\"Empty\".into(), ::boxology_contract::SlotValue::Null),",
        ] {
            assert!(rust.contains(expected), "missing {expected} in {rust}");
        }
        let value_payload_token = "::boxology_contract::VariantPayload::Value(\n                    ::boxology_contract::TypeDescriptor::u32(),\n                )";
        assert_eq!(
            rust.matches(value_payload_token).count(),
            2,
            "both descriptor sites must emit VariantPayload::Value(TypeDescriptor::u32())"
        );
        assert!(rust.contains("static FAULT_DESCRIPTOR"));
        assert!(rust.contains("static __BOXOLOGY_CONTRACT_DESCRIPTOR"));

        let model = scalar_model(VALUE_PAYLOAD).model().clone();
        let descriptor = schema::descriptor_source("hello", &model, &[0u8; 32]);
        let dispatch = dispatch_source("hello", &model);
        let shared = "::boxology_contract::VariantDescriptor::new(\"Code\", ::boxology_contract::VariantPayload::Value(::boxology_contract::TypeDescriptor::u32()), None),";
        assert!(
            descriptor.contains(shared),
            "contract descriptor must use shared helper tokens"
        );
        assert!(
            dispatch.contains(shared),
            "FAULT_DESCRIPTOR must use shared helper tokens"
        );
        assert_eq!(
            descriptor.matches(shared).count(),
            1,
            "contract descriptor site"
        );
        assert_eq!(
            dispatch.matches(shared).count(),
            1,
            "dispatch descriptor site"
        );
    }

    /// Mixed payload fixture mirroring PR1 `mixed_payloads` — the cross-crate semantic anchor.
    const MIXED: &str = "boxology::contract! { #[error] pub enum PayloadError { Unit, #[doc=\"value variant\"] Value(#[doc=\"value payload\"] #[deprecated(note=\"use detail\")] u32), #[deprecated(note=\"retired\")] Named { #[doc=\"message field\"] message: String, #[deprecated(note=\"use text\")] code: i64 }, EmptyNamed {} } #[capability(exposure=external)] pub async fn inspect(name:String)->Result<String,PayloadError>; }";

    #[derive(Clone, Copy)]
    struct Mutation {
        name: &'static str,
        anchor: &'static str,
        replacement: &'static str,
    }

    impl Mutation {
        fn apply(self, source: &str) -> String {
            let expected = usize::from(self.name == "error_rename") + 1;
            assert_eq!(
                source.matches(self.anchor).count(),
                expected,
                "mutation anchor must occur exactly {expected} time(s): {}",
                self.anchor
            );
            source.replace(self.anchor, self.replacement)
        }
    }

    const HELLO_MUTATION_NAMES: [&str; 11] = [
        "error_rename",
        "error_docs",
        "error_deprecation",
        "variant_rename",
        "variant_docs",
        "variant_deprecation",
        "variant_added",
        "capability_docs",
        "capability_deprecation",
        "capability_rename",
        "input_name",
    ];

    fn hello_mutations() -> [Mutation; 11] {
        [
            Mutation {
                name: "error_rename",
                anchor: "GreetError",
                replacement: "HelloError",
            },
            Mutation {
                name: "error_docs",
                anchor: "#[error]",
                replacement: "#[doc=\"error\"] #[error]",
            },
            Mutation {
                name: "error_deprecation",
                anchor: "#[error]",
                replacement: "#[deprecated] #[error]",
            },
            Mutation {
                name: "variant_rename",
                anchor: "EmptyName",
                replacement: "MissingName",
            },
            Mutation {
                name: "variant_docs",
                anchor: "EmptyName",
                replacement: "#[doc=\"empty\"] EmptyName",
            },
            Mutation {
                name: "variant_deprecation",
                anchor: "EmptyName",
                replacement: "#[deprecated(note=\"old\")] EmptyName",
            },
            Mutation {
                name: "variant_added",
                anchor: "EmptyName",
                replacement: "EmptyName, Busy",
            },
            Mutation {
                name: "capability_docs",
                anchor: "#[capability",
                replacement: "#[doc=\"greet\"] #[capability",
            },
            Mutation {
                name: "capability_deprecation",
                anchor: "#[capability",
                replacement: "#[deprecated] #[capability",
            },
            Mutation {
                name: "capability_rename",
                anchor: "fn greet",
                replacement: "fn welcome",
            },
            Mutation {
                name: "input_name",
                anchor: "(name:",
                replacement: "(person:",
            },
        ]
    }

    const MIXED_MUTATION_NAMES: [&str; 21] = [
        "unit_to_value",
        "value_to_unit",
        "value_to_named",
        "named_to_value",
        "named_empty_to_unit",
        "named_empty_field_added",
        "variant_order",
        "field_order",
        "value_type_u32_to_u64",
        "field_type_string_to_bool",
        "field_type_i64_to_u8",
        "field_rename_message_to_text",
        "variant_docs",
        "payload_docs",
        "payload_deprecation_removed",
        "payload_note",
        "variant_deprecation_removed",
        "field_note",
        "field_docs_added",
        "variant_removed",
        "variant_added",
    ];

    fn mixed_mutations() -> [Mutation; 21] {
        [
            Mutation {
                name: "unit_to_value",
                anchor: "Unit,",
                replacement: "Unit(u32),",
            },
            Mutation {
                name: "value_to_unit",
                anchor: "#[doc=\"value variant\"] Value(#[doc=\"value payload\"] #[deprecated(note=\"use detail\")] u32)",
                replacement: "Value",
            },
            Mutation {
                name: "value_to_named",
                anchor: "#[doc=\"value variant\"] Value(#[doc=\"value payload\"] #[deprecated(note=\"use detail\")] u32)",
                replacement: "Value { detail: u32 }",
            },
            Mutation {
                name: "named_to_value",
                anchor: "#[deprecated(note=\"retired\")] Named { #[doc=\"message field\"] message: String, #[deprecated(note=\"use text\")] code: i64 }",
                replacement: "Named(String)",
            },
            Mutation {
                name: "named_empty_to_unit",
                anchor: "EmptyNamed {}",
                replacement: "EmptyNamed",
            },
            Mutation {
                name: "named_empty_field_added",
                anchor: "EmptyNamed {}",
                replacement: "EmptyNamed { spare: bool }",
            },
            Mutation {
                name: "variant_order",
                anchor: "#[doc=\"value variant\"] Value(#[doc=\"value payload\"] #[deprecated(note=\"use detail\")] u32), #[deprecated(note=\"retired\")] Named { #[doc=\"message field\"] message: String, #[deprecated(note=\"use text\")] code: i64 }",
                replacement: "#[deprecated(note=\"retired\")] Named { #[doc=\"message field\"] message: String, #[deprecated(note=\"use text\")] code: i64 }, #[doc=\"value variant\"] Value(#[doc=\"value payload\"] #[deprecated(note=\"use detail\")] u32)",
            },
            Mutation {
                name: "field_order",
                anchor: "#[doc=\"message field\"] message: String, #[deprecated(note=\"use text\")] code: i64",
                replacement: "#[deprecated(note=\"use text\")] code: i64, #[doc=\"message field\"] message: String",
            },
            Mutation {
                name: "value_type_u32_to_u64",
                anchor: "u32)",
                replacement: "u64)",
            },
            Mutation {
                name: "field_type_string_to_bool",
                anchor: "message: String",
                replacement: "message: bool",
            },
            Mutation {
                name: "field_type_i64_to_u8",
                anchor: "code: i64",
                replacement: "code: u8",
            },
            Mutation {
                name: "field_rename_message_to_text",
                anchor: "message: String",
                replacement: "text: String",
            },
            Mutation {
                name: "variant_docs",
                anchor: "\"value variant\"",
                replacement: "\"variant\"",
            },
            Mutation {
                name: "payload_docs",
                anchor: "\"value payload\"",
                replacement: "\"payload\"",
            },
            Mutation {
                name: "payload_deprecation_removed",
                anchor: " #[deprecated(note=\"use detail\")]",
                replacement: "",
            },
            Mutation {
                name: "payload_note",
                anchor: "\"use detail\"",
                replacement: "\"use other\"",
            },
            Mutation {
                name: "variant_deprecation_removed",
                anchor: " #[deprecated(note=\"retired\")]",
                replacement: "",
            },
            Mutation {
                name: "field_note",
                anchor: "\"use text\"",
                replacement: "\"use code\"",
            },
            Mutation {
                name: "field_docs_added",
                anchor: "code: i64",
                replacement: "#[doc=\"code field\"] code: i64",
            },
            Mutation {
                name: "variant_removed",
                anchor: ", EmptyNamed {}",
                replacement: "",
            },
            Mutation {
                name: "variant_added",
                anchor: "Unit,",
                replacement: "Unit, Other,",
            },
        ]
    }

    fn mixed_error_rename() -> String {
        assert_eq!(
            MIXED.matches("PayloadError").count(),
            2,
            "error rename must cover the enum and capability sites"
        );
        MIXED.replace("PayloadError", "StorageFault")
    }

    fn mixed_contract() -> boxology_generator_model::ControlledContract {
        scalar_model(MIXED)
    }

    fn mixed_revision_of(source: &str) -> [u8; 32] {
        schema::revision("payloads", scalar_model(source).model())
    }

    fn hash_hex(hash: &[u8; 32]) -> String {
        format!(
            "sha256:{}",
            hash.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }

    fn assert_unique(anchor: &str) {
        assert_eq!(
            MIXED.matches(anchor).count(),
            1,
            "mutation anchor must occur exactly once: {anchor}"
        );
    }

    const MIXED_SCHEMA: &[u8] = br#"{
  "box_id": "payloads",
  "capabilities": [
    {
      "deprecation": null,
      "docs": [],
      "error": "PayloadError",
      "id": "payloads.inspect",
      "idempotency": "none",
      "input": {
        "name": "name",
        "type": "String"
      },
      "max_exposure": "external",
      "name": "inspect",
      "output": {
        "type": "String"
      },
      "shape": "unary"
    }
  ],
  "provenance": {
    "generator": "boxology-generator",
    "generator_version": "0.0.0",
    "semantic_digest": "sha256:795b2224a1e4cc8360b5cf541499efad26adf3619bb4afa6748c159373d44806"
  },
  "revision": "sha256:ab76207e29cc030e0a072ccfad054352e67fd98871a84a1b820a162eb411597e",
  "schema_format": 1,
  "types": [
    {
      "deprecation": null,
      "docs": [],
      "kind": "error",
      "name": "PayloadError",
      "variants": [
        {
          "deprecation": null,
          "docs": [],
          "name": "Unit",
          "payload": "unit"
        },
        {
          "deprecation": null,
          "docs": [
            "value variant"
          ],
          "name": "Value",
          "payload": {
            "deprecation": {
              "note": "use detail"
            },
            "docs": [
              "value payload"
            ],
            "kind": "value",
            "type": "u32"
          }
        },
        {
          "deprecation": {
            "note": "retired"
          },
          "docs": [],
          "name": "Named",
          "payload": {
            "fields": [
              {
                "deprecation": null,
                "docs": [
                  "message field"
                ],
                "name": "message",
                "type": "String"
              },
              {
                "deprecation": {
                  "note": "use text"
                },
                "docs": [],
                "name": "code",
                "type": "i64"
              }
            ],
            "kind": "named"
          }
        },
        {
          "deprecation": null,
          "docs": [],
          "name": "EmptyNamed",
          "payload": {
            "fields": [],
            "kind": "named"
          }
        }
      ]
    }
  ]
}
"#;

    #[test]
    fn mixed_payload_schema_pins_projection_revision_digest_and_document() {
        const PROJECTION: &[u8] = b"\x62\x6f\x78\x6f\x6c\x6f\x67\x79\x2e\x70\x75\x62\x6c\x69\x63\x2d\x63\x6f\x6e\x74\x72\x61\x63\x74\x2d\x72\x65\x76\x69\x73\x69\x6f\x6e\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x08\x70\x61\x79\x6c\x6f\x61\x64\x73\x00\x00\x00\x00\x00\x00\x00\x01\x01\x00\x00\x00\x00\x00\x00\x00\x0c\x50\x61\x79\x6c\x6f\x61\x64\x45\x72\x72\x6f\x72\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x04\x00\x00\x00\x00\x00\x00\x00\x04\x55\x6e\x69\x74\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x05\x56\x61\x6c\x75\x65\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x0d\x76\x61\x6c\x75\x65\x20\x76\x61\x72\x69\x61\x6e\x74\x00\x01\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x0d\x76\x61\x6c\x75\x65\x20\x70\x61\x79\x6c\x6f\x61\x64\x01\x00\x00\x00\x00\x00\x00\x00\x0a\x75\x73\x65\x20\x64\x65\x74\x61\x69\x6c\x00\x00\x00\x00\x00\x00\x00\x03\x75\x33\x32\x00\x00\x00\x00\x00\x00\x00\x05\x4e\x61\x6d\x65\x64\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x07\x72\x65\x74\x69\x72\x65\x64\x02\x00\x00\x00\x00\x00\x00\x00\x02\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x0d\x6d\x65\x73\x73\x61\x67\x65\x20\x66\x69\x65\x6c\x64\x00\x00\x00\x00\x00\x00\x00\x00\x07\x6d\x65\x73\x73\x61\x67\x65\x00\x00\x00\x00\x00\x00\x00\x06\x53\x74\x72\x69\x6e\x67\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x08\x75\x73\x65\x20\x74\x65\x78\x74\x00\x00\x00\x00\x00\x00\x00\x04\x63\x6f\x64\x65\x00\x00\x00\x00\x00\x00\x00\x03\x69\x36\x34\x00\x00\x00\x00\x00\x00\x00\x0a\x45\x6d\x70\x74\x79\x4e\x61\x6d\x65\x64\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x10\x70\x61\x79\x6c\x6f\x61\x64\x73\x2e\x69\x6e\x73\x70\x65\x63\x74\x00\x00\x00\x00\x00\x00\x00\x07\x69\x6e\x73\x70\x65\x63\x74\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x04\x6e\x61\x6d\x65\x00\x00\x00\x00\x00\x00\x00\x06\x53\x74\x72\x69\x6e\x67\x00\x00\x00\x00\x00\x00\x00\x06\x53\x74\x72\x69\x6e\x67\x00\x00\x00\x00\x00\x00\x00\x0c\x50\x61\x79\x6c\x6f\x61\x64\x45\x72\x72\x6f\x72\x00\x00\x00\x00\x00\x00\x00\x05\x75\x6e\x61\x72\x79\x00\x00\x00\x00\x00\x00\x00\x08\x65\x78\x74\x65\x72\x6e\x61\x6c\x00\x00\x00\x00\x00\x00\x00\x04\x6e\x6f\x6e\x65";
        let contract = mixed_contract();
        let model = contract.model();
        let projection = schema::projection("payloads", model);
        assert_eq!(projection.len(), 571);
        assert_eq!(projection, PROJECTION);
        assert_eq!(
            [
                projection[121],
                projection[165],
                projection[262],
                projection[405]
            ],
            [0x00, 0x01, 0x02, 0x02]
        );
        let revision = schema::revision("payloads", model);
        let independently_hashed = format!("sha256:{:x}", Sha256::digest(PROJECTION));
        assert_eq!(
            independently_hashed,
            "sha256:ab76207e29cc030e0a072ccfad054352e67fd98871a84a1b820a162eb411597e"
        );
        assert_eq!(revision, <[u8; 32]>::from(Sha256::digest(PROJECTION)));
        assert_eq!(hash_hex(&revision), independently_hashed);
        assert_eq!(
            hash_hex(contract.semantic_digest()),
            "sha256:795b2224a1e4cc8360b5cf541499efad26adf3619bb4afa6748c159373d44806"
        );
        let document = schema::document(
            "payloads",
            model,
            &revision,
            contract.semantic_digest(),
            "0.0.0",
        );
        assert_eq!(document, MIXED_SCHEMA);
    }

    #[test]
    fn mixed_payload_mutations_change_the_public_revision() {
        let base = schema::revision("payloads", mixed_contract().model());
        assert_eq!(
            mixed_mutations()
                .iter()
                .map(|mutation| mutation.name)
                .collect::<Vec<_>>(),
            MIXED_MUTATION_NAMES
        );
        for mutation in mixed_mutations() {
            let mutated = mutation.apply(MIXED);
            assert_ne!(
                mixed_revision_of(&mutated),
                base,
                "mutation `{}` must change the public revision",
                mutation.name
            );
        }

        assert_ne!(
            mixed_revision_of(&mixed_error_rename()),
            base,
            "mutation `error_rename` must change the public revision"
        );
    }

    fn generated_document(box_id: &str, source: &str) -> SchemaDocument {
        let contract = scalar_model(source);
        let revision = schema::revision(box_id, contract.model());
        SchemaDocument::parse(&schema::document(
            box_id,
            contract.model(),
            &revision,
            contract.semantic_digest(),
            "0.0.0",
        ))
        .unwrap()
    }

    type ExpectedFinding = (
        &'static str,
        &'static str,
        boxology_classifier::Class,
        Option<&'static str>,
    );

    fn expected_findings(corpus: &str, name: &str) -> Vec<ExpectedFinding> {
        use boxology_classifier::Class::{
            Additive, CompatibleWithConditions, Deprecation, Documentation, Incompatible,
        };
        let conditional = Some("unknown-variant tolerance");
        match (corpus, name) {
            ("hello", "error_rename") => vec![
                ("BXC0044", "hello.greet/error", Incompatible, None),
                ("BXC0032", "hello/type/GreetError", Incompatible, None),
                ("BXC0031", "hello/type/HelloError", Additive, None),
            ],
            ("hello", "error_docs") => {
                vec![("BXC0033", "hello/type/GreetError", Documentation, None)]
            }
            ("hello", "error_deprecation") => {
                vec![("BXC0034", "hello/type/GreetError", Deprecation, None)]
            }
            ("hello", "variant_rename") => vec![
                (
                    "BXC0035",
                    "hello/type/GreetError/variant/EmptyName",
                    Incompatible,
                    None,
                ),
                (
                    "BXC0036",
                    "hello/type/GreetError/variant/MissingName",
                    CompatibleWithConditions,
                    conditional,
                ),
            ],
            ("hello", "variant_docs") => vec![(
                "BXC0033",
                "hello/type/GreetError/variant/EmptyName",
                Documentation,
                None,
            )],
            ("hello", "variant_deprecation") => vec![(
                "BXC0034",
                "hello/type/GreetError/variant/EmptyName",
                Deprecation,
                None,
            )],
            ("hello", "variant_added") => vec![(
                "BXC0036",
                "hello/type/GreetError/variant/Busy",
                CompatibleWithConditions,
                conditional,
            )],
            ("hello", "capability_docs") => vec![("BXC0033", "hello.greet", Documentation, None)],
            ("hello", "capability_deprecation") => {
                vec![("BXC0034", "hello.greet", Deprecation, None)]
            }
            ("hello", "capability_rename") => vec![
                ("BXC0040", "hello.greet", Incompatible, None),
                ("BXC0039", "hello.welcome", Additive, None),
            ],
            ("hello", "input_name") => vec![("BXC0041", "hello.greet/input", Incompatible, None)],
            ("mixed", "unit_to_value" | "named_empty_to_unit" | "value_type_u32_to_u64") => {
                vec![("BXC0052", "payloads.inspect/error", Incompatible, None)]
            }
            ("mixed", "value_to_unit" | "value_to_named") => vec![
                ("BXC0052", "payloads.inspect/error", Incompatible, None),
                (
                    "BXC0033",
                    "payloads/type/PayloadError/variant/Value",
                    Documentation,
                    None,
                ),
            ],
            ("mixed", "named_to_value") => vec![
                ("BXC0052", "payloads.inspect/error", Incompatible, None),
                (
                    "BXC0034",
                    "payloads/type/PayloadError/variant/Named",
                    Deprecation,
                    None,
                ),
            ],
            ("mixed", "named_empty_field_added") => vec![(
                "BXC0049",
                "payloads/type/PayloadError/variant/EmptyNamed/field/spare",
                Additive,
                None,
            )],
            ("mixed", "variant_order" | "field_order") => {
                vec![("BXC0028", "payloads", Incompatible, None)]
            }
            ("mixed", "field_type_string_to_bool") => vec![(
                "BXC0051",
                "payloads/type/PayloadError/variant/Named/field/message",
                Incompatible,
                None,
            )],
            ("mixed", "field_type_i64_to_u8") => vec![(
                "BXC0051",
                "payloads/type/PayloadError/variant/Named/field/code",
                Incompatible,
                None,
            )],
            ("mixed", "field_rename_message_to_text") => vec![
                (
                    "BXC0050",
                    "payloads/type/PayloadError/variant/Named/field/message",
                    Incompatible,
                    None,
                ),
                (
                    "BXC0049",
                    "payloads/type/PayloadError/variant/Named/field/text",
                    Additive,
                    None,
                ),
            ],
            ("mixed", "variant_docs" | "payload_docs") => vec![(
                "BXC0033",
                "payloads/type/PayloadError/variant/Value",
                Documentation,
                None,
            )],
            ("mixed", "payload_deprecation_removed" | "payload_note") => vec![(
                "BXC0034",
                "payloads/type/PayloadError/variant/Value",
                Deprecation,
                None,
            )],
            ("mixed", "variant_deprecation_removed") => vec![(
                "BXC0034",
                "payloads/type/PayloadError/variant/Named",
                Deprecation,
                None,
            )],
            ("mixed", "field_note") => vec![(
                "BXC0034",
                "payloads/type/PayloadError/variant/Named/field/code",
                Deprecation,
                None,
            )],
            ("mixed", "field_docs_added") => vec![(
                "BXC0033",
                "payloads/type/PayloadError/variant/Named/field/code",
                Documentation,
                None,
            )],
            ("mixed", "variant_removed") => vec![(
                "BXC0035",
                "payloads/type/PayloadError/variant/EmptyNamed",
                Incompatible,
                None,
            )],
            ("mixed", "variant_added") => vec![(
                "BXC0036",
                "payloads/type/PayloadError/variant/Other",
                CompatibleWithConditions,
                conditional,
            )],
            ("mixed", "error_rename") => vec![
                ("BXC0044", "payloads.inspect/error", Incompatible, None),
                ("BXC0032", "payloads/type/PayloadError", Incompatible, None),
                ("BXC0031", "payloads/type/StorageFault", Additive, None),
            ],
            _ => panic!("missing expected findings for {corpus}/{name}"),
        }
    }

    fn assert_classification(
        corpus: &str,
        name: &str,
        base: &SchemaDocument,
        submitted: &SchemaDocument,
    ) {
        let report = boxology_classifier::classify(Some(base), Some(submitted)).unwrap();
        let observed = report
            .findings()
            .iter()
            .map(|finding| {
                (
                    finding.code(),
                    finding.path(),
                    finding.class(),
                    finding.condition(),
                )
            })
            .collect::<Vec<_>>();
        let expected = expected_findings(corpus, name);
        assert_eq!(observed, expected, "mutation {corpus}/{name}");
        assert_ne!(
            report.verdict(),
            boxology_classifier::Class::Unchanged,
            "mutation {corpus}/{name}"
        );
    }

    #[test]
    fn classifier_maps_the_exact_generator_mutation_corpora() {
        assert_eq!(
            hello_mutations()
                .iter()
                .map(|mutation| mutation.name)
                .collect::<Vec<_>>(),
            HELLO_MUTATION_NAMES
        );
        let hello = generated_document("hello", CONTRACT);
        for mutation in hello_mutations() {
            assert_classification(
                "hello",
                mutation.name,
                &hello,
                &generated_document("hello", &mutation.apply(CONTRACT)),
            );
        }

        assert_eq!(
            mixed_mutations()
                .iter()
                .map(|mutation| mutation.name)
                .collect::<Vec<_>>(),
            MIXED_MUTATION_NAMES
        );
        let mixed = generated_document("payloads", MIXED);
        for mutation in mixed_mutations() {
            assert_classification(
                "mixed",
                mutation.name,
                &mixed,
                &generated_document("payloads", &mutation.apply(MIXED)),
            );
        }
        assert_classification(
            "mixed",
            "error_rename",
            &mixed,
            &generated_document("payloads", &mixed_error_rename()),
        );
    }

    fn assert_unchanged(base: &SchemaDocument, submitted: &SchemaDocument, label: &str) {
        let report = boxology_classifier::classify(Some(base), Some(submitted)).unwrap();
        assert!(report.findings().is_empty(), "negative control {label}");
        assert_eq!(
            report.verdict(),
            boxology_classifier::Class::Unchanged,
            "negative control {label}"
        );
    }

    #[test]
    fn classifier_ignores_generator_negative_controls() {
        let base = generated_document("payloads", MIXED);
        let decorated = "// ignored\nboxology::contract! { #[error] pub enum PayloadError { Unit, #[doc = \"value variant\"] Value(#[doc = \"value payload\"] #[deprecated(note = \"use detail\")] u32), #[deprecated(note = \"retired\")] Named { #[doc = \"message field\"] message: String, #[deprecated(note = \"use text\")] code: i64 }, EmptyNamed {} } /* ignored */ #[capability(exposure = external)] pub async fn inspect(name: String)->Result<String,PayloadError>; }";
        assert_unchanged(
            &base,
            &generated_document("payloads", decorated),
            "decorated source",
        );

        let nfc = MIXED.replace("message: String", "\u{e9}: String");
        let nfd = MIXED.replace("message: String", "e\u{301}: String");
        assert_unchanged(
            &generated_document("payloads", &nfc),
            &generated_document("payloads", &nfd),
            "NFC normalization",
        );

        let mut provenance = base.clone();
        provenance.provenance = boxology_schema::Provenance::new(json!({"generator": "different"}));
        assert_unchanged(&base, &provenance, "provenance");

        let canonical = base.canonical_bytes();
        let compact =
            serde_json::to_vec(&serde_json::from_slice::<Value>(&canonical).unwrap()).unwrap();
        assert_ne!(canonical, compact);
        assert_unchanged(
            &SchemaDocument::parse(&canonical).unwrap(),
            &SchemaDocument::parse(&compact).unwrap(),
            "stored encoding",
        );
    }

    #[test]
    fn mixed_payload_non_mutations_preserve_projection_and_document() {
        let contract = mixed_contract();
        let model = contract.model();
        let base_projection = schema::projection("payloads", model);
        let base_revision = schema::revision("payloads", model);
        let base_document = schema::document(
            "payloads",
            model,
            &base_revision,
            contract.semantic_digest(),
            "0.0.0",
        );

        let decorated = "// ignored\nboxology::contract! { #[error] pub enum PayloadError { Unit, #[doc = \"value variant\"] Value(#[doc = \"value payload\"] #[deprecated(note = \"use detail\")] u32), #[deprecated(note = \"retired\")] Named { #[doc = \"message field\"] message: String, #[deprecated(note = \"use text\")] code: i64 }, EmptyNamed {} } /* ignored */ #[capability(exposure = external)] pub async fn inspect(name: String)->Result<String,PayloadError>; }";
        assert_ne!(decorated, MIXED);
        let decorated_contract = scalar_model(decorated);
        assert_eq!(
            schema::projection("payloads", decorated_contract.model()),
            base_projection
        );
        assert_eq!(
            schema::document(
                "payloads",
                decorated_contract.model(),
                &base_revision,
                contract.semantic_digest(),
                "0.0.0",
            ),
            base_document
        );

        assert_unique("message: String");
        let nfc = MIXED.replace("message: String", "\u{e9}: String");
        let nfd = MIXED.replace("message: String", "e\u{301}: String");
        let nfc_contract = scalar_model(&nfc);
        let nfd_contract = scalar_model(&nfd);
        assert_eq!(
            schema::projection("payloads", nfc_contract.model()),
            schema::projection("payloads", nfd_contract.model())
        );
        assert_eq!(
            schema::document(
                "payloads",
                nfc_contract.model(),
                &schema::revision("payloads", nfc_contract.model()),
                &[0; 32],
                "0.0.0",
            ),
            schema::document(
                "payloads",
                nfd_contract.model(),
                &schema::revision("payloads", nfd_contract.model()),
                &[0; 32],
                "0.0.0",
            )
        );

        let changed_provenance =
            schema::document("payloads", model, &base_revision, &[7; 32], "9.9.9");
        assert_ne!(changed_provenance, base_document);
        assert_eq!(
            serde_json::from_slice::<Value>(&changed_provenance).unwrap()["revision"],
            serde_json::from_slice::<Value>(&base_document).unwrap()["revision"]
        );

        let value: Value = serde_json::from_slice(&base_document).unwrap();
        let compact = serde_json::to_vec(&value).unwrap();
        assert_ne!(Sha256::digest(&base_document), Sha256::digest(&compact));
        for document in [base_document.as_slice(), compact.as_slice()] {
            let parsed: Value = serde_json::from_slice(document).unwrap();
            let document_hash = format!("sha256:{:x}", Sha256::digest(document));
            assert_eq!(
                parsed["revision"],
                "sha256:ab76207e29cc030e0a072ccfad054352e67fd98871a84a1b820a162eb411597e"
            );
            assert_ne!(document_hash, parsed["revision"].as_str().unwrap());
        }
    }

    #[test]
    fn mixed_payload_document_round_trips_through_the_strict_reader() {
        let contract = mixed_contract();
        let model = contract.model();
        let document_bytes = schema::document(
            "payloads",
            model,
            &schema::revision("payloads", model),
            contract.semantic_digest(),
            "0.0.0",
        );
        let expected = SchemaDocument::parse(MIXED_SCHEMA).unwrap();
        assert_eq!(document_bytes, MIXED_SCHEMA);
        assert_eq!(SchemaDocument::parse(&document_bytes).unwrap(), expected);
        assert_eq!(expected.types[0].variants[0].payload, SchemaPayload::Unit);
        assert_eq!(
            expected.types[0].variants[3].payload,
            SchemaPayload::Named(Vec::new())
        );
        assert_ne!(SchemaPayload::Unit, SchemaPayload::Named(Vec::new()));
        let SchemaPayload::Named(fields) = &expected.types[0].variants[2].payload else {
            panic!("Named variant must carry named fields");
        };
        assert_eq!(fields[0].name, "message");
        assert_eq!(fields[1].name, "code");
    }

    #[test]
    fn mixed_payload_variants_still_fail_generate_with_bxg0048() {
        let diagnostics = generate(request(MIXED, false, OUTPUTS.to_vec())).unwrap_err();
        assert_eq!(
            diagnostics
                .as_slice()
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>(),
            ["BXG0048"]
        );
        assert_eq!(diagnostics.as_slice().len(), 1);
    }

    #[test]
    fn semantic_change_updates_rust_name_and_digest_marker() {
        let changed = CONTRACT.replace("EmptyName", "MissingName");
        let before = tree(CONTRACT, false);
        let after = tree(&changed, false);
        assert_eq!(
            file(&before, "generated/contract/Cargo.toml"),
            file(&after, "generated/contract/Cargo.toml")
        );
        let before = marker_parts(file(&before, "generated/contract/src/lib.rs").bytes());
        let after = marker_parts(file(&after, "generated/contract/src/lib.rs").bytes());
        assert!(before.0.contains("EmptyName"));
        assert!(after.0.contains("MissingName"));
        assert_eq!(before.2, after.2);
        assert_ne!(before.1, after.1);
    }

    #[test]
    fn public_error_preserves_decoded_metadata() {
        let source = "boxology::contract! { #[doc = \"failure\"] #[deprecated(note = \"old\")] #[error] pub enum GreetError { #[doc = \"empty\"] #[deprecated] EmptyName } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
        let generated = tree(source, false);
        let rust =
            std::str::from_utf8(file(&generated, "generated/contract/src/lib.rs").bytes()).unwrap();
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
    fn structured_type_template_is_public_ordered_and_byte_locked() {
        const SOURCE: &str = r#"boxology::contract! {
            pub struct Empty {}
            #[doc = "mood"] pub enum Mood { Calm, #[deprecated(note = "avoid")] Busy }
            #[deprecated] pub struct Profile {
                #[doc = "name"] pub name: String,
                pub scores: Vec<u32>,
                pub mood: Option<Mood>,
                pub history: Option<Vec<Mood>>,
            }
            #[error] pub enum Fault { Bad }
            #[capability] pub async fn save(input: Profile) -> Result<Profile, Fault>;
        }"#;
        const SOURCE_SHA256: &str =
            "1d310b0c762fceec5596e51e55144c6fc615b52ee8b37c718a7269bed4fd5ef5";
        let contract = scalar_model(SOURCE);
        let source = structured_types_source(contract.model());
        assert_eq!(source, structured_types_source(contract.model()));
        let printed = prettyplease::unparse(&syn::parse_file(&source).unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(printed.as_bytes())),
            SOURCE_SHA256
        );

        let positions = ["pub struct Empty", "pub enum Mood", "pub struct Profile"].map(|text| {
            printed
                .find(text)
                .unwrap_or_else(|| panic!("missing `{text}` in {printed}"))
        });
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        for expected in [
            "#[derive(Debug, Clone, PartialEq)]\npub struct Empty {}",
            "let fields = ::std::vec::Vec::new();",
            "///mood\n#[derive(Debug, Clone, PartialEq)]\npub enum Mood",
            "#[deprecated]\n#[derive(Debug, Clone, PartialEq)]\npub struct Profile",
            "///name\n    pub name: ::std::string::String",
            "pub scores: ::std::vec::Vec<u32>",
            "pub mood: ::core::option::Option<Mood>",
            "pub history: ::core::option::Option<::std::vec::Vec<Mood>>",
            "Self::Calm => (\"Calm\".into()",
            "Self::Busy => (\"Busy\".into()",
            "Unknown {",
            "OpaquePayload",
            "ValueRef::Opaque(payload)",
            "::boxology_contract::ContractValue::enum_value(",
            "::boxology_contract::SlotValue::Null",
            "DecodeErrorKind::UnexpectedPayload",
            "DecodeErrorKind::UnknownVariant(",
            "DecodeErrorKind::UnknownField(",
            "\"name\" | \"scores\" | \"mood\" | \"history\" => {}",
        ] {
            assert!(
                printed.contains(expected),
                "missing `{expected}` in {printed}"
            );
        }
        let field_order = ["&self.name", "&self.scores", "&self.mood", "&self.history"]
            .map(|text| printed.find(text).unwrap());
        assert!(field_order.windows(2).all(|pair| pair[0] < pair[1]));

        generate(request(SOURCE, false, OUTPUTS.to_vec()))
            .expect("the accepted structured subset generates");
    }

    #[test]
    fn structured_descriptors_and_call_glue_are_recursive_and_site_qualified() {
        const SOURCE: &str = r#"boxology::contract! {
            pub enum Mood { Calm, #[deprecated(note = "avoid")] Busy }
            pub struct Profile {
                pub name: String,
                #[deprecated] pub mood: Option<Mood>,
            }
            #[error] pub enum Fault { Bad }
            #[capability] pub async fn save(input: Profile) -> Result<Option<Vec<Profile>>, Fault>;
        }"#;
        let parsed = scalar_model(SOURCE);
        let contract = parsed.model();
        let output = &contract.capabilities[0].output_type;
        assert_eq!(
            schema::type_descriptor_source(contract, output, "::boxology_contract::"),
            concat!(
                "::boxology_contract::TypeDescriptor::optional(",
                "::boxology_contract::TypeDescriptor::list(",
                "::boxology_contract::TypeDescriptor::structure([",
                "::boxology_contract::FieldDescriptor::new(\"name\", ::boxology_contract::TypeDescriptor::string(), None),",
                "::boxology_contract::FieldDescriptor::new(\"mood\", ",
                "::boxology_contract::TypeDescriptor::optional(",
                "::boxology_contract::TypeDescriptor::enumeration([",
                "::boxology_contract::VariantDescriptor::new(\"Calm\", ::boxology_contract::VariantPayload::Unit, None),",
                "::boxology_contract::VariantDescriptor::new(\"Busy\", ::boxology_contract::VariantPayload::Unit, ",
                "Some(::boxology_contract::Deprecation::new(Some(\"avoid\".into())))),",
                "]).expect(\"generated enum descriptor is valid\")",
                ").expect(\"generated optional descriptor is valid\"), ",
                "Some(::boxology_contract::Deprecation::new(None))),",
                "]).expect(\"generated struct descriptor is valid\")",
                ").expect(\"generated list descriptor is valid\")",
                ").expect(\"generated optional descriptor is valid\")",
            )
        );

        let descriptor = schema::descriptor_source(
            "profiles",
            contract,
            &schema::revision("profiles", contract),
        );
        let checker = checker_source("profiles", contract);
        let dispatch = dispatch_source("profiles", contract);
        let fake = test_support_source("profiles", contract);
        let adapter = adapter_source("profiles", contract, &[]);
        for source in [&descriptor, &checker, &dispatch, &fake, &adapter] {
            syn::parse_file(source).unwrap_or_else(|error| panic!("{error}: {source}"));
        }
        for expected in [
            "$crate::Profile",
            "::core::option::Option<::std::vec::Vec<$crate::Profile>>",
        ] {
            assert!(
                checker.contains(expected),
                "missing `{expected}` in {checker}"
            );
        }
        for expected in [
            "input: Profile",
            "Result<::core::option::Option<::std::vec::Vec<Profile>>, Fault>",
            "<::core::option::Option<::std::vec::Vec<Profile>> as ContractType>::decode(&output)",
        ] {
            assert!(
                dispatch.contains(expected),
                "missing `{expected}` in {dispatch}"
            );
        }
        for expected in [
            "Fn(CallContext, super::Profile)",
            "Result<::core::option::Option<::std::vec::Vec<super::Profile>>, Fault>",
            "<super::Profile as ContractType>::decode(&input)",
        ] {
            assert!(fake.contains(expected), "missing `{expected}` in {fake}");
        }
        assert!(adapter.contains(
            "<::boxology_generated_contract::Profile as ::boxology_contract::ContractType>::decode(&input)"
        ));
        assert!(descriptor.contains("generated optional descriptor is valid"));

        generate(request(SOURCE, false, OUTPUTS.to_vec()))
            .expect("structured descriptors and call glue generate together");
        // `cold_hello_bytes_are_exact_and_parseable` remains the byte lock for scalar output.
    }

    #[test]
    #[ignore = "deep cold structured generation proof runs explicitly and in main-push --no-budget CI"]
    fn generated_structured_contract_routes_and_preserves_unknown_outputs() {
        use std::{fs, process::Command};
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let source = r#"boxology::contract! {
            pub struct Empty {}
            pub enum Mood { Calm, Busy }
            pub struct Profile { pub name: String, pub mood: Mood, pub history: Option<Vec<Mood>> }
            #[error] pub enum Fault { Bad }
            #[capability(exposure=external)] pub async fn echo_mood(input: Mood) -> Result<Mood, Fault>;
            #[capability(exposure=external)] pub async fn echo_profiles(input: Option<Vec<Profile>>) -> Result<Option<Vec<Profile>>, Fault>;
        }"#;
        let generated = generate(request_for("shapes", source)).unwrap();
        assert_eq!(generated, generate(request_for("shapes", source)).unwrap());
        let schema = SchemaDocument::parse(file(&generated, "generated/schema.json").bytes())
            .expect("generated structured schema must read");
        let empty = schema
            .data_types
            .iter()
            .find(|item| item.name == "Empty")
            .unwrap();
        assert_eq!(
            empty.shape,
            boxology_schema::SchemaDataShape::Struct(Vec::new())
        );
        let mood = schema
            .data_types
            .iter()
            .find(|item| item.name == "Mood")
            .unwrap();
        let boxology_schema::SchemaDataShape::Enum(variants) = &mood.shape else {
            panic!("Mood must be a schema enum")
        };
        assert_eq!(
            variants
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>(),
            ["Calm", "Busy"]
        );
        assert_eq!(
            schema.types[0]
                .variants
                .iter()
                .map(|variant| variant.name.as_str())
                .collect::<Vec<_>>(),
            ["Bad"]
        );
        let root = std::env::temp_dir().join(format!(
            "boxology-structured-e2e-{}-{}",
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
                "[workspace]\nmembers=[\"generated/contract\",\"implementation\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology={{path={:?}}}\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\nboxology-runtime={{version=\"=0.1.0\",path={:?},features=[\"test-support\"]}}\n",
                workspace.join("boxology"),
                workspace.join("boxology-contract"),
                workspace.join("boxology-runtime"),
            ),
        )
        .unwrap();
        let implementation = root.join("implementation");
        fs::create_dir_all(implementation.join("src")).unwrap();
        fs::write(
            implementation.join("Cargo.toml"),
            "[package]\nname=\"shapes-implementation\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology={workspace=true}\nboxology-contract={workspace=true}\nboxology-runtime={workspace=true}\nboxology_generated_contract={package=\"shapes-contract\",path=\"../generated/contract\",features=[\"test-support\"]}\n",
        )
        .unwrap();
        fs::write(
            implementation.join("src/main.rs"),
            r#"
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Waker};
use boxology_contract::{CallContext, CallError, Caller, CancelToken, CapabilityId, ContractType, ContractValue, DecodeErrorKind, DescriptorRef, ErasedCallError, ErasedCallTarget, ExposureLevel, OpaqueTree, PathSegment, SlotValue, TraceContext};
use boxology_runtime::{CompositionBuilder, test_support::StubTransport};
use boxology_generated_contract::{Empty, Fault, Mood, Profile, ShapesHandle, test_support::ShapesFake};

pub struct ShapesService;

#[boxology::implementation]
impl ShapesService {
    pub async fn echo_mood(&self, context: CallContext, input: Mood) -> Result<Mood, Fault> { let _ = context; Ok(input) }
    pub async fn echo_profiles(&self, context: CallContext, input: Option<Vec<Profile>>) -> Result<Option<Vec<Profile>>, Fault> { let _ = context; Ok(input) }
}

mod generated { include!("../../generated/adapter/adapter.rs"); }

#[derive(Clone)]
struct Target(SlotValue);
impl ErasedCallTarget for Target {
    fn call<'a>(&'a self, _: &'a CapabilityId, _: CallContext, _: SlotValue) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        Box::pin(ready(Ok(self.0.clone())))
    }
}

fn context() -> CallContext {
    CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None)
}
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop { if let Poll::Ready(output) = future.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) { return output; } }
}
fn handle(output: SlotValue) -> ShapesHandle { ShapesHandle::from_erased(Arc::new(Target(output))) }

fn main() {
    const SENTINEL: &str = "unknown-generated-mood-payload";
    let empty = Empty {};
    assert_eq!(empty.encode().unwrap(), SlotValue::Value(ContractValue::object(Vec::<(String, ContractValue)>::new()).unwrap()));
    assert_eq!(Empty::decode(&empty.encode().unwrap()).unwrap(), empty);
    let descriptor = boxology_generated_contract::contract_descriptor();
    let DescriptorRef::Enum(direct) = descriptor.capabilities()[0].output().view() else { panic!() };
    assert_eq!(direct.iter().map(|variant| variant.tag()).collect::<Vec<_>>(), ["Calm", "Busy"]);
    let DescriptorRef::Enum(errors) = descriptor.capabilities()[0].error().view() else { panic!() };
    assert_eq!(errors.iter().map(|variant| variant.tag()).collect::<Vec<_>>(), ["Bad"]);
    let DescriptorRef::Optional(list) = descriptor.capabilities()[1].output().view() else { panic!() };
    let DescriptorRef::List(profile) = list.view() else { panic!() };
    let DescriptorRef::Struct(fields) = profile.view() else { panic!() };
    let DescriptorRef::Enum(nested) = fields.iter().find(|field| field.name() == "mood").unwrap().descriptor().view() else { panic!() };
    assert_eq!(nested.iter().map(|variant| variant.tag()).collect::<Vec<_>>(), ["Calm", "Busy"]);

    let profile = Profile { name: "Ada".into(), mood: Mood::Calm, history: Some(vec![Mood::Busy]) };
    let fake = ShapesFake::new()
        .with_echo_mood(|_, input| async move { Ok(input) })
        .with_echo_profiles(|_, input| async move { Ok(input) });
    assert_eq!(block_on(fake.handle().echo_mood(context(), Mood::Busy)), Ok(Mood::Busy));
    assert_eq!(block_on(fake.handle().echo_profiles(context(), Some(vec![profile.clone()]))), Ok(Some(vec![profile.clone()])));

    let direct_capability = descriptor.capabilities()[0].id().clone();
    let nested_capability = descriptor.capabilities()[1].id().clone();
    let transport = Arc::new(StubTransport::new());
    let mut builder = CompositionBuilder::new();
    builder.add_box(generated::implementation_descriptor(), |imports| generated::factory(ShapesService, imports));
    builder.expose(boxology_contract::BoxId::new("shapes").unwrap(), direct_capability, transport.clone(), ExposureLevel::External);
    builder.expose(boxology_contract::BoxId::new("shapes").unwrap(), nested_capability, transport.clone(), ExposureLevel::External);
    let composition = builder.start().unwrap();
    let runtime = transport.runtime().unwrap();
    let exposures = runtime.exposures();
    let direct = exposures.iter().find(|item| item.descriptor().id().to_string() == "shapes.echo_mood").unwrap();
    let nested = exposures.iter().find(|item| item.descriptor().id().to_string() == "shapes.echo_profiles").unwrap();
    assert_eq!(Mood::decode(&block_on(direct.dispatch(context(), Mood::Calm.encode().unwrap())).unwrap()).unwrap(), Mood::Calm);
    let profiles = Some(vec![profile.clone()]);
    let output = block_on(nested.dispatch(context(), profiles.encode().unwrap())).unwrap();
    assert_eq!(<Option<Vec<Profile>> as ContractType>::decode(&output).unwrap(), profiles);

    let raw_mood = SlotValue::Value(ContractValue::enum_value("Future", SlotValue::Value(ContractValue::string(SENTINEL))));
    let raw_profile = ContractValue::object([
        ("name".into(), ContractValue::string("Ada")),
        ("mood".into(), ContractValue::enum_value("Future", SlotValue::Value(ContractValue::string(SENTINEL)))),
    ]).unwrap();
    let raw_profiles = SlotValue::Value(ContractValue::list([raw_profile.clone()]));
    for rejected in [block_on(direct.dispatch(context(), raw_mood.clone())), block_on(nested.dispatch(context(), raw_profiles.clone()))] {
        let Err(ErasedCallError::ContractViolation(detail)) = rejected else { panic!("unknown provider input was accepted") };
        assert_eq!(detail.code(), "input_decode");
        assert!(!format!("{detail:?}").contains(SENTINEL));
    }

    let direct_raw_error = Mood::decode(&raw_mood).unwrap_err();
    assert_eq!(direct_raw_error.kind(), &DecodeErrorKind::UnknownVariant("Future".into()));
    let nested_raw_error = <Option<Vec<Profile>> as ContractType>::decode(&raw_profiles).unwrap_err();
    assert_eq!(nested_raw_error.kind(), &DecodeErrorKind::UnknownVariant("Future".into()));
    let known_payload = ContractValue::enum_value("Calm", SlotValue::Value(ContractValue::bool(true)));
    assert_eq!(Mood::decode_value(&known_payload).unwrap_err().kind(), &DecodeErrorKind::UnexpectedPayload);

    let unknown = block_on(handle(raw_mood).echo_mood(context(), Mood::Calm)).unwrap();
    let Mood::Unknown { tag, payload } = &unknown else { panic!() };
    assert_eq!(tag, "Future");
    assert_eq!(payload.reveal(), &OpaqueTree::String(SENTINEL.into()));
    assert_eq!(format!("{payload:?}"), "OpaquePayload(<redacted>)");
    assert!(!format!("{unknown:?}").contains(SENTINEL));
    assert_eq!(Mood::decode(&unknown.encode().unwrap()).unwrap(), unknown);

    let unknown_profiles = block_on(handle(raw_profiles).echo_profiles(context(), Some(Vec::new()))).unwrap();
    let Mood::Unknown { tag, payload } = &unknown_profiles.as_ref().unwrap()[0].mood else { panic!() };
    assert_eq!(tag, "Future");
    assert_eq!(payload.reveal(), &OpaqueTree::String(SENTINEL.into()));
    assert!(!format!("{unknown_profiles:?}").contains(SENTINEL));
    let encoded = unknown_profiles.encode().unwrap();
    assert_eq!(<Option<Vec<Profile>> as ContractType>::decode(&encoded).unwrap(), unknown_profiles);

    let malformed = SlotValue::Value(ContractValue::string("wrong"));
    let Err(CallError::InvalidResponse(detail)) = block_on(handle(malformed).echo_mood(context(), Mood::Calm)) else { panic!("malformed output was accepted") };
    assert_eq!(detail.code(), "output_decode");
    assert_eq!(direct_raw_error.path(), &[PathSegment::Variant("Future".into())]);
    drop(composition);
}
"#,
        )
        .unwrap();
        let output = Command::new("cargo")
            .args(["run", "--offline", "--manifest-path"])
            .arg(implementation.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("target"))
            .output()
            .unwrap();
        let _ = fs::remove_dir_all(&root);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    #[ignore = "deep nested-Cargo matrix runs in main-push --no-budget CI"]
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
                "[workspace]\nmembers=[\"generated/contract\",\"consumer\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\n",
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
                "boxology::contract! {{ /* spelling is irrelevant */\n#[error] pub enum {error} {{ {variants} }}\n#[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, {error}>; }}\nuse boxology_contract::{{CapabilityShape, ContractError, ContractType, ContractValue, DecodeErrorKind, DescriptorRef, ExposureLevel, Idempotency, ImplementationDescriptor, OpaquePayload, OpaqueTree, PathSegment, SlotValue, VariantPayload}};\nfn main() {{ {body} }}\n"
            )
        };
        let abi = r#"
            let descriptor = boxology_generated_contract::contract_descriptor();
            assert!(std::ptr::eq(descriptor, boxology_generated_contract::contract_descriptor()));
            assert_eq!(descriptor.box_id().as_str(), "hello");
            assert_eq!(descriptor.revision().as_str(), __BASE_REVISION__);
            assert_eq!(descriptor.capabilities().len(), 1);
            let capability = &descriptor.capabilities()[0];
            assert_eq!(capability.id().box_id().as_str(), "hello");
            assert_eq!(capability.id().name().as_str(), "greet");
            assert!(matches!(capability.input().view(), DescriptorRef::String));
            assert!(matches!(capability.output().view(), DescriptorRef::String));
            assert!(matches!(capability.error().view(), DescriptorRef::Enum(variants) if variants.len() == 1 && variants[0].tag() == "EmptyName" && matches!(variants[0].payload(), VariantPayload::Unit) && variants[0].deprecation().is_none()));
            assert_eq!(capability.shape(), CapabilityShape::Unary);
            assert_eq!(capability.max_exposure(), ExposureLevel::External);
            assert_eq!(capability.idempotency(), Idempotency::None);
            assert!(capability.deprecation().is_none());
            let implementation = ImplementationDescriptor::new(descriptor, []).unwrap();
            assert!(std::ptr::eq(implementation.contract(), descriptor));
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
        "#
        .replace("__BASE_REVISION__", &format!("{:?}", revision(CONTRACT)));
        fs::write(
            consumer.join("src/main.rs"),
            source("GreetError", "EmptyName", abi.clone()),
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
                "let _ = GreetError::EmptyName;".into(),
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
        let renamed_body = format!(
            "let descriptor = boxology_generated_contract::contract_descriptor(); assert_ne!(descriptor.revision().as_str(), {:?}); assert_eq!(descriptor.revision().as_str(), {:?}); assert!(matches!(descriptor.capabilities()[0].error().view(), DescriptorRef::Enum(variants) if variants.len() == 2 && variants[1].tag() == \"Busy\")); let value = HelloFailure::Busy; assert_eq!(value.error_tag(), \"Busy\"); assert_eq!(HelloFailure::decode_value(&value.encode_value().unwrap()).unwrap(), value); let _: boxology_generated_contract::HelloFailure = value;",
            revision(CONTRACT),
            revision(&renamed),
        );
        fs::write(
            consumer.join("src/main.rs"),
            source("HelloFailure", "EmptyName, Busy", renamed_body),
        )
        .unwrap();
        assert!(cargo("run", &manifest, "consumer-target").status.success());
        let decorated = "boxology::contract! { #[error] pub enum GreetError { #[deprecated] EmptyName } #[deprecated(note = \"later\")] #[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, GreetError>; }";
        for file in tree(decorated, false).files() {
            fs::write(root.join(file.path()), file.bytes()).unwrap();
        }
        let decorated_body = format!(
            "let descriptor = boxology_generated_contract::contract_descriptor(); assert_eq!(descriptor.revision().as_str(), {:?}); assert_eq!(descriptor.capabilities()[0].deprecation().unwrap().note(), Some(\"later\")); match descriptor.capabilities()[0].error().view() {{ DescriptorRef::Enum(variants) => assert_eq!(variants[0].deprecation().unwrap().note(), None), _ => panic!(), }};",
            revision(decorated),
        );
        let decorated_source = format!(
            "boxology::contract! {{ #[error] pub enum GreetError {{ #[deprecated] EmptyName }} #[deprecated(note = \"later\")] #[capability(exposure = external)] pub async fn greet(name: String) -> Result<String, GreetError>; }}\nuse boxology_contract::{{DescriptorRef}};\nfn main() {{ {decorated_body} }}\n"
        );
        fs::write(consumer.join("src/main.rs"), decorated_source).unwrap();
        assert!(
            cargo("run", &manifest, "consumer-metadata-target")
                .status
                .success()
        );

        let value_payload = "boxology::contract! { #[error] pub enum Fault { Code(u32), Empty } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,Fault>; }";
        for file in tree(value_payload, false).files() {
            fs::write(root.join(file.path()), file.bytes()).unwrap();
        }
        let value_body = r#"
            use boxology_contract::{CallError, ErasedCallError};
            let descriptor = boxology_generated_contract::contract_descriptor();
            let fault_descriptor = descriptor.capabilities()[0].error();
            assert!(matches!(
                fault_descriptor.view(),
                DescriptorRef::Enum(variants)
                    if variants.len() == 2
                        && variants[0].tag() == "Code"
                        && matches!(variants[0].payload(), VariantPayload::Value(_))
                        && variants[1].tag() == "Empty"
                        && matches!(variants[1].payload(), VariantPayload::Unit)
            ));
            let known = Fault::Code(7);
            let encoded = known.encode_value().unwrap();
            assert_eq!(
                encoded,
                ContractValue::enum_value("Code", SlotValue::Value(ContractValue::u64(7)))
            );
            assert_eq!(Fault::decode_value(&encoded).unwrap(), known);
            assert_eq!(known.error_tag(), "Code");
            let unit_payload = ContractValue::enum_value(
                "Empty",
                SlotValue::Value(ContractValue::u64(1)),
            );
            let unexpected = Fault::decode_value(&unit_payload).unwrap_err();
            assert_eq!(unexpected.kind(), &DecodeErrorKind::UnexpectedPayload);
            assert_eq!(unexpected.path(), &[PathSegment::Variant("Empty".into())]);
            let unknown = Fault::Unknown {
                tag: "Future".into(),
                payload: OpaquePayload::new(OpaqueTree::String("secret".into())),
            };
            assert_eq!(
                Fault::decode_value(&unknown.encode_value().unwrap()).unwrap(),
                unknown
            );
            let erased = ErasedCallError::from_domain(&known);
            assert_eq!(
                erased.into_typed::<Fault>(fault_descriptor),
                CallError::Domain(Fault::Code(7))
            );
            let _: boxology_generated_contract::Fault = known;
        "#;
        fs::write(
            consumer.join("src/main.rs"),
            source("Fault", "Code(u32), Empty", value_body.to_string()),
        )
        .unwrap();
        let value_run = cargo("run", &manifest, "consumer-value-target");
        assert!(
            value_run.status.success(),
            "{}",
            String::from_utf8_lossy(&value_run.stderr)
        );

        let path_locality = "boxology::contract! { #[error] pub enum Fault { Bad(f32) } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,Fault>; }";
        for file in tree(path_locality, false).files() {
            fs::write(root.join(file.path()), file.bytes()).unwrap();
        }
        let path_body = r#"
            use boxology_contract::{EncodeErrorKind, PathSegment};
            let error = Fault::Bad(f32::NAN).encode_value().unwrap_err();
            assert_eq!(error.kind(), &EncodeErrorKind::NonFiniteF32);
            assert_eq!(error.path(), &[PathSegment::Variant("Bad".into())]);
            let mismatched = ContractValue::enum_value(
                "Bad",
                SlotValue::Value(ContractValue::string("not-a-float")),
            );
            let decode_error = Fault::decode_value(&mismatched).unwrap_err();
            assert_eq!(decode_error.path(), &[PathSegment::Variant("Bad".into())]);
        "#;
        fs::write(
            consumer.join("src/main.rs"),
            source("Fault", "Bad(f32)", path_body.to_string()),
        )
        .unwrap();
        let path_run = cargo("run", &manifest, "consumer-path-target");
        assert!(
            path_run.status.success(),
            "{}",
            String::from_utf8_lossy(&path_run.stderr)
        );
        fs::write(
            consumer.join("src/main.rs"),
            source(
                "Fault",
                "Code(String), Empty",
                "let _ = Fault::Code(String::from(\"stale\"));".to_string(),
            ),
        )
        .unwrap();
        let stale_payload = cargo("check", &manifest, "consumer-value-target");
        assert!(!stale_payload.status.success());
        assert!(
            String::from_utf8_lossy(&stale_payload.stderr)
                .contains("Boxology generated contract is stale")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "deep nested-Cargo matrix runs in main-push --no-budget CI"]
    fn generated_dispatch_handle_compiles_and_routes_typed_calls() {
        use std::{fs, process::Command};
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "boxology-dispatch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        for file in tree(CONTRACT, false).files() {
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
                "[workspace]\nmembers=[\"generated/contract\",\"consumer\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\n",
                workspace.join("boxology-contract")
            ),
        )
        .unwrap();
        let consumer = root.join("consumer");
        fs::create_dir_all(consumer.join("src")).unwrap();
        fs::write(
            consumer.join("Cargo.toml"),
            "[package]\nname=\"consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology-contract={workspace=true}\nhello-contract={package=\"hello-contract\",path=\"../generated/contract\",features=[\"test-support\"]}\n",
        )
        .unwrap();
        fs::write(
            consumer.join("src/main.rs"),
            r#"
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Waker};
use boxology_contract::{CallContext, CallError, Caller, CancelToken, CapabilityId, ContractValue, Deadline, Detail, ErasedCallError, ErasedCallTarget, IdempotencyKey, OpaqueTree, SlotValue, TraceContext};
use hello_contract::{test_support::HelloFake, GreetError, HelloDispatch, HelloHandle};

struct Target { response: Result<SlotValue, ErasedCallError>, expected: CallContext }
impl ErasedCallTarget for Target {
    fn call<'a>(&'a self, capability: &'a CapabilityId, context: CallContext, input: SlotValue) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        assert_eq!(capability.to_string(), "hello.greet");
        assert_eq!(input, SlotValue::Value(ContractValue::string("Ada")));
        assert_eq!(context.caller(), self.expected.caller());
        assert_eq!(context.deadline(), self.expected.deadline());
        assert_eq!(context.cancellation().is_cancelled(), self.expected.cancellation().is_cancelled());
        assert_eq!(context.trace(), self.expected.trace());
        assert_eq!(context.idempotency_key(), self.expected.idempotency_key());
        Box::pin(ready(self.response.clone()))
    }
}

struct Probe;
impl HelloDispatch for Probe {
    fn greet<'a>(&'a self, _context: CallContext, name: String) -> Pin<Box<dyn Future<Output = Result<String, GreetError>> + Send + 'a>> {
        Box::pin(async move { Ok(format!("Hello, {name}!")) })
    }
}

fn context() -> CallContext {
    let token = CancelToken::new();
    token.cancel();
    CallContext::new(Caller::System("generator-test"), Some(Deadline::at(std::time::Instant::now())), token, TraceContext::new(Some("parent".into()), Some("state".into())), Some(IdempotencyKey::new("id-1").unwrap()))
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop { if let Poll::Ready(output) = future.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) { return output; } }
}

fn invoke(response: Result<SlotValue, ErasedCallError>) -> Result<String, CallError<GreetError>> {
    let expected = context();
    let target: Arc<dyn ErasedCallTarget> = Arc::new(Target { response, expected: expected.clone() });
    block_on(HelloHandle::from_erased(target).greet(expected, "Ada".into()))
}

fn main() {
    fn bounds<T: Send + Sync + 'static>() {}
    fn clone_default<T: Clone + Default>() {}
    fn send<T: Send>(value: T) -> T { value }
    bounds::<HelloHandle>();
    bounds::<HelloFake>();
    clone_default::<HelloFake>();
    bounds::<Arc<dyn HelloDispatch>>();
    let dispatch: Arc<dyn HelloDispatch> = Arc::new(Probe);
    assert_eq!(block_on(send(dispatch.greet(context(), "Ada".into()))), Ok("Hello, Ada!".into()));

    let expected = context();
    let fake = HelloFake::new().with_greet({
        let expected = expected.clone();
        move |actual, name| {
            let expected = expected.clone();
            async move {
                assert_eq!(actual.caller(), expected.caller());
                assert_eq!(actual.deadline(), expected.deadline());
                assert_eq!(actual.cancellation().is_cancelled(), expected.cancellation().is_cancelled());
                assert_eq!(actual.trace(), expected.trace());
                assert_eq!(actual.idempotency_key(), expected.idempotency_key());
                Ok(format!("Hello, {name}!"))
            }
        }
    });
    let fake_handle = fake.handle();
    assert_eq!(block_on(send(fake_handle.greet(expected, "Ada".into()))), Ok("Hello, Ada!".into()));
    let domain_fake = HelloFake::new().with_greet(|_, _| async { Err(GreetError::EmptyName) });
    assert_eq!(block_on(domain_fake.handle().greet(context(), "Ada".into())), Err(CallError::Domain(GreetError::EmptyName)));
    let Err(CallError::Internal(detail)) = block_on(HelloFake::new().handle().greet(context(), "Ada".into())) else { panic!("unprogrammed fake did not return an internal call error") };
    assert_eq!(detail.code(), "unprogrammed_capability");

    assert_eq!(invoke(Ok(SlotValue::Value(ContractValue::string("Hello, Ada!")))), Ok("Hello, Ada!".into()));
    assert_eq!(invoke(Err(ErasedCallError::Domain { error_tag: "EmptyName".into(), payload: SlotValue::Null })), Err(CallError::Domain(GreetError::EmptyName)));
    let raw = ContractValue::object([("secret".into(), ContractValue::string("opaque-secret"))]).unwrap();
    let error = invoke(Err(ErasedCallError::Domain { error_tag: "Future".into(), payload: SlotValue::Value(raw) })).unwrap_err();
    let CallError::Domain(GreetError::Unknown { tag, payload }) = &error else { panic!("unknown domain error was not preserved") };
    assert_eq!(tag, "Future");
    assert_eq!(payload.forward().reveal(), &OpaqueTree::Object(vec![("secret".into(), OpaqueTree::String("opaque-secret".into()))]));
    assert!(!format!("{error:?}").contains("opaque-secret"));
    for output in [SlotValue::Null, SlotValue::Value(ContractValue::u64(7))] {
        let Err(CallError::InvalidResponse(detail)) = invoke(Ok(output)) else { panic!("malformed output accepted") };
        assert_eq!(detail.code(), "output_decode");
    }
    let Err(CallError::InvalidResponse(detail)) = invoke(Err(ErasedCallError::Domain { error_tag: "EmptyName".into(), payload: SlotValue::Value(ContractValue::string("wrong")) })) else { panic!("malformed domain error accepted") };
    assert_eq!(detail.code(), "domain_error_decode");
    let detail = Detail::new("preserved");
    for (erased, expected) in [
        (ErasedCallError::Deadline, CallError::Deadline),
        (ErasedCallError::Cancelled, CallError::Cancelled),
        (ErasedCallError::Unavailable(detail.clone()), CallError::Unavailable(detail.clone())),
        (ErasedCallError::ContractViolation(detail.clone()), CallError::ContractViolation(detail.clone())),
        (ErasedCallError::InvalidResponse(detail.clone()), CallError::InvalidResponse(detail.clone())),
        (ErasedCallError::Internal(detail.clone()), CallError::Internal(detail.clone())),
    ] { assert_eq!(invoke(Err(erased)), Err(expected)); }
}
"#,
        )
        .unwrap();
        let default_status = Command::new("cargo")
            .args(["check", "--offline", "--manifest-path"])
            .arg(root.join("generated/contract/Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("default-target"))
            .status()
            .unwrap();
        assert!(default_status.success());
        let status = Command::new("cargo")
            .args(["run", "--offline", "--manifest-path"])
            .arg(consumer.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("target"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "deep nested-Cargo matrix runs in main-push --no-budget CI"]
    fn generated_adapter_registers_and_dispatches_through_stub_transport() {
        use std::{
            fs,
            process::Command,
            sync::atomic::{AtomicUsize, Ordering},
        };
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "boxology-adapter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        for file in tree(CONTRACT, false).files() {
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
                "[workspace]\nmembers=[\"generated/contract\",\"implementation\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology={{path={:?}}}\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\nboxology-runtime={{version=\"=0.1.0\",path={:?},features=[\"test-support\"]}}\n",
                workspace.join("boxology"),
                workspace.join("boxology-contract"),
                workspace.join("boxology-runtime"),
            ),
        )
        .unwrap();
        let implementation = root.join("implementation");
        fs::create_dir_all(implementation.join("src")).unwrap();
        fs::write(
            implementation.join("Cargo.toml"),
            "[package]\nname=\"hello-implementation\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology={workspace=true}\nboxology-contract={workspace=true}\nboxology-runtime={workspace=true}\nboxology_generated_contract={package=\"hello-contract\",path=\"../generated/contract\"}\n",
        )
        .unwrap();
        fs::write(
            implementation.join("src/main.rs"),
            r#"
use std::future::Future;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Waker};
use boxology_contract::{
    CallContext, Caller, CancelToken, ContractType, ErasedCallError, ExposureLevel,
    TraceContext,
};
use boxology_runtime::{CompositionBuilder, test_support::StubTransport};
use boxology_generated_contract::GreetError;

pub struct HelloService;

#[boxology::implementation]
impl HelloService {
    pub async fn greet(&self, context: CallContext, name: String) -> Result<String, GreetError> {
        let _ = context;
        if name.is_empty() {
            Err(GreetError::EmptyName)
        } else {
            Ok(format!("Hello, {name}!"))
        }
    }
}

mod generated {
    include!("../../generated/adapter/adapter.rs");
}

fn context() -> CallContext {
    CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None)
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) {
            return output;
        }
    }
}

fn assert_bounds<T: Send + Sync + 'static>() {}
fn assert_send<T: Send>(value: T) -> T { value }

fn main() {
    assert_bounds::<generated::HelloAdapter<HelloService>>();
    let descriptor = generated::implementation_descriptor();
    assert!(std::ptr::eq(
        descriptor.contract(),
        boxology_generated_contract::contract_descriptor()
    ));
    assert!(descriptor.imports().is_empty());
    let capability = descriptor.contract().capabilities()[0].id().clone();
    let transport = Arc::new(StubTransport::new());
    let mut builder = CompositionBuilder::new();
    builder.add_box(descriptor, |imports| generated::factory(HelloService, imports));
    builder.expose(
        boxology_contract::BoxId::new("hello").unwrap(),
        capability.clone(),
        transport.clone(),
        ExposureLevel::External,
    );
    let composition = builder.start().unwrap();
    let runtime = transport.runtime().unwrap();
    let exposure = &runtime.exposures()[0];
    let input = "Ada".to_owned().encode().unwrap();
    let future = exposure.dispatch(context(), input);
    let output = block_on(assert_send(future)).unwrap();
    assert_eq!(String::decode(&output).unwrap(), "Hello, Ada!");

    let malformed = block_on(exposure.dispatch(context(), boxology_contract::SlotValue::Null));
    let Err(ErasedCallError::ContractViolation(detail)) = malformed else {
        panic!("malformed provider input was accepted")
    };
    assert_eq!(detail.code(), "input_decode");

    let domain = block_on(exposure.dispatch(context(), String::new().encode().unwrap()));
    assert!(matches!(domain, Err(ErasedCallError::Domain { error_tag, .. }) if error_tag == "EmptyName"));

    drop(composition);
}
"#,
        )
        .unwrap();
        let status = Command::new("cargo")
            .args(["run", "--offline", "--manifest-path"])
            .arg(implementation.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("target"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "deep nested-Cargo matrix runs in main-push --no-budget CI"]
    fn generated_import_adapter_typed_imports_compile_and_fail_unsealed() {
        // The greeter box imports hello; its adapter's typed import surface must type-check against
        // the real crates and behave. The box captures its `HelloImport` out of the `add_box`
        // factory closure (composition.rs `F: FnOnce(Imports) -> T` has no `'static` bound, so the
        // borrow-then-move compiles), then calls the typed `greet` on the still-unsealed handle. No
        // second box and no resolve_import (sealed e2e is PR-3): the call must fail closed as
        // Unavailable("unsealed_import"). This proves the emitted encode succeeds, the erased error
        // passes straight through `?`, and the `typed_imports` BoxId lookup agrees with the emitted
        // ImportDescriptor slot — else `typed_imports` would have panicked on the missing handle.
        use std::{
            fs,
            process::Command,
            sync::atomic::{AtomicUsize, Ordering},
        };
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "boxology-typed-imports-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let request = request_for_with_import(
            "greeter",
            CONTRACT,
            "hello",
            "imports/hello.json",
            &valid_hello_schema(),
        );
        for file in generate(request).unwrap().files() {
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
                "[workspace]\nmembers=[\"generated/contract\",\"implementation\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology={{path={:?}}}\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\nboxology-runtime={{version=\"=0.1.0\",path={:?}}}\n",
                workspace.join("boxology"),
                workspace.join("boxology-contract"),
                workspace.join("boxology-runtime"),
            ),
        )
        .unwrap();
        let implementation = root.join("implementation");
        fs::create_dir_all(implementation.join("src")).unwrap();
        fs::write(
            implementation.join("Cargo.toml"),
            "[package]\nname=\"greeter-implementation\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology={workspace=true}\nboxology-contract={workspace=true}\nboxology-runtime={workspace=true}\nboxology_generated_contract={package=\"greeter-contract\",path=\"../generated/contract\"}\n",
        )
        .unwrap();
        fs::write(
            implementation.join("src/main.rs"),
            r#"
use std::future::Future;
use std::task::{Context as TaskContext, Poll, Waker};
use boxology_contract::{CallContext, Caller, CancelToken, ErasedCallError, TraceContext};
use boxology_runtime::CompositionBuilder;
use boxology_generated_contract::GreetError;

pub struct GreeterService {
    hello: generated::HelloImport,
}

#[boxology::implementation]
impl GreeterService {
    pub async fn greet(&self, context: CallContext, name: String) -> Result<String, GreetError> {
        let _ = (&self.hello, context);
        Ok(format!("Hello, {name}!"))
    }
}

mod generated {
    include!("../../generated/adapter/adapter.rs");
}

fn context() -> CallContext {
    CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None)
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) {
            return output;
        }
    }
}

fn main() {
    let descriptor = generated::implementation_descriptor();
    assert_eq!(descriptor.imports().len(), 1);
    let mut builder = CompositionBuilder::new();
    let mut captured = None;
    builder.add_box(descriptor, |imports| {
        let deps = generated::typed_imports(&imports);
        captured = Some(generated::typed_imports(&imports).hello);
        generated::factory(GreeterService { hello: deps.hello }, imports)
    });
    let result = block_on(captured.unwrap().greet(context(), "Ada".into()));
    let Err(ErasedCallError::Unavailable(detail)) = result else {
        panic!("unsealed import did not fail as unavailable: {result:?}")
    };
    assert_eq!(detail.code(), "unsealed_import");
}
"#,
        )
        .unwrap();
        let status = Command::new("cargo")
            .args(["run", "--offline", "--manifest-path"])
            .arg(implementation.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("target"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn structured_import_routes_through_provider_owned_alias_end_to_end() {
        // A greeter calls a provider-owned nested struct + unit enum + Option + Vec through its
        // sealed generated import. The app's explicit `boxology_import_hello` dependency is the
        // only owner-facing path used by the consumer adapter; no provider declarations are copied.
        // Two generated adapters still use `::boxology_generated_contract` for their own contract,
        // so each is aliased in its own crate: a `hello-impl` lib box and an `app` bin box. The app
        // adds both boxes, asserts `validate()` reports exactly the unresolved-import assembly error
        // (folded-in negative test), resolves greeter->hello, exposes greeter.greet_loudly on a
        // StubTransport, `start()`s (sealing the import), dispatches "Ada", and asserts "HELLO, ADA!"
        // — which exists only if the sealed import actually routed to hello ("Hello, " prefix) and
        // greeter uppercased it. Greeter calls `self.hello.greet(context.child(), name)`; hello's impl
        // asserts the inherited traceparent+deadline crossed the sealed boundary.
        use std::{
            fs,
            process::Command,
            sync::atomic::{AtomicUsize, Ordering},
        };
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "boxology-sealed-e2e-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let hello_source = r#"boxology::contract! {
            pub enum Tone { Calm, Loud }
            pub struct GreetRequest {
                pub name: String, pub tones: Vec<Tone>, pub nickname: Option<String>,
            }
            pub struct GreetOutcome {
                pub messages: Vec<String>, pub selected: Option<Tone>,
            }
            #[error] pub enum GreetError { EmptyName }
            #[capability(exposure=external)]
            pub async fn greet(request:GreetRequest)->Result<GreetOutcome,GreetError>;
        }"#;
        let hello_tree = generate(request_for("hello", hello_source)).unwrap();
        let hello_schema = std::str::from_utf8(file(&hello_tree, "generated/schema.json").bytes())
            .unwrap()
            .to_owned();
        for file in hello_tree.files() {
            let path = root.join("hello").join(file.path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file.bytes()).unwrap();
        }
        let greeter_request = request_for_with_import(
            "greeter",
            GREETER,
            "hello",
            "imports/hello.json",
            &hello_schema,
        );
        for file in generate(greeter_request).unwrap().files() {
            let path = root.join("greeter").join(file.path());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file.bytes()).unwrap();
        }
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[workspace]\nmembers=[\"hello/generated/contract\",\"greeter/generated/contract\",\"hello-impl\",\"app\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology={{path={:?}}}\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\nboxology-runtime={{version=\"=0.1.0\",path={:?},features=[\"test-support\"]}}\n",
                workspace.join("boxology"),
                workspace.join("boxology-contract"),
                workspace.join("boxology-runtime"),
            ),
        )
        .unwrap();
        let hello_impl = root.join("hello-impl");
        fs::create_dir_all(hello_impl.join("src")).unwrap();
        fs::write(
            hello_impl.join("Cargo.toml"),
            "[package]\nname=\"hello-impl\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology={workspace=true}\nboxology-contract={workspace=true}\nboxology-runtime={workspace=true}\nboxology_generated_contract={package=\"hello-contract\",path=\"../hello/generated/contract\"}\n",
        )
        .unwrap();
        fs::write(
            hello_impl.join("src/lib.rs"),
            r#"
use boxology_contract::CallContext;
use boxology_generated_contract::{GreetError, GreetOutcome, GreetRequest};

pub struct HelloService;

#[boxology::implementation]
impl HelloService {
    pub async fn greet(
        &self,
        context: CallContext,
        request: GreetRequest,
    ) -> Result<GreetOutcome, GreetError> {
        // The sealed import must carry the parent's inherited trace + absolute deadline across the
        // boundary; `context.child()` in the greeter derives them, so both are present here.
        assert_eq!(context.trace().traceparent(), Some("e2e-parent"));
        assert!(context.deadline().is_some());
        let who = request.nickname.unwrap_or(request.name);
        Ok(GreetOutcome {
            messages: vec![format!("Hello, {who}!")],
            selected: request.tones.into_iter().next(),
        })
    }
}

pub mod generated {
    include!("../../hello/generated/adapter/adapter.rs");
}
"#,
        )
        .unwrap();
        let app = root.join("app");
        fs::create_dir_all(app.join("src")).unwrap();
        fs::write(
            app.join("Cargo.toml"),
            "[package]\nname=\"app\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology={workspace=true}\nboxology-contract={workspace=true}\nboxology-runtime={workspace=true}\nboxology_generated_contract={package=\"greeter-contract\",path=\"../greeter/generated/contract\"}\nboxology_import_hello={package=\"hello-contract\",path=\"../hello/generated/contract\"}\nhello-impl={path=\"../hello-impl\"}\n",
        )
        .unwrap();
        fs::write(
            app.join("src/main.rs"),
            r#"
use std::future::Future;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Waker};
use std::time::{Duration, Instant};
use boxology_contract::{
    CallContext, Caller, CancelToken, ContractType, Deadline, ExposureLevel, TraceContext,
};
use boxology_runtime::{
    AssemblyError, CompositionBuilder, ImportTarget, test_support::StubTransport,
};
use boxology_generated_contract::GreetLoudlyError;

pub struct GreeterService {
    hello: generated::HelloImport,
}

#[boxology::implementation]
impl GreeterService {
    pub async fn greet_loudly(
        &self,
        context: CallContext,
        name: String,
    ) -> Result<String, GreetLoudlyError> {
        let outcome = self
            .hello
            .greet(
                context.child(),
                boxology_import_hello::GreetRequest {
                    name,
                    tones: vec![boxology_import_hello::Tone::Loud],
                    nickname: Some("Ada".into()),
                },
            )
            .await
            .map_err(|_| GreetLoudlyError::Refused)?;
        assert!(matches!(
            outcome.selected,
            Some(boxology_import_hello::Tone::Loud)
        ));
        let greeting = outcome.messages.into_iter().next().unwrap();
        Ok(greeting.to_uppercase())
    }
}

mod generated {
    include!("../../greeter/generated/adapter/adapter.rs");
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) {
            return output;
        }
    }
}

fn assert_send<T: Send>(value: T) -> T {
    value
}

fn dispatch_context() -> CallContext {
    CallContext::new(
        Caller::Anonymous,
        Some(Deadline::at(Instant::now() + Duration::from_secs(30))),
        CancelToken::new(),
        TraceContext::new(Some("e2e-parent".into()), None),
        None,
    )
}

fn main() {
    let hello_descriptor = hello_impl::generated::implementation_descriptor();
    let greeter_descriptor = generated::implementation_descriptor();
    assert!(hello_descriptor.imports().is_empty());
    assert_eq!(greeter_descriptor.imports().len(), 1);
    let greet_loudly_cap = greeter_descriptor.contract().capabilities()[0].id().clone();
    assert_eq!(greet_loudly_cap.to_string(), "greeter.greet_loudly");

    let greeter = boxology_contract::BoxId::new("greeter").unwrap();
    let hello = boxology_contract::BoxId::new("hello").unwrap();
    let transport = Arc::new(StubTransport::new());
    let mut builder = CompositionBuilder::new();
    builder.add_box(hello_descriptor, |imports| {
        hello_impl::generated::factory(hello_impl::HelloService, imports)
    });
    builder.add_box(greeter_descriptor, |imports| {
        let deps = generated::typed_imports(&imports);
        generated::factory(GreeterService { hello: deps.hello }, imports)
    });

    // Folded-in negative test: the declared import is still unresolved, so validate reports exactly
    // the missing-import-resolution assembly error and nothing else.
    let error = builder.validate().unwrap_err();
    assert_eq!(
        error.errors(),
        &[AssemblyError::MissingImportResolution {
            consumer: greeter.clone(),
            slot: hello.clone(),
        }]
    );

    builder.resolve_import(greeter.clone(), hello.clone(), ImportTarget::local(hello.clone()));
    builder.expose(greeter.clone(), greet_loudly_cap, transport.clone(), ExposureLevel::External);
    let composition = builder.start().unwrap();

    let runtime = transport.runtime().unwrap();
    let exposure = runtime
        .exposures()
        .iter()
        .find(|exposure| exposure.descriptor().id().to_string() == "greeter.greet_loudly")
        .expect("greeter.greet_loudly exposure missing");
    let input = "Ada".to_owned().encode().unwrap();
    let output = block_on(assert_send(exposure.dispatch(dispatch_context(), input))).unwrap();
    assert_eq!(String::decode(&output).unwrap(), "HELLO, ADA!");

    drop(composition);
}
"#,
        )
        .unwrap();
        let status = Command::new("cargo")
            .args(["run", "--offline", "--manifest-path"])
            .arg(app.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("target"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "deep nested-Cargo matrix runs in main-push --no-budget CI"]
    fn generated_multi_capability_adapter_and_implementation_compile_and_route() {
        use std::{
            fs,
            process::Command,
            sync::atomic::{AtomicUsize, Ordering},
        };
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let source = "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }";
        let root = std::env::temp_dir().join(format!(
            "boxology-multicap-adapter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        for file in generate(request_for("store", source)).unwrap().files() {
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
                "[workspace]\nmembers=[\"generated/contract\",\"implementation\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology={{path={:?}}}\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\nboxology-runtime={{version=\"=0.1.0\",path={:?},features=[\"test-support\"]}}\n",
                workspace.join("boxology"),
                workspace.join("boxology-contract"),
                workspace.join("boxology-runtime"),
            ),
        )
        .unwrap();
        let implementation = root.join("implementation");
        fs::create_dir_all(implementation.join("src")).unwrap();
        fs::write(
            implementation.join("Cargo.toml"),
            "[package]\nname=\"store-implementation\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology={workspace=true}\nboxology-contract={workspace=true}\nboxology-runtime={workspace=true}\nboxology_generated_contract={package=\"store-contract\",path=\"../generated/contract\"}\n",
        )
        .unwrap();
        fs::write(
            implementation.join("src/main.rs"),
            r#"
use std::future::Future;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Waker};
use boxology_contract::{CallContext, Caller, CancelToken, ContractType, ExposureLevel, TraceContext};
use boxology_runtime::{CompositionBuilder, test_support::StubTransport};
use boxology_generated_contract::StoreError;

pub struct StoreService;

#[boxology::implementation]
impl StoreService {
    pub async fn get(&self, context: CallContext, key: u64) -> Result<String, StoreError> {
        let _ = context;
        Ok(format!("value-{key}"))
    }

    pub async fn put(&self, context: CallContext, value: String) -> Result<bool, StoreError> {
        let _ = context;
        Ok(!value.is_empty())
    }
}

mod generated {
    include!("../../generated/adapter/adapter.rs");
}

fn context() -> CallContext {
    CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None)
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) {
            return output;
        }
    }
}

fn assert_bounds<T: Send + Sync + 'static>() {}
fn assert_send<T: Send>(value: T) -> T { value }

fn main() {
    assert_bounds::<generated::StoreAdapter<StoreService>>();
    let descriptor = generated::implementation_descriptor();
    assert!(std::ptr::eq(
        descriptor.contract(),
        boxology_generated_contract::contract_descriptor()
    ));
    assert!(descriptor.imports().is_empty());
    let capabilities = descriptor.contract().capabilities();
    assert_eq!(capabilities.len(), 2);
    let get_cap = capabilities[0].id().clone();
    let put_cap = capabilities[1].id().clone();
    assert_eq!(get_cap.to_string(), "store.get");
    assert_eq!(put_cap.to_string(), "store.put");

    let transport = Arc::new(StubTransport::new());
    let provider = boxology_contract::BoxId::new("store").unwrap();
    let mut builder = CompositionBuilder::new();
    builder.add_box(descriptor, |imports| generated::factory(StoreService, imports));
    builder.expose(provider.clone(), get_cap.clone(), transport.clone(), ExposureLevel::External);
    builder.expose(provider.clone(), put_cap.clone(), transport.clone(), ExposureLevel::External);
    let composition = builder.start().unwrap();

    let runtime = transport.runtime().unwrap();
    let exposures = runtime.exposures();
    assert_eq!(exposures.len(), 2);
    let get_exposure = exposures
        .iter()
        .find(|exposure| exposure.descriptor().id().to_string() == "store.get")
        .expect("store.get exposure missing");
    let put_exposure = exposures
        .iter()
        .find(|exposure| exposure.descriptor().id().to_string() == "store.put")
        .expect("store.put exposure missing");

    let get_input = 7u64.encode().unwrap();
    let get_output = block_on(assert_send(get_exposure.dispatch(context(), get_input))).unwrap();
    assert_eq!(String::decode(&get_output).unwrap(), "value-7");

    let put_input = "x".to_owned().encode().unwrap();
    let put_output = block_on(assert_send(put_exposure.dispatch(context(), put_input))).unwrap();
    assert!(bool::decode(&put_output).unwrap());

    drop(composition);
}
"#,
        )
        .unwrap();
        let status = Command::new("cargo")
            .args(["run", "--offline", "--manifest-path"])
            .arg(implementation.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("target"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn output_set_fails_closed_with_one_stable_code() {
        let cases = [
            (
                vec![OUTPUTS[0], OUTPUTS[1]],
                r#"BXG0039 <request>:1:1-1:1 offending="declared outputs [\"generated/contract/Cargo.toml\", \"generated/contract/src/lib.rs\"]" rule="declared outputs must equal the generator's complete output set without duplicates" source="specs/s2-contract-generator.md D1""#,
            ),
            (
                vec![OUTPUTS[0], OUTPUTS[1], OUTPUTS[3], "generated/extra"],
                r#"BXG0039 <request>:1:1-1:1 offending="declared outputs [\"generated/contract/Cargo.toml\", \"generated/contract/src/lib.rs\", \"generated/extra\", \"generated/schema.json\"]" rule="declared outputs must equal the generator's complete output set without duplicates" source="specs/s2-contract-generator.md D1""#,
            ),
            (
                vec![OUTPUTS[0], OUTPUTS[1], OUTPUTS[3], OUTPUTS[3]],
                r#"BXG0039 <request>:1:1-1:1 offending="declared outputs [\"generated/contract/Cargo.toml\", \"generated/contract/src/lib.rs\", \"generated/schema.json\", \"generated/schema.json\"]" rule="declared outputs must equal the generator's complete output set without duplicates" source="specs/s2-contract-generator.md D1""#,
            ),
        ];
        for (outputs, expected) in cases {
            let diagnostics = generate(request(CONTRACT, false, outputs)).unwrap_err();
            assert_eq!(diagnostics.as_slice().len(), 1);
            assert_eq!(diagnostics.as_slice()[0].code(), "BXG0039");
            assert_eq!(diagnostics.to_string(), expected);
        }
    }

    #[test]
    fn generator_inventory_duplicates_fail_closed() {
        let request = request(CONTRACT, false, OUTPUTS.to_vec());
        let diagnostics = request
            .require_exact_outputs(&[OUTPUTS[0], OUTPUTS[1], OUTPUTS[3], OUTPUTS[3]])
            .unwrap_err();
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0039");
    }

    #[test]
    fn scalar_boundary_emits_and_only_blob_fails_closed() {
        for source in [
            CONTRACT.replace("name:String", "name:u32"),
            CONTRACT.replace("Result<String", "Result<bool"),
        ] {
            generate(request(&source, false, OUTPUTS.to_vec()))
                .expect("scalar boundary leaves now generate end-to-end");
        }
        for source in [
            CONTRACT.replace("name:String", "name:Blob"),
            CONTRACT.replace("Result<String", "Result<Blob"),
        ] {
            let request = request(&source, false, OUTPUTS.to_vec());
            let expected_span = ParsedRustInputs::parse(&request)
                .and_then(|parsed| parsed.controlled_contract())
                .unwrap()
                .span();
            let diagnostics = generate(request).unwrap_err();
            assert_eq!(diagnostics.as_slice().len(), 1);
            assert_eq!(diagnostics.as_slice()[0].code(), "BXG0040");
            assert_eq!(diagnostics.as_slice()[0].span(), expected_span);
        }
        // The guard loops over EVERY capability: a two-capability box whose SECOND capability
        // carries a Blob boundary leaf still fails closed with BXG0040, not just the first.
        let second_cap_blob = "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:Blob)->Result<bool,StoreError>; }";
        let request = request_for("store", second_cap_blob);
        let expected_span = ParsedRustInputs::parse(&request)
            .and_then(|parsed| parsed.controlled_contract())
            .unwrap()
            .span();
        let diagnostics = generate(request).unwrap_err();
        assert_eq!(diagnostics.as_slice().len(), 1);
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0040");
        assert_eq!(diagnostics.as_slice()[0].span(), expected_span);
    }

    #[test]
    fn multi_capability_box_generates() {
        // The BXG0041 single-capability guard is lifted: the same two-capability store box that
        // used to fail closed now generates end-to-end through generate() with the full output set.
        let source = "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }";
        let tree = generate(request_for("store", source))
            .expect("a two-capability box generates now that the guard is lifted");
        for path in OUTPUTS {
            assert!(
                tree.files().iter().any(|file| file.path() == path),
                "generated tree missing {path}"
            );
        }
    }

    #[test]
    #[ignore = "deep nested-Cargo matrix runs in main-push --no-budget CI"]
    fn generated_numeric_boundary_compiles_and_routes_typed_values() {
        use std::{fs, process::Command};
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let source = "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(count:u32)->Result<bool,GreetError>; }";
        let root = std::env::temp_dir().join(format!(
            "boxology-numeric-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        for file in tree(source, false).files() {
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
                "[workspace]\nmembers=[\"generated/contract\",\"consumer\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\n",
                workspace.join("boxology-contract")
            ),
        )
        .unwrap();
        let consumer = root.join("consumer");
        fs::create_dir_all(consumer.join("src")).unwrap();
        fs::write(
            consumer.join("Cargo.toml"),
            "[package]\nname=\"consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology-contract={workspace=true}\nhello-contract={package=\"hello-contract\",path=\"../generated/contract\",features=[\"test-support\"]}\n",
        )
        .unwrap();
        fs::write(
            consumer.join("src/main.rs"),
            r#"
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Waker};
use boxology_contract::{CallContext, Caller, CancelToken, CapabilityId, ContractValue, DescriptorRef, ErasedCallError, ErasedCallTarget, SlotValue, TraceContext};
use hello_contract::{test_support::HelloFake, contract_descriptor, HelloHandle};

struct Echo;
impl ErasedCallTarget for Echo {
    fn call<'a>(&'a self, capability: &'a CapabilityId, _context: CallContext, input: SlotValue) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        assert_eq!(capability.to_string(), "hello.greet");
        assert_eq!(input, SlotValue::Value(ContractValue::u64(41)));
        Box::pin(ready(Ok(SlotValue::Value(ContractValue::bool(true)))))
    }
}

fn context() -> CallContext {
    CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None)
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop { if let Poll::Ready(output) = future.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) { return output; } }
}

fn main() {
    let capability = &contract_descriptor().capabilities()[0];
    assert!(matches!(capability.input().view(), DescriptorRef::U32));
    assert!(matches!(capability.output().view(), DescriptorRef::Bool));

    let handle = HelloHandle::from_erased(Arc::new(Echo));
    assert_eq!(block_on(handle.greet(context(), 41u32)), Ok(true));

    let fake = HelloFake::new().with_greet(|_, count: u32| async move { Ok(count == 41) });
    assert_eq!(block_on(fake.handle().greet(context(), 41u32)), Ok(true));
}
"#,
        )
        .unwrap();
        let status = Command::new("cargo")
            .args(["run", "--offline", "--manifest-path"])
            .arg(consumer.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("target"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generated_multi_capability_box_compiles_and_routes_both_capabilities() {
        use std::{fs, process::Command};
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let source = "boxology::contract! { #[error] pub enum StoreError { Missing } #[capability(exposure=external)] pub async fn get(key:u64)->Result<String,StoreError>; #[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>; }";
        let root = std::env::temp_dir().join(format!(
            "boxology-multi-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        for file in generate(request_for("store", source)).unwrap().files() {
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
                "[workspace]\nmembers=[\"generated/contract\",\"consumer\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology-contract={{version=\"=0.1.0\",path={:?}}}\n",
                workspace.join("boxology-contract")
            ),
        )
        .unwrap();
        let consumer = root.join("consumer");
        fs::create_dir_all(consumer.join("src")).unwrap();
        fs::write(
            consumer.join("Cargo.toml"),
            "[package]\nname=\"consumer\"\nversion=\"0.0.0\"\nedition=\"2024\"\n\n[dependencies]\nboxology-contract={workspace=true}\nstore-contract={package=\"store-contract\",path=\"../generated/contract\",features=[\"test-support\"]}\n",
        )
        .unwrap();
        fs::write(
            consumer.join("src/main.rs"),
            r#"
use std::future::{ready, Future};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll, Waker};
use boxology_contract::{CallContext, CallError, Caller, CancelToken, CapabilityId, ContractValue, ErasedCallError, ErasedCallTarget, SlotValue, TraceContext};
use store_contract::{test_support::StoreFake, contract_descriptor, StoreError, StoreHandle};

struct Stub;
impl ErasedCallTarget for Stub {
    fn call<'a>(&'a self, capability: &'a CapabilityId, _context: CallContext, input: SlotValue) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>> {
        let output = match capability.to_string().as_str() {
            "store.get" => {
                assert_eq!(input, SlotValue::Value(ContractValue::u64(7)));
                SlotValue::Value(ContractValue::string("seven"))
            }
            "store.put" => {
                assert_eq!(input, SlotValue::Value(ContractValue::string("seven")));
                SlotValue::Value(ContractValue::bool(true))
            }
            other => panic!("unexpected capability {other}"),
        };
        Box::pin(ready(Ok(output)))
    }
}

fn context() -> CallContext {
    CallContext::new(Caller::Anonymous, None, CancelToken::new(), TraceContext::empty(), None)
}

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = Box::pin(future);
    loop { if let Poll::Ready(output) = future.as_mut().poll(&mut TaskContext::from_waker(Waker::noop())) { return output; } }
}

fn main() {
    let capabilities = contract_descriptor().capabilities();
    assert_eq!(capabilities.len(), 2);
    assert_eq!(capabilities[0].id().to_string(), "store.get");
    assert_eq!(capabilities[1].id().to_string(), "store.put");

    let handle = StoreHandle::from_erased(Arc::new(Stub));
    assert_eq!(block_on(handle.get(context(), 7u64)), Ok("seven".into()));
    assert_eq!(block_on(handle.put(context(), "seven".into())), Ok(true));

    let fake = StoreFake::new()
        .with_get(|_, key: u64| async move { assert_eq!(key, 7); Ok("seven".to_string()) })
        .with_put(|_, value: String| async move { assert_eq!(value, "seven"); Ok(true) });
    assert_eq!(block_on(fake.handle().get(context(), 7u64)), Ok("seven".into()));
    assert_eq!(block_on(fake.handle().put(context(), "seven".into())), Ok(true));

    let put_only = StoreFake::new().with_put(|_, _| async { Ok(true) });
    let Err(CallError::Internal(detail)) = block_on(put_only.handle().get(context(), 7u64)) else {
        panic!("unprogrammed get did not fail closed")
    };
    assert_eq!(detail.code(), "unprogrammed_capability");

    let domain = StoreFake::new().with_get(|_, _| async { Err(StoreError::Missing) });
    assert_eq!(block_on(domain.handle().get(context(), 7u64)), Err(CallError::Domain(StoreError::Missing)));
}
"#,
        )
        .unwrap();
        let contract_status = Command::new("cargo")
            .args(["check", "--offline", "--manifest-path"])
            .arg(root.join("generated/contract/Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("contract-target"))
            .status()
            .unwrap();
        assert!(contract_status.success());
        let status = Command::new("cargo")
            .args(["run", "--offline", "--manifest-path"])
            .arg(consumer.join("Cargo.toml"))
            .env("CARGO_TARGET_DIR", root.join("target"))
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(root).unwrap();
    }
}
