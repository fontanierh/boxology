//! Shared parser and owned model for controlled Boxology contract tokens.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::canonicalize_ordinary_rust_identifier;
use proc_macro2::TokenStream;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use syn::{
    Attribute, Expr, FnArg, ItemEnum, Lit, Meta, ReturnType, Token, Type, parse::Parse,
    spanned::Spanned,
};
/// A controlled contract block, independent of source spelling and location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contract {
    /// The domain-error declaration.
    pub error: ErrorDeclaration,
    /// The exported capability declarations in source order; always at least one.
    pub capabilities: Vec<CapabilityDeclaration>,
}
/// A controlled domain-error enum. Raw identifiers are rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorDeclaration {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note; empty means `#[deprecated]`.
    pub deprecation: Option<String>,
    /// Ordinary Rust identifier.
    pub name: String,
    /// Unit variants in declaration order.
    pub variants: Vec<ErrorVariant>,
}
/// One controlled unit error variant with decoded metadata and an ordinary name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorVariant {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note.
    pub deprecation: Option<String>,
    /// Ordinary Rust identifier.
    pub name: String,
}
/// A controlled unary asynchronous capability and its complete semantic metadata.
/// All names are ordinary identifiers; raw identifiers are rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityDeclaration {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note; empty means `#[deprecated]`.
    pub deprecation: Option<String>,
    /// Capability name.
    pub name: String,
    /// Input name.
    pub input_name: String,
    /// Canonical scalar leaf accepted as the single input.
    pub input_type: CanonicalType,
    /// Canonical scalar leaf produced on success.
    pub output_type: CanonicalType,
    /// Directly named in-block error type.
    pub error: String,
    /// Declared maximum exposure.
    pub exposure: &'static str,
    /// Declared idempotency, defaulting to none.
    pub idempotency: &'static str,
}
/// One canonical scalar leaf permitted as a capability input or output boundary type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalType {
    /// The `bool` leaf.
    Bool,
    /// The `u8` leaf.
    U8,
    /// The `u16` leaf.
    U16,
    /// The `u32` leaf.
    U32,
    /// The `u64` leaf.
    U64,
    /// The `i8` leaf.
    I8,
    /// The `i16` leaf.
    I16,
    /// The `i32` leaf.
    I32,
    /// The `i64` leaf.
    I64,
    /// The `f32` leaf.
    F32,
    /// The `f64` leaf.
    F64,
    /// The `String` leaf.
    String,
    /// The `Blob` leaf.
    Blob,
}
impl CanonicalType {
    /// Returns the exact Rust identifier naming the leaf.
    pub fn canonical_name(&self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
            Self::String => "String",
            Self::Blob => "Blob",
        }
    }
    /// Returns whether the leaf is the `String` boundary type.
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String)
    }
    /// Returns whether the leaf is the `Blob` boundary type.
    pub fn is_blob(&self) -> bool {
        matches!(self, Self::Blob)
    }
}
/// Version of the generation-consistency semantic encoding.
pub const SEMANTIC_ENCODING_VERSION: u32 = 1;

/// Encodes one controlled contract into the versioned canonical semantic format.
pub fn canonical_semantic_bytes(contract: &Contract) -> Vec<u8> {
    let mut out = b"boxology.contract-semantics\0".to_vec();
    out.extend_from_slice(&SEMANTIC_ENCODING_VERSION.to_be_bytes());
    count(&mut out, 1 + contract.capabilities.len());
    out.push(1);
    encode_metadata(&mut out, &contract.error.docs, &contract.error.deprecation);
    string(&mut out, &contract.error.name);
    count(&mut out, contract.error.variants.len());
    for variant in &contract.error.variants {
        out.push(0);
        encode_metadata(&mut out, &variant.docs, &variant.deprecation);
        string(&mut out, &variant.name);
    }
    for capability in &contract.capabilities {
        out.push(2);
        encode_metadata(&mut out, &capability.docs, &capability.deprecation);
        for value in [
            capability.name.as_str(),
            capability.input_name.as_str(),
            capability.input_type.canonical_name(),
            capability.output_type.canonical_name(),
            capability.error.as_str(),
            capability.exposure,
            capability.idempotency,
        ] {
            string(&mut out, value);
        }
    }
    out
}

/// Returns the SHA-256 generation-consistency digest of one controlled contract.
pub fn semantic_digest(contract: &Contract) -> [u8; 32] {
    semantic_artifacts(contract).1
}

