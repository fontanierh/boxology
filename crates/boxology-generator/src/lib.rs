//! Pure generation of deterministic Boxology artifacts from validated logical inputs.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract_syntax::CanonicalType;
use boxology_generator_model::{Diagnostics, GenerationRequest, ParsedRustInputs};

mod schema;

const OUTPUTS: [&str; 4] = [
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
pub fn generate(request: &GenerationRequest) -> Result<GeneratedTree, Diagnostics> {
    request.require_exact_outputs(&OUTPUTS)?;
    let parsed = ParsedRustInputs::parse(request)?;
    let contract = parsed.controlled_contract()?;
    contract.require_v0_emittable()?;
    // `require_v0_emittable` fails closed unless there is exactly one capability, so binding the
    // sole capability once here makes the single-capability emission invariant explicit.
    let capability = &contract.model().capabilities[0];
    let revision = schema::revision(request.box_id().as_str(), contract.model());
    let manifest = format!(
        "[package]\nname = \"{}-contract\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[features]\ndefault = []\ntest-support = []\n\n[dependencies]\nboxology-contract = {{ workspace = true }}\n",
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
                    fn require_future<F: ::core::future::Future<Output = ::core::result::Result<__OUTPUT_TY__, $crate::__ERROR__>> + ::core::marker::Send>(_: F) {}
                    fn check(receiver: &$receiver, context: ::boxology::CallContext, input: __INPUT_TY__) {
                        require_service::<$receiver>();
                        require_future(receiver.__CAPABILITY__(context, input));
                    }
                };
                impl $crate::HelloDispatch for $receiver {
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
        rust_value_type(capability.input_type, true),
    )
    .replace(
        "__OUTPUT_TY__",
        rust_value_type(capability.output_type, true),
    );
    let descriptor =
        schema::descriptor_source(request.box_id().as_str(), contract.model(), &revision);
    let dispatch = dispatch_source(
        request.box_id().as_str(),
        &capability.name,
        &capability.input_name,
        &contract.model().error.name,
        capability.input_type,
        capability.output_type,
        contract
            .model()
            .error
            .variants
            .iter()
            .map(|variant| (variant.name.as_str(), variant.deprecation.as_deref())),
    );
    let test_support =
        test_support_source(&error.name, capability.input_type, capability.output_type);
    let adapter = adapter_source(&capability.name, capability.input_type);
    let syntax = syn::parse_file(&format!(
        "{descriptor} {dispatch} {error_attrs}#[derive(Debug, Clone, PartialEq)] pub enum {} {{{variants} Unknown {{ tag: ::std::string::String, payload: ::boxology_contract::OpaquePayload }}}} {error_abi} {test_support} #[doc(hidden)] pub const __BOXOLOGY_SEMANTIC_DIGEST: [u8; 32] = [{digest}]; {checker}",
        error.name
    ))
    .expect("validated names and fixed generator template must parse");
    let rust = format!(
        "// Generated by boxology-generator {}\n{}",
        env!("CARGO_PKG_VERSION"),
        prettyplease::unparse(&syntax)
    );
    let adapter_syntax =
        syn::parse_file(&adapter).expect("validated names and fixed adapter template must parse");
    let adapter_rust = format!(
        "// Generated by boxology-generator {}\n{}",
        env!("CARGO_PKG_VERSION"),
        prettyplease::unparse(&adapter_syntax)
    );
    let schema = schema::document(
        request.box_id().as_str(),
        contract.model(),
        &revision,
        contract.semantic_digest(),
        env!("CARGO_PKG_VERSION"),
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
    files.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    Ok(GeneratedTree(files))
}

fn test_support_source(
    error_name: &str,
    input_type: CanonicalType,
    output_type: CanonicalType,
) -> String {
    let input_bare = rust_value_type(input_type, false);
    let output_bare = rust_value_type(output_type, false);
    let input_constructor = schema::descriptor_constructor(input_type);
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

            use super::{{{error_name}, HELLO_GREET, HelloHandle, conversion_detail}};

            type GreetFuture =
                Pin<Box<dyn Future<Output = Result<{output_bare}, {error_name}>> + Send + 'static>>;
            type GreetResponder =
                dyn Fn(CallContext, {input_bare}) -> GreetFuture + Send + Sync + 'static;

            #[derive(Clone, Default)]
            pub struct HelloFake {{
                greet: Option<Arc<GreetResponder>>,
            }}

            impl HelloFake {{
                pub fn new() -> Self {{
                    Self::default()
                }}

                pub fn with_greet<F, Fut>(mut self, responder: F) -> Self
                where
                    F: Fn(CallContext, {input_bare}) -> Fut + Send + Sync + 'static,
                    Fut: Future<Output = Result<{output_bare}, {error_name}>> + Send + 'static,
                {{
                    self.greet = Some(Arc::new(move |context, name| {{
                        Box::pin(responder(context, name))
                    }}));
                    self
                }}

                pub fn handle(&self) -> HelloHandle {{
                    HelloHandle::from_erased(Arc::new(self.clone()))
                }}
            }}

            impl ErasedCallTarget for HelloFake {{
                fn call<'a>(
                    &'a self,
                    capability: &'a CapabilityId,
                    context: CallContext,
                    input: SlotValue,
                ) -> Pin<Box<dyn Future<Output = Result<SlotValue, ErasedCallError>> + Send + 'a>>
                {{
                    if capability != &*HELLO_GREET {{
                        return Box::pin(ready(Err(unprogrammed())));
                    }}
                    let Some(responder) = self.greet.clone() else {{
                        return Box::pin(ready(Err(unprogrammed())));
                    }};
                    Box::pin(async move {{
                        let input = TypeDescriptor::{input_constructor}()
                            .conform(DecodeRole::ProviderInput, input)
                            .map_err(|error| {{
                                ErasedCallError::ContractViolation(conversion_detail(
                                    "input_decode",
                                    error,
                                ))
                            }})?;
                        let name = {input_bare}::decode(&input).map_err(|error| {{
                            ErasedCallError::ContractViolation(conversion_detail(
                                "input_decode",
                                error,
                            ))
                        }})?;
                        match responder(context, name).await {{
                            Ok(output) => output.encode().map_err(|error| {{
                                ErasedCallError::InvalidResponse(conversion_detail(
                                    "output_encode",
                                    error,
                                ))
                            }}),
                            Err(error) => Err(ErasedCallError::from_domain(&error)),
                        }}
                    }})
                }}
            }}

            fn unprogrammed() -> ErasedCallError {{
                ErasedCallError::Internal(Detail::new("unprogrammed_capability"))
            }}
        }}
        "#,
        error_name = error_name,
        input_bare = input_bare,
        output_bare = output_bare,
        input_constructor = input_constructor,
    )
}

