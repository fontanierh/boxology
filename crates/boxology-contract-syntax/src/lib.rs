//! Shared parser and owned model for controlled Boxology contract tokens.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::{ExposureLevel, Idempotency, canonicalize_ordinary_rust_identifier};
use proc_macro2::TokenStream;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use syn::{
    Attribute, Expr, FnArg, ItemEnum, Lit, Meta, ReturnType, Token, Type, parse::Parse,
    parse::ParseStream, spanned::Spanned,
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
    /// Error variants in declaration order.
    pub variants: Vec<ErrorVariant>,
}
/// One controlled error variant with decoded metadata, an ordinary name, and a payload shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorVariant {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note.
    pub deprecation: Option<String>,
    /// Ordinary Rust identifier.
    pub name: String,
    /// The variant's unit, one-value, or named-field payload.
    pub payload: VariantPayload,
}
/// The supported payload shape of one controlled error variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VariantPayload {
    /// A variant with no payload, such as `Empty`.
    Unit,
    /// A variant with exactly one unnamed scalar field, such as `Code(u32)`.
    Value(VariantValue),
    /// A variant with zero or more named scalar fields, such as `Detail { message: String }`.
    Named(Vec<VariantField>),
}
impl VariantPayload {
    /// Returns whether this payload is the distinct unit shape.
    pub fn is_unit(&self) -> bool {
        matches!(self, Self::Unit)
    }
}
/// The metadata and canonical scalar type of one unnamed variant field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VariantValue {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note.
    pub deprecation: Option<String>,
    /// The canonical scalar leaf type.
    pub ty: CanonicalType,
}
/// One named variant field with decoded metadata and a canonical scalar type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VariantField {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note.
    pub deprecation: Option<String>,
    /// Ordinary Rust identifier in canonical NFC spelling.
    pub name: String,
    /// The canonical scalar leaf type.
    pub ty: CanonicalType,
}
/// A controlled unary asynchronous capability and its complete semantic metadata.
/// All names are ordinary identifiers; raw identifiers are rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityDeclaration {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note; empty means `#[deprecated]`.
    pub deprecation: Option<String>,
    /// Wire capability identity (schema, descriptor, revision, digest).
    pub name: String,
    /// Rust surface spelling of the capability method.
    pub rust_name: String,
    /// Input name.
    pub input_name: String,
    /// Canonical scalar leaf accepted as the single input.
    pub input_type: CanonicalType,
    /// Canonical scalar leaf produced on success.
    pub output_type: CanonicalType,
    /// Directly named in-block error type.
    pub error: String,
    /// Declared maximum exposure.
    pub exposure: ExposureLevel,
    /// Declared idempotency, defaulting to none.
    pub idempotency: Idempotency,
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

/// Wire spelling of an exposure level in the semantic encoding and public-revision projection.
pub fn exposure_spelling(level: ExposureLevel) -> &'static str {
    match level {
        ExposureLevel::CodeOnly => "code_only",
        ExposureLevel::Internal => "internal",
        ExposureLevel::External => "external",
    }
}