/// Computes canonical bytes and their SHA-256 digest together.
pub fn semantic_artifacts(contract: &Contract) -> (Vec<u8>, [u8; 32]) {
    let bytes = canonical_semantic_bytes(contract);
    let digest = Sha256::digest(&bytes).into();
    (bytes, digest)
}

fn encode_metadata(out: &mut Vec<u8>, docs: &[String], deprecation: &Option<String>) {
    count(out, docs.len());
    for doc in docs {
        string(out, doc);
    }
    match deprecation {
        None => out.push(0),
        Some(note) => {
            out.push(1);
            string(out, note);
        }
    }
}

fn string(out: &mut Vec<u8>, value: &str) {
    count(out, value.len());
    out.extend_from_slice(value.as_bytes());
}

fn count(out: &mut Vec<u8>, value: usize) {
    out.extend_from_slice(&(value as u64).to_be_bytes());
}
/// Parses exact tokens from a direct `boxology::contract!` invocation.
///
/// # Errors
/// Returns an error when tokens use any unsupported form.
pub fn parse(tokens: TokenStream) -> syn::Result<Contract> {
    syn::parse2(tokens)
}

impl Parse for Contract {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let attrs = Attribute::parse_outer(input)?;
        let error = parse_error(&attrs, &input.parse()?)?;
        let mut capabilities = Vec::new();
        let mut names = BTreeSet::new();
        while !input.is_empty() {
            let attrs = Attribute::parse_outer(input)?;
            let capability = parse_capability(&attrs, input)?;
            if capability.error != error.name {
                return Err(
                    input.error("capability error must directly name an in-block #[error] enum")
                );
            }
            if !names.insert(capability.name.clone()) {
                return Err(input.error("capability names must be unique"));
            }
            capabilities.push(capability);
        }
        if capabilities.is_empty() {
            return Err(
                input.error("a contract requires one #[error] enum and at least one capability")
            );
        }
        Ok(Self {
            error,
            capabilities,
        })
    }
}