fn adapter_source(capability_name: &str, input_type: CanonicalType) -> String {
    let input_constructor = schema::descriptor_constructor(input_type);
    let input_qualified = rust_value_type(input_type, true);
    format!(
        r#"
        use ::boxology_contract::ContractType;

        #[doc(hidden)]
        pub fn implementation_descriptor() -> ::boxology_contract::ImplementationDescriptor {{
            ::boxology_contract::ImplementationDescriptor::new(
                ::boxology_generated_contract::contract_descriptor(),
                [],
            )
            .expect("generated adapter import descriptors are valid")
        }}

        #[doc(hidden)]
        pub struct HelloAdapter<T> {{
            service: T,
            _imports: ::boxology_runtime::Imports,
        }}

        #[doc(hidden)]
        pub fn factory<T>(
            service: T,
            imports: ::boxology_runtime::Imports,
        ) -> HelloAdapter<T>
        where
            T: ::boxology_generated_contract::HelloDispatch + Send + Sync + 'static,
        {{
            HelloAdapter {{
                service,
                _imports: imports,
            }}
        }}

        impl<T> ::boxology_contract::ErasedTarget for HelloAdapter<T>
        where
            T: ::boxology_generated_contract::HelloDispatch + Send + Sync + 'static,
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
                let expected = ::boxology_generated_contract::contract_descriptor()
                    .capabilities()
                    .first()
                    .expect("generated Hello contract has one capability")
                    .id();
                if capability != expected {{
                    return Box::pin(::std::future::ready(Err(unknown_capability())));
                }}
                Box::pin(async move {{
                    let input = ::boxology_contract::TypeDescriptor::{input_constructor}()
                        .conform(
                            ::boxology_contract::DecodeRole::ProviderInput,
                            input,
                        )
                        .map_err(|error| {{
                            ::boxology_contract::ErasedCallError::ContractViolation(
                                conversion_detail("input_decode", error),
                            )
                        }})?;
                    let input = {input_qualified}::decode(&input).map_err(|error| {{
                        ::boxology_contract::ErasedCallError::ContractViolation(
                            conversion_detail("input_decode", error),
                        )
                    }})?;
                    match ::boxology_generated_contract::HelloDispatch::{capability_name}(
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
                    }}
                }})
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
        capability_name = capability_name,
        input_constructor = input_constructor,
        input_qualified = input_qualified,
    )
}