/// Wire spelling of an idempotency property in the semantic encoding and public-revision projection.
pub fn idempotency_spelling(value: Idempotency) -> &'static str {
    match value {
        Idempotency::None => "none",
        Idempotency::Inherent => "inherent",
    }
}

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
        out.push(match &variant.payload {
            VariantPayload::Unit => 0,
            VariantPayload::Value(_) => 1,
            VariantPayload::Named(_) => 2,
        });
        encode_metadata(&mut out, &variant.docs, &variant.deprecation);
        string(&mut out, &variant.name);
        match &variant.payload {
            VariantPayload::Unit => {}
            VariantPayload::Value(value) => {
                encode_metadata(&mut out, &value.docs, &value.deprecation);
                string(&mut out, value.ty.canonical_name());
            }
            VariantPayload::Named(fields) => {
                count(&mut out, fields.len());
                for field in fields {
                    encode_metadata(&mut out, &field.docs, &field.deprecation);
                    string(&mut out, &field.name);
                    string(&mut out, field.ty.canonical_name());
                }
            }
        }
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
            exposure_spelling(capability.exposure),
            idempotency_spelling(capability.idempotency),
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
        let mut rust_names = BTreeSet::new();
        while !input.is_empty() {
            let attrs = Attribute::parse_outer(input)?;
            let (capability, rust_ident) = parse_capability(&attrs, input)?;
            if capability.error != error.name {
                return Err(
                    input.error("capability error must directly name an in-block #[error] enum")
                );
            }
            if !names.insert(capability.name.clone()) {
                return Err(input.error("capability names must be unique"));
            }
            if !rust_names.insert(capability.rust_name.clone()) {
                return Err(syn::Error::new(
                    rust_ident.span(),
                    "capability Rust names must be unique",
                ));
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
    if marker.is_none()
        || !matches!(item.vis, syn::Visibility::Public(_))
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
        || item.variants.is_empty()
    {
        return Err(error(
            item,
            "#[error] requires a public non-generic enum of supported scalar variants",
        ));
    }
    if let Some(variant) = item
        .variants
        .iter()
        .find(|variant| variant.discriminant.is_some())
    {
        return Err(error(variant, "error variants must not have discriminants"));
    }
    let variants = item
        .variants
        .iter()
        .map(|variant| {
            let (docs, deprecation, _) = metadata(&variant.attrs, "")?;
            let payload = match &variant.fields {
                syn::Fields::Unit => VariantPayload::Unit,
                syn::Fields::Unnamed(fields) => {
                    if fields.unnamed.len() != 1 {
                        return Err(error(
                            variant,
                            "error variants must have exactly one unnamed field",
                        ));
                    }
                    let field = fields
                        .unnamed
                        .first()
                        .expect("one unnamed field was checked above");
                    let (docs, deprecation, _) = metadata(&field.attrs, "")?;
                    if !matches!(field.vis, syn::Visibility::Inherited) {
                        return Err(error(
                            &field.vis,
                            "error variant fields must not have visibility",
                        ));
                    }
                    let ty = leaf(&field.ty).ok_or_else(|| {
                        error(
                            &field.ty,
                            "error variant field type must be a canonical scalar leaf",
                        )
                    })?;
                    VariantPayload::Value(VariantValue {
                        docs,
                        deprecation,
                        ty,
                    })
                }
                syn::Fields::Named(fields) => {
                    let mut names = BTreeSet::new();
                    let mut fields_out = Vec::with_capacity(fields.named.len());
                    for field in &fields.named {
                        let (docs, deprecation, _) = metadata(&field.attrs, "")?;
                        if !matches!(field.vis, syn::Visibility::Inherited) {
                            return Err(error(
                                &field.vis,
                                "error variant fields must not have visibility",
                            ));
                        }
                        let name = identifier(
                            field
                                .ident
                                .as_ref()
                                .expect("syn named fields always have identifiers"),
                        )?;
                        if !names.insert(name.clone()) {
                            return Err(error(
                                field
                                    .ident
                                    .as_ref()
                                    .expect("syn named fields always have identifiers"),
                                "error variant field names must be unique",
                            ));
                        }
                        let ty = leaf(&field.ty).ok_or_else(|| {
                            error(
                                &field.ty,
                                "error variant field type must be a canonical scalar leaf",
                            )
                        })?;
                        fields_out.push(VariantField {
                            docs,
                            deprecation,
                            name,
                            ty,
                        });
                    }
                    VariantPayload::Named(fields_out)
                }
            };
            Ok(ErrorVariant {
                docs,
                deprecation,
                name: identifier(&variant.ident)?,
                payload,
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
    input: ParseStream<'_>,
) -> syn::Result<(CapabilityDeclaration, syn::Ident)> {
    let (docs, deprecation, marker) = metadata(attrs, "capability")?;
    let Some(marker) = marker else {
        return Err(input.error("capability declaration requires #[capability]"));
    };
    let (name_override, exposure, idempotency) = parse_capability_metadata(marker)?;
    input.parse::<Token![pub]>()?;
    input.parse::<Token![async]>()?;
    input.parse::<Token![fn]>()?;
    let name_ident: syn::Ident = input.parse()?;
    let rust_name = identifier(&name_ident)?;
    let name = match name_override {
        Some(overridden) => overridden,
        None => {
            if !capability_name(&rust_name) {
                return Err(error(
                    &name_ident,
                    "capability name must match [a-z][a-z0-9_]*",
                ));
            }
            rust_name.clone()
        }
    };
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
    Ok((
        CapabilityDeclaration {
            docs,
            deprecation,
            name,
            rust_name,
            input_name: identifier(&input_ident.ident)?,
            input_type,
            output_type,
            error: error_name,
            exposure,
            idempotency,
        },
        name_ident,
    ))
}

fn parse_capability_metadata(
    attr: &Attribute,
) -> syn::Result<(Option<String>, ExposureLevel, Idempotency)> {
    let mut name = None;
    let mut exposure = None;
    let mut idempotency = None;
    match &attr.meta {
        Meta::Path(_) => {}
        Meta::List(list) => list.parse_args_with(|input: ParseStream<'_>| {
            while !input.is_empty() {
                let key: syn::Ident = input.parse()?;
                let key_name = identifier(&key)?;
                input.parse::<Token![=]>()?;
                match key_name.as_str() {
                    "name" => {
                        if name.is_some() {
                            return Err(error(&key, "duplicate capability metadata"));
                        }
                        let lit = input.parse::<Lit>()?;
                        let Lit::Str(value) = lit else {
                            return Err(error(&lit, "capability name override must be a string"));
                        };
                        let overridden = value.value();
                        if !capability_name(&overridden) {
                            return Err(error(
                                &value,
                                "capability name override must match [a-z][a-z0-9_]*",
                            ));
                        }
                        if canonicalize_ordinary_rust_identifier(&overridden).as_deref()
                            != Some(overridden.as_str())
                        {
                            return Err(error(
                                &value,
                                "capability name override must be an ordinary non-raw Rust identifier",
                            ));
                        }
                        name = Some(overridden);
                    }
                    "exposure" => {
                        if exposure.is_some() {
                            return Err(error(&key, "duplicate capability metadata"));
                        }
                        let value: syn::Ident = input.parse()?;
                        exposure = Some(match identifier(&value)?.as_str() {
                            "code_only" => ExposureLevel::CodeOnly,
                            "internal" => ExposureLevel::Internal,
                            "external" => ExposureLevel::External,
                            _ => {
                                return Err(error(
                                    &value,
                                    "exposure must be code_only, internal, or external",
                                ));
                            }
                        });
                    }
                    "idempotency" => {
                        if idempotency.is_some() {
                            return Err(error(&key, "duplicate capability metadata"));
                        }
                        let value: syn::Ident = input.parse()?;
                        idempotency = Some(match identifier(&value)?.as_str() {
                            "none" => Idempotency::None,
                            "inherent" => Idempotency::Inherent,
                            "keyed" => {
                                return Err(error(
                                    &value,
                                    "idempotency keyed is not supported in v0",
                                ));
                            }
                            _ => {
                                return Err(error(&value, "idempotency must be none or inherent"));
                            }
                        });
                    }
                    _ => return Err(error(&key, "unknown capability metadata")),
                }
                if input.is_empty() {
                    break;
                }
                input.parse::<Token![,]>()?;
            }
            Ok(())
        })?,
        Meta::NameValue(_) => {
            return Err(error(attr, "capability metadata must be key = value pairs"));
        }
    }
    Ok((
        name,
        exposure.unwrap_or(ExposureLevel::CodeOnly),
        idempotency.unwrap_or(Idempotency::None),
    ))
}

fn metadata<'a>(
    attrs: &'a [Attribute],
    marker_name: &str,
) -> syn::Result<(Vec<String>, Option<String>, Option<&'a Attribute>)> {
    let mut docs = Vec::new();
    let mut deprecated = None;
    let mut marker = None;
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
        } else if !marker_name.is_empty() && name == marker_name {
            if marker.is_some() {
                return Err(error(attr, "duplicate marker"));
            }
            if marker_name == "error" && !matches!(attr.meta, Meta::Path(_)) {
                return Err(error(attr, "#[error] takes no arguments"));
            }
            marker = Some(attr);
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
    const META_BYTES: &str = "626f786f6c6f67792e636f6e74726163742d73656d616e746963730000000001000000000000000201000000000000000000000000000000000145000000000000000100000000000000000000000000000000000156020000000000000000000000000000000007726573637565640000000000000001780000000000000006537472696e670000000000000006537472696e670000000000000001450000000000000008696e7465726e616c0000000000000008696e686572656e74";
    const META_DIGEST: &str = "9b987115e4e54d5895cce41117ddfd589090ec993922b37dfcb5dad096e3849d";
    #[test]
    fn hello_parses_to_owned_semantics() {
        fn traits<T: Send + Sync + 'static>() {}
        traits::<Contract>();
        let contract = parse(format!("{ERROR} {CAP}").parse().unwrap()).unwrap();
        assert_eq!(contract.error.name, "GreetError");
        assert_eq!(contract.error.variants[0].name, "EmptyName");
        assert_eq!(contract.capabilities.len(), 1);
        assert_eq!(contract.capabilities[0].name, "greet");
        assert_eq!(contract.capabilities[0].rust_name, "greet");
        assert_eq!(contract.capabilities[0].input_name, "name");
        assert_eq!(contract.capabilities[0].error, "GreetError");
        assert_eq!(contract.capabilities[0].exposure, ExposureLevel::External);
        assert_eq!(contract.capabilities[0].idempotency, Idempotency::None);
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
    fn omitted_metadata_takes_fail_safe_defaults() {
        let contract = parse(
            format!(
                "{ERROR} #[capability] pub async fn greet(name:String)->Result<String,GreetError>;"
            )
            .parse()
            .unwrap(),
        )
        .unwrap();
        assert_eq!(contract.capabilities[0].exposure, ExposureLevel::CodeOnly);
        assert_eq!(contract.capabilities[0].idempotency, Idempotency::None);
    }
    #[test]
    fn capability_metadata_accept_matrix() {
        let cases = [
            (
                "exposure=code_only",
                ExposureLevel::CodeOnly,
                Idempotency::None,
            ),
            (
                "exposure=internal",
                ExposureLevel::Internal,
                Idempotency::None,
            ),
            (
                "exposure=external",
                ExposureLevel::External,
                Idempotency::None,
            ),
            (
                "idempotency=none",
                ExposureLevel::CodeOnly,
                Idempotency::None,
            ),
            (
                "idempotency=inherent",
                ExposureLevel::CodeOnly,
                Idempotency::Inherent,
            ),
            (
                "exposure=code_only,idempotency=none",
                ExposureLevel::CodeOnly,
                Idempotency::None,
            ),
            (
                "exposure=code_only,idempotency=inherent",
                ExposureLevel::CodeOnly,
                Idempotency::Inherent,
            ),
            (
                "exposure=internal,idempotency=none",
                ExposureLevel::Internal,
                Idempotency::None,
            ),
            (
                "exposure=internal,idempotency=inherent",
                ExposureLevel::Internal,
                Idempotency::Inherent,
            ),
            (
                "exposure=external,idempotency=none",
                ExposureLevel::External,
                Idempotency::None,
            ),
            (
                "exposure=external,idempotency=inherent",
                ExposureLevel::External,
                Idempotency::Inherent,
            ),
            (
                "idempotency=inherent,exposure=internal",
                ExposureLevel::Internal,
                Idempotency::Inherent,
            ),
            (
                "exposure=external,",
                ExposureLevel::External,
                Idempotency::None,
            ),
            ("", ExposureLevel::CodeOnly, Idempotency::None),
        ];
        for (args, exposure, idempotency) in cases {
            let marker = if args.is_empty() {
                "#[capability()]".to_owned()
            } else {
                format!("#[capability({args})]")
            };
            let contract = parse(
                format!(
                    "{ERROR} {marker} pub async fn greet(name:String)->Result<String,GreetError>;"
                )
                .parse()
                .unwrap(),
            )
            .unwrap();
            assert_eq!(contract.capabilities[0].exposure, exposure, "{args}");
            assert_eq!(contract.capabilities[0].idempotency, idempotency, "{args}");
        }
    }
    #[test]
    fn name_override_replaces_identity_and_digest() {
        let overridden = parse(
            format!(
                "{ERROR} #[capability(name=\"rescued\")] pub async fn BadName(name:String)->Result<String,GreetError>;"
            )
            .parse()
            .unwrap(),
        )
        .unwrap();
        assert_eq!(overridden.capabilities[0].name, "rescued");
        assert_eq!(overridden.capabilities[0].rust_name, "BadName");
        let without = parse(
            format!(
                "{ERROR} #[capability] pub async fn greet(name:String)->Result<String,GreetError>;"
            )
            .parse()
            .unwrap(),
        )
        .unwrap();
        assert_ne!(semantic_digest(&overridden), semantic_digest(&without));
    }
    #[test]
    fn capability_metadata_rejections_have_precise_spans() {
        let rejected = [
            (
                "#[capability(unknown=external)]",
                "unknown capability metadata",
                "unknown",
            ),
            (
                "#[capability(exposure=external,exposure=internal)]",
                "duplicate capability metadata",
                "exposure",
            ),
            (
                "#[capability(name=7)]",
                "capability name override must be a string",
                "7",
            ),
            (
                "#[capability(name=\"Bad-Name\")]",
                "capability name override must match [a-z][a-z0-9_]*",
                "\"Bad-Name\"",
            ),
            (
                "#[capability(name=\"match\")]",
                "capability name override must be an ordinary non-raw Rust identifier",
                "\"match\"",
            ),
            (
                "#[capability(name=\"self\")]",
                "capability name override must be an ordinary non-raw Rust identifier",
                "\"self\"",
            ),
            (
                "#[capability(exposure=private)]",
                "exposure must be code_only, internal, or external",
                "private",
            ),
            (
                "#[capability(idempotency=later)]",
                "idempotency must be none or inherent",
                "later",
            ),
            (
                "#[capability(idempotency=keyed)]",
                "idempotency keyed is not supported in v0",
                "keyed",
            ),
        ];
        for (marker, message, slice) in rejected {
            let source = format!(
                "{ERROR} {marker} pub async fn greet(name:String)->Result<String,GreetError>;"
            );
            let error = parse(source.parse().unwrap()).unwrap_err();
            assert_eq!(error.to_string(), message, "{marker}");
            assert_eq!(&source[error.span().byte_range()], slice, "{marker}");
        }
        let source = format!(
            "{ERROR} #[capability(name=\"alpha\",exposure=external)] pub async fn same(n:String)->Result<String,GreetError>; #[capability(name=\"beta\",exposure=external)] pub async fn same(n:String)->Result<String,GreetError>;"
        );
        let error = parse(source.parse().unwrap()).unwrap_err();
        assert_eq!(error.to_string(), "capability Rust names must be unique");
        assert_eq!(&source[error.span().byte_range()], "same");
    }
    #[test]
    fn non_default_metadata_semantic_encoding_is_pinned() {
        let contract = parse(
            "#[error] pub enum E { V } #[capability(name=\"rescued\",exposure=internal,idempotency=inherent)] pub async fn BadName(x:String)->Result<String,E>;"
                .parse()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(hex(&canonical_semantic_bytes(&contract)), META_BYTES);
        assert_eq!(hex(&semantic_digest(&contract)), META_DIGEST);
        assert_eq!(
            hex(&canonical_semantic_bytes(
                &parse(format!("{ERROR} {CAP}").parse().unwrap()).unwrap()
            )),
            HELLO_BYTES
        );
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
            |c| c.capabilities[0].exposure = ExposureLevel::Internal,
            |c| c.capabilities[0].idempotency = Idempotency::Inherent,
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

    #[test]
    fn payload_variants_preserve_full_ordered_model_and_literal_semantics() {
        const MIXED: &str = r#"#[doc = "failure"] #[deprecated(note = "old")] #[error]
            pub enum Fault {
                #[doc = "unit"] Unit,
                #[doc = "code"] #[deprecated(note = "obsolete")] Code(
                    #[doc = "value"] #[deprecated] u32
                ),
                Detail {
                    #[doc = "message field"] message: String,
                    #[deprecated(note = "later")] retryable: bool,
                },
                Empty {},
            }
            #[capability(exposure = external)]
            pub async fn greet(name: String) -> Result<String, Fault>;"#;
        const MIXED_BYTES: &str = "626f786f6c6f67792e636f6e74726163742d73656d616e746963730000000001000000000000000201000000000000000100000000000000076661696c7572650100000000000000036f6c6400000000000000054661756c7400000000000000040000000000000000010000000000000004756e6974000000000000000004556e69740100000000000000010000000000000004636f64650100000000000000086f62736f6c6574650000000000000004436f64650000000000000001000000000000000576616c7565010000000000000000000000000000000375333202000000000000000000000000000000000644657461696c00000000000000020000000000000001000000000000000d6d657373616765206669656c640000000000000000076d6573736167650000000000000006537472696e6700000000000000000100000000000000056c617465720000000000000009726574727961626c650000000000000004626f6f6c020000000000000000000000000000000005456d7074790000000000000000020000000000000000000000000000000005677265657400000000000000046e616d650000000000000006537472696e670000000000000006537472696e6700000000000000054661756c74000000000000000865787465726e616c00000000000000046e6f6e65";
        const MIXED_DIGEST: &str =
            "a662d7d8c096a3ce1690588651ff05b516e6f4fef68b29bf320cf6b413980b6a";
        let contract = parse(MIXED.parse().unwrap()).unwrap();
        assert!(contract.error.variants[0].payload.is_unit());
        assert!(matches!(
            &contract.error.variants[1].payload,
            VariantPayload::Value(VariantValue { docs, deprecation, ty: CanonicalType::U32 })
                if docs == &["value"] && deprecation.as_deref() == Some("")
        ));
        assert!(matches!(
            &contract.error.variants[2].payload,
            VariantPayload::Named(fields)
                if fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>()
                    == ["message", "retryable"]
                    && fields[0].docs == ["message field"]
                    && fields[0].ty == CanonicalType::String
                    && fields[1].deprecation.as_deref() == Some("later")
                    && fields[1].ty == CanonicalType::Bool
        ));
        assert!(matches!(
            &contract.error.variants[3].payload,
            VariantPayload::Named(fields) if fields.is_empty()
        ));
        assert!(!contract.error.variants[3].payload.is_unit());
        assert_eq!(hex(&canonical_semantic_bytes(&contract)), MIXED_BYTES);
        assert_eq!(hex(&semantic_digest(&contract)), MIXED_DIGEST);

        for changed in [
            MIXED.replace("Empty {},", "Empty,"),
            MIXED.replace("retryable: bool", "retry_later: bool"),
            MIXED.replace("retryable: bool", "retryable: u8"),
            MIXED.replace("#[doc = \"message field\"] ", ""),
            MIXED.replace("#[deprecated(note = \"later\")] retryable", "retryable"),
            MIXED.replace("#[doc = \"value\"] ", ""),
            MIXED.replace("#[doc = \"value\"] #[deprecated] u32", "#[doc = \"value\"] u32"),
            MIXED.replace("message: String,\n                    #[deprecated(note = \"later\")] retryable: bool", "#[deprecated(note = \"later\")] retryable: bool,\n                    message: String"),
        ] {
            let changed = parse(changed.parse().unwrap()).unwrap();
            assert_ne!(semantic_digest(&contract), semantic_digest(&changed));
        }
    }

    #[test]
    fn every_scalar_leaf_is_accepted_in_value_and_named_payloads() {
        let leaves = [
            "bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64", "f32", "f64", "String",
            "Blob",
        ];
        let variants = leaves
            .iter()
            .enumerate()
            .map(|(index, leaf)| format!("Value{index}({leaf}), Named{index} {{ field: {leaf} }},"))
            .collect::<String>();
        let source = format!(
            "#[error] pub enum Fault {{ {variants} }} {CAP}",
            CAP = CAP.replace("GreetError", "Fault")
        );
        let contract = parse(source.parse().unwrap()).unwrap();
        for (index, leaf) in leaves.iter().enumerate() {
            let expected = match *leaf {
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
                _ => unreachable!(),
            };
            assert!(matches!(
                contract.error.variants[index * 2].payload,
                VariantPayload::Value(VariantValue { ty, .. }) if ty == expected
            ));
            assert!(matches!(
                contract.error.variants[index * 2 + 1].payload,
                VariantPayload::Named(ref fields) if fields[0].ty == expected
            ));
        }
    }

    #[test]
    fn payload_rejections_have_precise_spans_and_metadata_is_fail_closed() {
        let rejected = [
            (
                "Code(u32, bool)",
                "error variants must have exactly one unnamed field",
                "Code(u32, bool)",
            ),
            (
                "Unit = 1",
                "error variants must not have discriminants",
                "Unit = 1",
            ),
            (
                "Code(pub u32)",
                "error variant fields must not have visibility",
                "pub",
            ),
            (
                "Code(Vec<u8>)",
                "error variant field type must be a canonical scalar leaf",
                "Vec<u8>",
            ),
            (
                "Code(crate::u32)",
                "error variant field type must be a canonical scalar leaf",
                "crate::u32",
            ),
            (
                "Code(&u32)",
                "error variant field type must be a canonical scalar leaf",
                "&u32",
            ),
            (
                "Code(Vec::<u32>)",
                "error variant field type must be a canonical scalar leaf",
                "Vec::<u32>",
            ),
            (
                "Code((u32, bool))",
                "error variant field type must be a canonical scalar leaf",
                "(u32, bool)",
            ),
            (
                "Detail { message: Vec<u8> }",
                "error variant field type must be a canonical scalar leaf",
                "Vec<u8>",
            ),
            (
                "Detail { Café: u8, Cafe\u{301}: u16 }",
                "error variant field names must be unique",
                "Cafe\u{301}",
            ),
            (
                "Detail { #[serde] message: String }",
                "unknown contract metadata",
                "#[serde]",
            ),
            (
                "Detail { #[deprecated] #[deprecated] message: String }",
                "duplicate deprecated attribute",
                "#[deprecated]",
            ),
        ];
        for (variant, message, slice) in rejected {
            let source = format!(
                "#[error] pub enum Fault {{ {variant} }} {CAP}",
                CAP = CAP.replace("GreetError", "Fault")
            );
            let error = parse(source.parse().unwrap()).unwrap_err();
            assert_eq!(error.to_string(), message, "{variant}");
            assert_eq!(&source[error.span().byte_range()], slice, "{variant}");
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