fn parse_error(attrs: &[Attribute], item: &ItemEnum) -> syn::Result<ErrorDeclaration> {
    let (docs, deprecation, marker) = metadata(attrs, "error")?;
    if !marker
        || !matches!(item.vis, syn::Visibility::Public(_))
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
        || item.variants.is_empty()
        || item
            .variants
            .iter()
            .any(|v| !v.fields.is_empty() || v.discriminant.is_some())
    {
        return Err(error(
            item,
            "#[error] requires a public non-generic enum of bare unit variants",
        ));
    }
    let variants = item
        .variants
        .iter()
        .map(|variant| {
            let (docs, deprecation, _) = metadata(&variant.attrs, "")?;
            Ok(ErrorVariant {
                docs,
                deprecation,
                name: identifier(&variant.ident)?,
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;
    let mut names = BTreeSet::new();
    if variants.iter().any(|variant| !names.insert(&variant.name)) {
        return Err(error(item, "error variant names must be unique"));
    }
    if let Some(variant) = item
        .variants
        .iter()
        .find(|variant| variant.ident == "Unknown")
    {
        return Err(error(
            &variant.ident,
            "error variant name `Unknown` is reserved",
        ));
    }
    Ok(ErrorDeclaration {
        docs,
        deprecation,
        name: identifier(&item.ident)?,
        variants,
    })
}

fn parse_capability(
    attrs: &[Attribute],
    input: syn::parse::ParseStream<'_>,
) -> syn::Result<CapabilityDeclaration> {
    let (docs, deprecation, marker) = metadata(attrs, "capability")?;
    if !marker {
        return Err(
            input.error("capability declaration requires #[capability(exposure = external)]")
        );
    }
    input.parse::<Token![pub]>()?;
    input.parse::<Token![async]>()?;
    input.parse::<Token![fn]>()?;
    let name: syn::Ident = input.parse()?;
    let name = identifier(&name)?;
    if !capability_name(&name) {
        return Err(input.error("capability name must match [a-z][a-z0-9_]*"));
    }
    let content;
    syn::parenthesized!(content in input);
    let args = content.parse_terminated(FnArg::parse, Token![,])?;
    let ReturnType::Type(_, output) = input.parse::<ReturnType>()? else {
        return Err(input.error("capability requires Result<String, Error>"));
    };
    input.parse::<Token![;]>()?;
    let [FnArg::Typed(arg)] = args.iter().collect::<Vec<_>>().as_slice() else {
        return Err(input.error("capability requires exactly one typed input and no receiver"));
    };
    let syn::Pat::Ident(input_ident) = arg.pat.as_ref() else {
        return Err(error(&arg.pat, "input must be a plain identifier"));
    };
    if !arg.attrs.is_empty()
        || !input_ident.attrs.is_empty()
        || input_ident.by_ref.is_some()
        || input_ident.mutability.is_some()
        || input_ident.subpat.is_some()
    {
        return Err(error(&arg.pat, "input must be an undecorated identifier"));
    }
    let Some(input_type) = leaf(&arg.ty) else {
        return Err(error(&arg.ty, "input type must be a canonical scalar leaf"));
    };
    let Some((output_type, error_name)) = result_error(&output)? else {
        return Err(error(
            &output,
            "output must be unqualified Result<Leaf, Error>",
        ));
    };
    Ok(CapabilityDeclaration {
        docs,
        deprecation,
        name,
        input_name: identifier(&input_ident.ident)?,
        input_type,
        output_type,
        error: error_name,
        exposure: "external",
        idempotency: "none",
    })
}

fn metadata(
    attrs: &[Attribute],
    marker_name: &str,
) -> syn::Result<(Vec<String>, Option<String>, bool)> {
    let mut docs = Vec::new();
    let mut deprecated = None;
    let mut marker = false;
    for attr in attrs {
        let name = attr
            .path()
            .get_ident()
            .ok_or_else(|| error(attr, "metadata must use an unqualified identifier"))?;
        let name = identifier(name)?;
        if name == "doc" {
            let Meta::NameValue(value) = &attr.meta else {
                return Err(error(attr, "doc must be a string"));
            };
            let Expr::Lit(value) = &value.value else {
                return Err(error(value, "doc must be a string"));
            };
            let Lit::Str(value) = &value.lit else {
                return Err(error(value, "doc must be a string"));
            };
            docs.push(value.value());
        } else if name == "deprecated" {
            if deprecated.is_some() {
                return Err(error(attr, "duplicate deprecated attribute"));
            }
            deprecated = Some(match &attr.meta {
                Meta::Path(_) => String::new(),
                Meta::List(list) => {
                    let pair: Pair<syn::LitStr> = list.parse_args()?;
                    if identifier(&pair.key)? != "note" {
                        return Err(error(&pair.key, "deprecated supports only note"));
                    }
                    pair.value.value()
                }
                Meta::NameValue(_) => {
                    return Err(error(attr, "invalid deprecated attribute"));
                }
            });
        } else if name == marker_name {
            if marker {
                return Err(error(attr, "duplicate marker"));
            }
            marker = true;
            if marker_name == "error" {
                if !matches!(attr.meta, Meta::Path(_)) {
                    return Err(error(attr, "#[error] takes no arguments"));
                }
            } else {
                let pair: Pair<syn::Ident> = attr.parse_args()?;
                if identifier(&pair.key)? != "exposure" || identifier(&pair.value)? != "external" {
                    return Err(error(&pair.value, "exposure must be external"));
                }
            }
        } else {
            return Err(error(attr, "unknown contract metadata"));
        }
    }
    Ok((docs, deprecated, marker))
}

struct Pair<T> {
    key: syn::Ident,
    value: T,
}
impl<T: Parse> Parse for Pair<T> {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let key = input.parse()?;
        input.parse::<Token![=]>()?;
        Ok(Self {
            key,
            value: input.parse()?,
        })
    }
}
/// Classifies a single-segment, non-raw, unqualified path type as one canonical scalar leaf.
fn leaf(ty: &Type) -> Option<CanonicalType> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let ident = path.path.get_ident()?;
    let name = ident.to_string();
    if name.starts_with("r#") {
        return None;
    }
    Some(match name.as_str() {
        "bool" => CanonicalType::Bool,
        "u8" => CanonicalType::U8,
        "u16" => CanonicalType::U16,
        "u32" => CanonicalType::U32,
        "u64" => CanonicalType::U64,
        "i8" => CanonicalType::I8,
        "i16" => CanonicalType::I16,
        "i32" => CanonicalType::I32,
        "i64" => CanonicalType::I64,
        "f32" => CanonicalType::F32,
        "f64" => CanonicalType::F64,
        "String" => CanonicalType::String,
        "Blob" => CanonicalType::Blob,
        _ => return None,
    })
}
fn result_error(ty: &Type) -> syn::Result<Option<(CanonicalType, String)>> {
    let Type::Path(result) = ty else {
        return Ok(None);
    };
    let Some(segment) = result.path.segments.first() else {
        return Ok(None);
    };
    if result.qself.is_some()
        || result.path.leading_colon.is_some()
        || result.path.segments.len() != 1
        || segment.ident != "Result"
    {
        return Ok(None);
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Ok(None);
    };
    let mut values = args.args.iter();
    let (
        Some(syn::GenericArgument::Type(ok)),
        Some(syn::GenericArgument::Type(Type::Path(error))),
        None,
    ) = (values.next(), values.next(), values.next())
    else {
        return Ok(None);
    };
    let Some(name) = error.path.get_ident() else {
        return Ok(None);
    };
    let Some(output_type) = leaf(ok) else {
        return Ok(None);
    };
    if error.qself.is_some() {
        return Ok(None);
    }
    Ok(Some((output_type, identifier(name)?)))
}
fn error(node: &impl Spanned, message: &str) -> syn::Error {
    syn::Error::new(node.span(), message)
}
fn identifier(ident: &syn::Ident) -> syn::Result<String> {
    let value = ident.to_string();
    if value.starts_with("r#") {
        return Err(error(ident, "contract identifiers must not be raw"));
    }
    canonicalize_ordinary_rust_identifier(&value).ok_or_else(|| {
        error(
            ident,
            "contract identifiers must be ordinary non-raw Rust identifiers",
        )
    })
}
fn capability_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxology_contract::is_ordinary_rust_identifier;

    const ERROR: &str = "#[error] pub enum GreetError { EmptyName }";
    const CAP: &str = "#[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>;";
    const HELLO_BYTES: &str = "626f786f6c6f67792e636f6e74726163742d73656d616e746963730000000001000000000000000201000000000000000000000000000000000a47726565744572726f720000000000000001000000000000000000000000000000000009456d7074794e616d65020000000000000000000000000000000005677265657400000000000000046e616d650000000000000006537472696e670000000000000006537472696e67000000000000000a47726565744572726f72000000000000000865787465726e616c00000000000000046e6f6e65";
    #[test]
    fn hello_parses_to_owned_semantics() {
        fn traits<T: Send + Sync + 'static>() {}
        traits::<Contract>();
        let contract = parse(format!("{ERROR} {CAP}").parse().unwrap()).unwrap();
        assert_eq!(contract.error.name, "GreetError");
        assert_eq!(contract.error.variants[0].name, "EmptyName");
        assert_eq!(contract.capabilities.len(), 1);
        assert_eq!(contract.capabilities[0].name, "greet");
        assert_eq!(contract.capabilities[0].input_name, "name");
        assert_eq!(contract.capabilities[0].error, "GreetError");
        assert_eq!(contract.capabilities[0].exposure, "external");
        assert_eq!(contract.capabilities[0].idempotency, "none");
        let metadata = parse(
            format!("#[doc=\"greet\"] #[deprecated] {ERROR} {CAP}")
                .parse()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(metadata.error.docs, ["greet"]);
        assert_eq!(metadata.error.deprecation.as_deref(), Some(""));
    }
    #[test]
    fn unsupported_forms_and_duplicate_declarations_fail_closed() {
        for source in [
            format!("#[error] pub enum r#GreetError{{EmptyName}} {CAP}"),
            format!("#[error] pub enum GreetError{{r#EmptyName}} {CAP}"),
            format!("{ERROR} {}", CAP.replace("greet", "r#greet")),
            format!("{ERROR} {}", CAP.replace("name:String", "r#name:String")),
            format!("{} {CAP}", ERROR.replace("error", "r#error")),
            format!("{ERROR} {}", CAP.replace("exposure", "r#exposure")),
            format!("{ERROR} {}", CAP.replace("external", "r#external")),
            format!("{ERROR} {}", CAP.replace("name:String", "name:r#String")),
            format!("{ERROR} {}", CAP.replace("Result", "r#Result")),
            format!("{ERROR} {}", CAP.replace("GreetError>", "r#GreetError>")),
            format!(
                "{ERROR} {}",
                CAP.replace("name:String", "#[doc=\"x\"] name:String")
            ),
            format!("{ERROR} {}", CAP.replace("name:String", "mut name:String")),
            format!("{ERROR} {}", CAP.replace("name:String", "ref name:String")),
            format!("{ERROR} {}", CAP.replace("name:String", "name @ _:String")),
            format!("{ERROR} {}", CAP.replace("name:String", "self,name:String")),
            format!(
                "{ERROR} {}",
                CAP.replace("name:String", "name:crate::String")
            ),
            format!("{ERROR} {}", CAP.replace("GreetError>", "Other>")),
            format!("{ERROR} {}", CAP.replace("external)", "external,other=x)")),
            format!("#[error] pub enum GreetError{{EmptyName,EmptyName}} {CAP}"),
            format!("{ERROR} {}", CAP.replace("greet", "Greet")),
            format!("{ERROR} {ERROR} {CAP}"),
            format!("{ERROR} {CAP} {ERROR}"),
            format!("{CAP} {ERROR}"),
            format!("{CAP} {CAP} {ERROR}"),
            format!("{CAP} {ERROR} {CAP}"),
        ] {
            assert!(parse(source.parse().unwrap()).is_err(), "{source}");
        }
        for boundary in [
            "Vec<u8>",
            "Option<u8>",
            "BTreeMap<String,u8>",
            "Field<u8>",
            "Secret<u8>",
            "crate::u32",
            "&str",
            "[u8;4]",
            "(u8,u8)",
            "u128",
            "usize",
            "isize",
            "char",
            "str",
            "r#u32",
            "GreetError",
        ] {
            let input = format!(
                "{ERROR} {}",
                CAP.replace("name:String", &format!("name:{boundary}"))
            );
            let output = format!(
                "{ERROR} {}",
                CAP.replace("Result<String", &format!("Result<{boundary}"))
            );
            assert!(parse(input.parse().unwrap()).is_err(), "input {boundary}");
            assert!(parse(output.parse().unwrap()).is_err(), "output {boundary}");
        }
    }
    #[test]
    fn non_leaf_input_type_has_a_stable_diagnostic() {
        let source = format!("{ERROR} {}", CAP.replace("name:String", "name:Vec<u8>"));
        let diagnostic = parse(source.parse().unwrap()).unwrap_err();
        assert_eq!(
            diagnostic.to_string(),
            "input type must be a canonical scalar leaf"
        );
    }
    #[test]
    fn non_leaf_output_type_has_a_stable_diagnostic() {
        let source = format!("{ERROR} {}", CAP.replace("Result<String", "Result<Vec<u8>"));
        let diagnostic = parse(source.parse().unwrap()).unwrap_err();
        assert_eq!(
            diagnostic.to_string(),
            "output must be unqualified Result<Leaf, Error>"
        );
    }
    #[test]
    fn every_canonical_leaf_is_accepted_at_input_and_output() {
        let leaves = [
            "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "String",
            "Blob",
        ];
        for name in leaves {
            let input = parse(
                format!(
                    "{ERROR} {}",
                    CAP.replace("name:String", &format!("name:{name}"))
                )
                .parse()
                .unwrap(),
            )
            .unwrap();
            assert_eq!(input.capabilities[0].input_type.canonical_name(), name);
            assert_eq!(input.capabilities[0].output_type.canonical_name(), "String");
            let output = parse(
                format!(
                    "{ERROR} {}",
                    CAP.replace("Result<String", &format!("Result<{name}"))
                )
                .parse()
                .unwrap(),
            )
            .unwrap();
            assert_eq!(output.capabilities[0].output_type.canonical_name(), name);
            assert_eq!(output.capabilities[0].input_type.canonical_name(), "String");
        }
    }
    #[test]
    fn non_string_boundary_semantic_encoding_is_pinned() {
        const NON_STRING_BYTES: &str = "626f786f6c6f67792e636f6e74726163742d73656d616e746963730000000001000000000000000201000000000000000000000000000000000a47726565744572726f720000000000000001000000000000000000000000000000000009456d7074794e616d65020000000000000000000000000000000005677265657400000000000000046e616d6500000000000000037533320000000000000004626f6f6c000000000000000a47726565744572726f72000000000000000865787465726e616c00000000000000046e6f6e65";
        const NON_STRING_DIGEST: &str =
            "18c8df53ed2aecbb124a34889d88997ae28a01f7ce72904fc719c8b991635532";
        let source = format!(
            "{ERROR} {}",
            CAP.replace("name:String", "name:u32")
                .replace("Result<String", "Result<bool")
        );
        let contract = parse(source.parse().unwrap()).unwrap();
        assert_eq!(contract.capabilities[0].input_type, CanonicalType::U32);
        assert_eq!(contract.capabilities[0].output_type, CanonicalType::Bool);
        assert_eq!(hex(&canonical_semantic_bytes(&contract)), NON_STRING_BYTES);
        assert_eq!(hex(&semantic_digest(&contract)), NON_STRING_DIGEST);
    }
    const STORE_ERROR: &str = "#[error] pub enum StoreError { Missing }";
    const GET: &str =
        "#[capability(exposure=external)] pub async fn get(key:String)->Result<String,StoreError>;";
    const PUT: &str =
        "#[capability(exposure=external)] pub async fn put(value:String)->Result<bool,StoreError>;";
    #[test]
    fn multiple_capabilities_under_one_error_parse_in_order_and_pin() {
        const MULTI_BYTES: &str = "626f786f6c6f67792e636f6e74726163742d73656d616e746963730000000001000000000000000301000000000000000000000000000000000a53746f72654572726f7200000000000000010000000000000000000000000000000000074d697373696e6702000000000000000000000000000000000367657400000000000000036b65790000000000000006537472696e670000000000000006537472696e67000000000000000a53746f72654572726f72000000000000000865787465726e616c00000000000000046e6f6e65020000000000000000000000000000000003707574000000000000000576616c75650000000000000006537472696e670000000000000004626f6f6c000000000000000a53746f72654572726f72000000000000000865787465726e616c00000000000000046e6f6e65";
        const MULTI_DIGEST: &str =
            "58aaace524b71255be6b3a0c007fc79d2a989d22c85a061cba9403e8eab9249d";
        let contract = parse(format!("{STORE_ERROR} {GET} {PUT}").parse().unwrap()).unwrap();
        assert_eq!(contract.error.name, "StoreError");
        assert_eq!(contract.capabilities.len(), 2);
        assert_eq!(contract.capabilities[0].name, "get");
        assert_eq!(contract.capabilities[1].name, "put");
        assert_eq!(contract.capabilities[0].output_type, CanonicalType::String);
        assert_eq!(contract.capabilities[1].output_type, CanonicalType::Bool);
        assert_eq!(hex(&canonical_semantic_bytes(&contract)), MULTI_BYTES);
        assert_eq!(hex(&semantic_digest(&contract)), MULTI_DIGEST);
        let swapped = parse(format!("{STORE_ERROR} {PUT} {GET}").parse().unwrap()).unwrap();
        assert_eq!(swapped.capabilities[0].name, "put");
        assert_eq!(swapped.capabilities[1].name, "get");
        assert_ne!(semantic_digest(&contract), semantic_digest(&swapped));
    }
    #[test]
    fn capability_error_mismatch_and_duplicate_names_fail_closed() {
        let mismatch = format!(
            "{STORE_ERROR} {}",
            GET.replace("StoreError>", "OtherError>")
        );
        assert!(parse(mismatch.parse().unwrap()).is_err(), "{mismatch}");
        let duplicate = format!("{STORE_ERROR} {GET} {GET}");
        assert!(parse(duplicate.parse().unwrap()).is_err(), "{duplicate}");
    }
    #[test]
    fn reserved_unknown_error_variant_has_a_stable_diagnostic() {
        let source = format!("#[error] pub enum GreetError{{Unknown}} {CAP}");
        let diagnostic = parse(source.parse().unwrap()).unwrap_err();
        assert_eq!(
            diagnostic.to_string(),
            "error variant name `Unknown` is reserved"
        );
    }
    #[test]
    fn nfc_equivalent_spellings_have_one_identity_and_cannot_duplicate() {
        let source = "#[error] pub enum Cafe\u{301}Error { EmptyName }
            #[capability(exposure=external)]
            pub async fn greet(name:String)->Result<String,CaféError>;";
        let contract = parse(source.parse().unwrap()).unwrap();
        assert_eq!(contract.error.name, "CaféError");
        assert_eq!(contract.capabilities[0].error, "CaféError");

        let duplicate = format!("#[error] pub enum GreetError{{Café,Cafe\u{301}}} {CAP}");
        let diagnostic = parse(duplicate.parse().unwrap()).unwrap_err();
        assert_eq!(diagnostic.to_string(), "error variant names must be unique");
    }
    #[test]
    fn result_error_identifier_failures_own_their_diagnostic_span() {
        for (name, message) in [
            (
                "gen",
                "contract identifiers must be ordinary non-raw Rust identifiers",
            ),
            ("r#gen", "contract identifiers must not be raw"),
        ] {
            let source = format!(
                "{ERROR} {}",
                CAP.replace("GreetError>", &format!("{name}>"))
            );
            let diagnostic = parse(source.parse().unwrap()).unwrap_err();
            assert_eq!(diagnostic.to_string(), message);
            assert_eq!(&source[diagnostic.span().byte_range()], name);
        }

        let source = format!(
            "{ERROR} {}",
            CAP.replace("Result<String,GreetError>", "Result<Vec<u8>,gen>")
        );
        let diagnostic = parse(source.parse().unwrap()).unwrap_err();
        assert_eq!(
            diagnostic.to_string(),
            "output must be unqualified Result<Leaf, Error>"
        );
    }
    #[test]
    fn shared_identifier_validator_is_the_final_gate_after_syn_lexing() {
        let cases = [
            ("hello", true, true),
            ("Москва", true, true),
            ("e\u{301}", true, true),
            ("_name", true, true),
            ("_", false, false),
            ("9lives", false, false),
            ("r#name", false, true),
            ("gen", false, true),
            ("async", false, false),
            ("a\u{200c}", false, true),
        ];
        for (value, expected, syn_accepts) in cases {
            assert_eq!(
                is_ordinary_rust_identifier(value),
                expected,
                "shared validator classification for {value:?}"
            );
            assert_eq!(
                syn::parse_str::<syn::Ident>(value).is_ok(),
                syn_accepts,
                "syn parser classification for {value:?}"
            );
        }

        let source = format!("#[error] pub enum GreetError{{gen}} {CAP}");
        let diagnostic = parse(source.parse().unwrap()).unwrap_err();
        assert_eq!(
            diagnostic.to_string(),
            "contract identifiers must be ordinary non-raw Rust identifiers"
        );
    }
    #[test]
    fn semantic_encoding_is_pinned_and_ignores_only_non_semantic_spelling() {
        let hello = parse(format!("{ERROR} {CAP}").parse().unwrap()).unwrap();
        let bytes = canonical_semantic_bytes(&hello);
        assert_eq!(hex(&bytes), HELLO_BYTES);
        assert_eq!(
            hex(&semantic_digest(&hello)),
            "545f142b0ced7670e3f9efc7bcaaf3b7a2a0b2b790e5b48acaa85e4901c89b18"
        );
        let respelled = parse(format!("/*x*/{ERROR}//x\n{CAP}").parse().unwrap()).unwrap();
        assert_eq!(semantic_digest(&hello), semantic_digest(&respelled));
        let mutations: &[fn(&mut Contract)] = &[
            |c| c.error.name = "OtherError".into(),
            |c| c.error.docs.push("docs".into()),
            |c| c.error.deprecation = Some(String::new()),
            |c| c.error.variants.push(c.error.variants[0].clone()),
            |c| c.error.variants[0].docs.push("docs".into()),
            |c| c.error.variants[0].deprecation = Some(String::new()),
            |c| c.error.variants[0].name = "Other".into(),
            |c| c.capabilities[0].docs.push("docs".into()),
            |c| c.capabilities[0].deprecation = Some(String::new()),
            |c| c.capabilities[0].name = "other".into(),
            |c| c.capabilities[0].input_name = "other".into(),
            |c| c.capabilities[0].error = "OtherError".into(),
        ];
        for mutate in mutations {
            let mut changed = hello.clone();
            mutate(&mut changed);
            assert_ne!(semantic_digest(&hello), semantic_digest(&changed));
        }
        let mut ordered = hello.clone();
        ordered
            .error
            .variants
            .push(ordered.error.variants[0].clone());
        ordered.error.variants[1].name = "Other".into();
        let mut reversed = ordered.clone();
        reversed.error.variants.swap(0, 1);
        assert_ne!(semantic_digest(&ordered), semantic_digest(&reversed));
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