fn dispatch_source<'a>(
    box_id: &str,
    capability_name: &str,
    input_name: &str,
    error_name: &str,
    input_type: CanonicalType,
    output_type: CanonicalType,
    variants: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> String {
    let input_bare = rust_value_type(input_type, false);
    let output_bare = rust_value_type(output_type, false);
    let output_constructor = schema::descriptor_constructor(output_type);
    let variants = variants
        .into_iter()
        .map(|(name, deprecation)| {
            format!(
                "::boxology_contract::VariantDescriptor::new({name:?}, ::boxology_contract::VariantPayload::Unit, {}),",
                rust_deprecation(deprecation),
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

        pub trait HelloDispatch: Send + Sync + 'static {{
            fn {capability_name}<'a>(
                &'a self,
                context: CallContext,
                {input_name}: {input_bare},
            ) -> Pin<Box<dyn Future<Output = Result<{output_bare}, {error_name}>> + Send + 'a>>;
        }}

        #[derive(Clone)]
        pub struct HelloHandle {{
            target: Arc<dyn ErasedCallTarget>,
        }}

        impl HelloHandle {{
            #[doc(hidden)]
            pub fn from_erased(target: Arc<dyn ErasedCallTarget>) -> Self {{
                Self {{ target }}
            }}

            pub async fn {capability_name}(
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
                    .call(&HELLO_GREET, context, input)
                    .await
                    .map_err(|error| error.into_typed::<{error_name}>(&GREET_ERROR_DESCRIPTOR))?;
                let output = TypeDescriptor::{output_constructor}()
                    .conform(DecodeRole::ConsumerOutput, output)
                    .map_err(|error| conversion_detail("output_decode", error))
                    .map_err(CallError::InvalidResponse)?;
                {output_bare}::decode(&output)
                    .map_err(|error| conversion_detail("output_decode", error))
                    .map_err(CallError::InvalidResponse)
            }}
        }}

        static HELLO_GREET: LazyLock<CapabilityId> = LazyLock::new(|| {{
            CapabilityId::new(
                BoxId::new({box_id:?}).expect("generated box identity is valid"),
                CapabilityName::new({capability_name:?})
                    .expect("generated capability name is valid"),
            )
        }});

        static GREET_ERROR_DESCRIPTOR: LazyLock<TypeDescriptor> = LazyLock::new(|| {{
            TypeDescriptor::enumeration([
                {variants}
            ])
            .expect("generated greet error descriptor is valid")
        }});

        fn conversion_detail(code: &'static str, error: impl std::fmt::Display) -> Detail {{
            Detail::new(code).with_message(error.to_string())
        }}
        "#,
        box_id = box_id,
        capability_name = capability_name,
        input_name = input_name,
        error_name = error_name,
        input_bare = input_bare,
        output_bare = output_bare,
        output_constructor = output_constructor,
        variants = variants,
    )
}

/// Spells a canonical boundary leaf as a Rust value type for a runtime template site.
///
/// Every scalar leaf spells identically bare and qualified (`u32` -> `u32`); only `String` differs
/// (`String` bare, `::std::string::String` qualified). `Blob` never reaches emission because
/// `require_v0_emittable` fails it closed, but a spelling is provided for completeness.
fn rust_value_type(leaf: CanonicalType, qualified: bool) -> &'static str {
    match leaf {
        CanonicalType::String if qualified => "::std::string::String",
        CanonicalType::String => "String",
        CanonicalType::Blob if qualified => "::boxology_contract::Blob",
        CanonicalType::Blob => "Blob",
        scalar => scalar.canonical_name(),
    }
}

fn rust_deprecation(note: Option<&str>) -> String {
    match note {
        None => "None".into(),
        Some("") => "Some(::boxology_contract::Deprecation::new(None))".into(),
        Some(note) => format!(
            "Some(::boxology_contract::Deprecation::new(Some({note:?}.into())))",
            note = note,
        ),
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
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};

    const MANIFEST: &[u8] = b"schema = 1\nid = \"hello\"\nkind = \"box\"\n";
    const CONTRACT: &str = "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; }";
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

    fn tree(source: &str, reverse: bool) -> GeneratedTree {
        generate(&request(source, reverse, OUTPUTS.to_vec())).unwrap()
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
        const SCHEMA: &[u8] = br#"{
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
  "revision": "sha256:29c955e4594137d11300bd0894da461c2a9a9ce9866c4fd9a3f4b5d89cb04176",
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
        let generated = generate(&cold).unwrap();
        assert_eq!(file(&generated, "generated/schema.json").bytes(), SCHEMA);
        let value: Value = serde_json::from_slice(SCHEMA).unwrap();
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
        assert_ne!(changed_provenance, SCHEMA);
        assert_eq!(
            serde_json::from_slice::<Value>(&changed_provenance).unwrap()["revision"],
            value["revision"]
        );
        let mut alternate = value.clone();
        alternate["schema_format"] = json!(1.0);
        let compact = serde_json::to_vec(&alternate).unwrap();
        assert_ne!(Sha256::digest(SCHEMA), Sha256::digest(&compact));
        for document in [SCHEMA, compact.as_slice()] {
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
        // because generate() still fails closed (BXG0041) on more than one capability.
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
        // over a single shared error descriptor. Exercised directly because generate() still
        // fails closed (BXG0041) on more than one capability.
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
        let adapter = adapter_source("greet", CanonicalType::U32);
        assert!(adapter.contains("::boxology_contract::TypeDescriptor::u32()"));
        assert!(adapter.contains("u32::decode(&input)"));
        assert!(!adapter.contains("::std::string::String::decode"));
        assert!(!adapter.contains("::boxology_contract::TypeDescriptor::string()"));
    }

    #[test]
    fn public_revision_tracks_every_public_semantic_and_only_public_semantics() {
        let base = revision(CONTRACT);
        for changed in [
            CONTRACT.replace("GreetError", "HelloError"),
            CONTRACT.replace("#[error]", "#[doc=\"error\"] #[error]"),
            CONTRACT.replace("#[error]", "#[deprecated] #[error]"),
            CONTRACT.replace("EmptyName", "MissingName"),
            CONTRACT.replace("EmptyName", "#[doc=\"empty\"] EmptyName"),
            CONTRACT.replace("EmptyName", "#[deprecated(note=\"old\")] EmptyName"),
            CONTRACT.replace("EmptyName", "EmptyName, Busy"),
            CONTRACT.replace("#[capability", "#[doc=\"greet\"] #[capability"),
            CONTRACT.replace("#[capability", "#[deprecated] #[capability"),
            CONTRACT.replace("fn greet", "fn welcome"),
            CONTRACT.replace("(name:", "(person:"),
        ] {
            assert_ne!(
                revision(&changed),
                base,
                "mutation did not change revision: {changed}"
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
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
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
                "[workspace]\nmembers=[\"generated/contract\",\"consumer\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology-contract={{version=\"=0.0.0\",path={:?}}}\n",
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
                "[workspace]\nmembers=[\"generated/contract\",\"implementation\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology={{path={:?}}}\nboxology-contract={{version=\"=0.0.0\",path={:?}}}\nboxology-runtime={{version=\"=0.0.0\",path={:?},features=[\"test-support\"]}}\n",
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
            generate(&request(&source, false, OUTPUTS.to_vec()))
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
            let diagnostics = generate(&request).unwrap_err();
            assert_eq!(diagnostics.as_slice().len(), 1);
            assert_eq!(diagnostics.as_slice()[0].code(), "BXG0040");
            assert_eq!(diagnostics.as_slice()[0].span(), expected_span);
        }
    }

    #[test]
    fn multiple_capabilities_fail_closed_with_bxg0041() {
        let source = "boxology::contract! { #[error] pub enum GreetError { EmptyName } #[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>; #[capability(exposure=external)] pub async fn shout(name:String)->Result<String,GreetError>; }";
        let request = request(source, false, OUTPUTS.to_vec());
        let expected_span = ParsedRustInputs::parse(&request)
            .and_then(|parsed| parsed.controlled_contract())
            .unwrap()
            .span();
        let diagnostics = generate(&request).unwrap_err();
        assert_eq!(diagnostics.as_slice().len(), 1);
        assert_eq!(diagnostics.as_slice()[0].code(), "BXG0041");
        assert_eq!(diagnostics.as_slice()[0].span(), expected_span);
    }

    #[test]
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
                "[workspace]\nmembers=[\"generated/contract\",\"consumer\"]\nresolver=\"3\"\n[workspace.dependencies]\nboxology-contract={{version=\"=0.0.0\",path={:?}}}\n",
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
}
