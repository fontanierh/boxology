//! Shared parser and owned model for controlled Boxology contract tokens.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use boxology_contract::{ExposureLevel, Idempotency, canonicalize_ordinary_rust_identifier};
use proc_macro2::TokenStream;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use syn::{
    Attribute, Expr, FnArg, ItemEnum, Lit, Meta, ReturnType, Token, Type, Visibility, parse::Parse,
    parse::ParseStream, spanned::Spanned,
};
/// A controlled contract block, independent of source spelling and location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Contract {
    /// Local structured data declarations in source order.
    pub data: Vec<DataDeclaration>,
    /// The domain-error declaration.
    pub error: ErrorDeclaration,
    /// The exported capability declarations in source order; always at least one.
    pub capabilities: Vec<CapabilityDeclaration>,
}
/// One local structured declaration and its source-ordered members.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataDeclaration {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note.
    pub deprecation: Option<String>,
    /// Ordinary Rust identifier.
    pub name: String,
    /// Struct or unit-enum shape.
    pub shape: DataShape,
}
/// The supported local structured declaration shapes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataShape {
    /// A named-field struct; the ordered field list may be empty.
    Struct(Vec<DataField>),
    /// A nonempty unit-only enum.
    Enum(Vec<DataVariant>),
}
/// One public named struct field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataField {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note.
    pub deprecation: Option<String>,
    /// Ordinary Rust identifier.
    pub name: String,
    /// Validated boundary type expression.
    pub ty: TypeExpression,
}
/// One unit enum variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataVariant {
    /// Decoded documentation lines in source order.
    pub docs: Vec<String>,
    /// Optional decoded deprecation note.
    pub deprecation: Option<String>,
    /// Ordinary Rust identifier.
    pub name: String,
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
    /// Capability name.
    pub name: String,
    /// Input name.
    pub input_name: String,
    /// Canonical boundary type accepted as the single input.
    pub input_type: TypeExpression,
    /// Canonical boundary type produced on success.
    pub output_type: TypeExpression,
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
/// One validated, owned boundary type expression.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeExpression {
    /// An existing canonical scalar leaf.
    Leaf(CanonicalType),
    /// An earlier local structured declaration.
    Local(String),
    /// Optional presence around a base or vector expression.
    Option(Box<TypeExpression>),
    /// A list around a base expression.
    Vec(Box<TypeExpression>),
}
impl TypeExpression {
    /// Returns the scalar when this expression is exactly one leaf.
    pub fn leaf(&self) -> Option<CanonicalType> {
        match self {
            Self::Leaf(value) => Some(*value),
            _ => None,
        }
    }
    /// Returns whether `Blob` occurs anywhere in the expression.
    pub fn contains_blob(&self) -> bool {
        match self {
            Self::Leaf(value) => value.is_blob(),
            Self::Local(_) => false,
            Self::Option(inner) | Self::Vec(inner) => inner.contains_blob(),
        }
    }
    /// Returns the canonical Rust-like spelling used by semantic encoding.
    pub fn canonical_spelling(&self) -> String {
        match self {
            Self::Leaf(value) => value.canonical_name().into(),
            Self::Local(name) => name.clone(),
            Self::Option(inner) => format!("Option<{}>", inner.canonical_spelling()),
            Self::Vec(inner) => format!("Vec<{}>", inner.canonical_spelling()),
        }
    }
}
impl From<CanonicalType> for TypeExpression {
    fn from(value: CanonicalType) -> Self {
        Self::Leaf(value)
    }
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
    count(
        &mut out,
        contract.data.len() + 1 + contract.capabilities.len(),
    );
    for declaration in &contract.data {
        out.push(match declaration.shape {
            DataShape::Struct(_) => 3,
            DataShape::Enum(_) => 4,
        });
        encode_metadata(&mut out, &declaration.docs, &declaration.deprecation);
        string(&mut out, &declaration.name);
        match &declaration.shape {
            DataShape::Struct(fields) => {
                count(&mut out, fields.len());
                for field in fields {
                    encode_metadata(&mut out, &field.docs, &field.deprecation);
                    string(&mut out, &field.name);
                    string(&mut out, &field.ty.canonical_spelling());
                }
            }
            DataShape::Enum(variants) => {
                count(&mut out, variants.len());
                for variant in variants {
                    encode_metadata(&mut out, &variant.docs, &variant.deprecation);
                    string(&mut out, &variant.name);
                }
            }
        }
    }
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
            &capability.input_type.canonical_spelling(),
            &capability.output_type.canonical_spelling(),
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
        let mut data = Vec::new();
        let mut data_names = BTreeSet::new();
        let error = loop {
            if input.is_empty() {
                return Err(input
                    .error("a contract requires one #[error] enum and at least one capability"));
            }
            let attrs = Attribute::parse_outer(input)?;
            if attrs.iter().any(|attr| attr.path().is_ident("error")) {
                let item: ItemEnum = input.parse()?;
                let error = parse_error(&attrs, &item)?;
                if data_names.contains(&error.name) {
                    return Err(error_at(
                        &item.ident,
                        "contract data and error names must be unique",
                    ));
                }
                break error;
            }
            let ahead = input.fork();
            let _: Visibility = ahead.parse()?;
            let declaration = if ahead.peek(Token![struct]) {
                parse_data_struct(&attrs, &input.parse()?, &data_names)?
            } else if ahead.peek(Token![enum]) {
                parse_data_enum(&attrs, &input.parse()?, &data_names)?
            } else {
                return Err(input.error("expected a local data declaration or #[error] enum"));
            };
            debug_assert!(data_names.insert(declaration.name.clone()));
            data.push(declaration);
        };
        let mut capabilities = Vec::new();
        let mut names = BTreeSet::new();
        while !input.is_empty() {
            let attrs = Attribute::parse_outer(input)?;
            let capability = parse_capability(&attrs, input, &data_names)?;
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
            data,
            error,
            capabilities,
        })
    }
}

fn parse_data_struct(
    attrs: &[Attribute],
    item: &syn::ItemStruct,
    locals: &BTreeSet<String>,
) -> syn::Result<DataDeclaration> {
    let (docs, deprecation, marker) = metadata(attrs, "")?;
    if marker.is_some()
        || !matches!(item.vis, Visibility::Public(_))
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err(error_at(
            item,
            "local structs must be public and non-generic",
        ));
    }
    let fields = match &item.fields {
        syn::Fields::Named(fields) => fields,
        syn::Fields::Unnamed(_) => {
            return Err(error_at(
                &item.fields,
                "local structs must use named fields",
            ));
        }
        syn::Fields::Unit => {
            return Err(error_at(&item.ident, "local structs must use named fields"));
        }
    };
    let mut names = BTreeSet::new();
    let mut output = Vec::with_capacity(fields.named.len());
    for field in &fields.named {
        if !matches!(field.vis, Visibility::Public(_)) {
            return Err(error_at(
                field.ident.as_ref().expect("named fields have identifiers"),
                "local struct fields must be public",
            ));
        }
        let (docs, deprecation, _) = metadata(&field.attrs, "")?;
        let ident = field.ident.as_ref().expect("named fields have identifiers");
        let name = identifier(ident)?;
        if !names.insert(name.clone()) {
            return Err(error_at(ident, "local struct field names must be unique"));
        }
        output.push(DataField {
            docs,
            deprecation,
            name,
            ty: type_expression(&field.ty, locals)?,
        });
    }
    let name = data_name(&item.ident, locals)?;
    Ok(DataDeclaration {
        docs,
        deprecation,
        name,
        shape: DataShape::Struct(output),
    })
}

fn parse_data_enum(
    attrs: &[Attribute],
    item: &ItemEnum,
    locals: &BTreeSet<String>,
) -> syn::Result<DataDeclaration> {
    let (docs, deprecation, marker) = metadata(attrs, "")?;
    if marker.is_some()
        || !matches!(item.vis, Visibility::Public(_))
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
        || item.variants.is_empty()
    {
        return Err(error_at(
            item,
            "local enums must be public, non-generic, and nonempty",
        ));
    }
    let mut names = BTreeSet::new();
    let mut output = Vec::with_capacity(item.variants.len());
    for variant in &item.variants {
        if !matches!(variant.fields, syn::Fields::Unit) || variant.discriminant.is_some() {
            return Err(error_at(
                variant,
                "local enum variants must be unit-only without discriminants",
            ));
        }
        let (docs, deprecation, _) = metadata(&variant.attrs, "")?;
        let name = identifier(&variant.ident)?;
        if name == "Unknown" {
            return Err(error_at(
                &variant.ident,
                "local enum variant name `Unknown` is reserved",
            ));
        }
        if !names.insert(name.clone()) {
            return Err(error_at(
                &variant.ident,
                "local enum variant names must be unique",
            ));
        }
        output.push(DataVariant {
            docs,
            deprecation,
            name,
        });
    }
    let name = data_name(&item.ident, locals)?;
    Ok(DataDeclaration {
        docs,
        deprecation,
        name,
        shape: DataShape::Enum(output),
    })
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

#[rustfmt::skip]
fn data_name(ident: &syn::Ident, locals: &BTreeSet<String>) -> syn::Result<String> {
    let name = identifier(ident)?;
    if locals.contains(&name) {
        return Err(error_at(ident, "local data declaration names must be unique"));
    }
    if matches!(name.as_str(), "bool" | "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "f32" | "f64" | "String" | "Blob") {
        return Err(error_at(ident, "local data declaration names must not shadow canonical leaves"));
    }
    Ok(name)
}

fn parse_capability(
    attrs: &[Attribute],
    input: ParseStream<'_>,
    locals: &BTreeSet<String>,
) -> syn::Result<CapabilityDeclaration> {
    let (docs, deprecation, marker) = metadata(attrs, "capability")?;
    let Some(marker) = marker else {
        return Err(input.error("capability declaration requires #[capability]"));
    };
    let (exposure, idempotency) = parse_capability_metadata(marker)?;
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
    let input_type = type_expression(&arg.ty, locals)?;
    let Some((output_type, error_name)) = result_error(&output, locals)? else {
        return Err(error(
            &output,
            "output must be unqualified Result<Type, Error>",
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
        exposure,
        idempotency,
    })
}

fn parse_capability_metadata(attr: &Attribute) -> syn::Result<(ExposureLevel, Idempotency)> {
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
        } else if name == marker_name {
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
fn type_expression(ty: &Type, locals: &BTreeSet<String>) -> syn::Result<TypeExpression> {
    fn base(ty: &Type, locals: &BTreeSet<String>) -> syn::Result<TypeExpression> {
        if let Some(value) = leaf(ty) {
            return Ok(value.into());
        }
        let Type::Path(path) = ty else {
            return Err(error_at(
                ty,
                "boundary type must use the controlled type-expression grammar",
            ));
        };
        let Some(ident) = path.path.get_ident().filter(|_| path.qself.is_none()) else {
            return Err(error_at(
                ty,
                "boundary type must be an unqualified scalar or earlier local declaration",
            ));
        };
        let name = identifier(ident)?;
        if !locals.contains(&name) {
            return Err(error_at(
                ty,
                "boundary type must name a scalar or earlier local declaration",
            ));
        }
        Ok(TypeExpression::Local(name))
    }
    if let Ok(value) = base(ty, locals) {
        return Ok(value);
    }
    let Type::Path(path) = ty else {
        return Err(error_at(ty, "unsupported boundary type expression"));
    };
    let Some(segment) = path.path.segments.first() else {
        return Err(error_at(ty, "unsupported boundary type expression"));
    };
    if path.qself.is_some() || path.path.leading_colon.is_some() || path.path.segments.len() != 1 {
        return Err(error_at(ty, "boundary type wrappers must be unqualified"));
    }
    let wrapper = identifier(&segment.ident)?;
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(error_at(
            ty,
            "boundary wrapper requires exactly one type argument",
        ));
    };
    if arguments.colon2_token.is_some() || arguments.args.len() != 1 {
        return Err(error_at(
            ty,
            "boundary wrapper requires exactly one type argument",
        ));
    }
    let Some(syn::GenericArgument::Type(inner)) = arguments.args.first() else {
        return Err(error_at(
            ty,
            "boundary wrapper requires exactly one type argument",
        ));
    };
    match wrapper.as_str() {
        "Vec" => Ok(TypeExpression::Vec(Box::new(base(inner, locals)?))),
        "Option" => {
            let value = if let Type::Path(path) = inner
                && path.qself.is_none()
                && path.path.leading_colon.is_none()
                && path.path.segments.len() == 1
                && path.path.segments[0].ident == "Vec"
            {
                let segment = &path.path.segments[0];
                let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                    return Err(error_at(inner, "Vec requires exactly one base type"));
                };
                if arguments.colon2_token.is_some() || arguments.args.len() != 1 {
                    return Err(error_at(inner, "Vec requires exactly one base type"));
                }
                let Some(syn::GenericArgument::Type(base_ty)) = arguments.args.first() else {
                    return Err(error_at(inner, "Vec requires exactly one base type"));
                };
                TypeExpression::Vec(Box::new(base(base_ty, locals)?))
            } else {
                base(inner, locals)?
            };
            Ok(TypeExpression::Option(Box::new(value)))
        }
        _ => Err(error_at(
            ty,
            "only Option and Vec are supported boundary wrappers",
        )),
    }
}
fn result_error(
    ty: &Type,
    locals: &BTreeSet<String>,
) -> syn::Result<Option<(TypeExpression, String)>> {
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
    let output_type = type_expression(ok, locals)?;
    if error.qself.is_some() {
        return Ok(None);
    }
    Ok(Some((output_type, identifier(name)?)))
}
fn error(node: &impl Spanned, message: &str) -> syn::Error {
    syn::Error::new(node.span(), message)
}
fn error_at(node: &impl Spanned, message: &str) -> syn::Error {
    error(node, message)
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
    const STRUCTURED: &str = r#"
        pub struct Empty {}
        pub enum Mood { Calm, Busy }
        pub struct Profile {
            pub name: String,
            pub scores: Vec<u32>,
            pub mood: Option<Mood>,
            pub history: Option<Vec<Mood>>,
        }
        #[error] pub enum Fault { Bad }
        #[capability] pub async fn save(input: Profile)->Result<Option<Vec<Profile>>,Fault>;
    "#;
    #[test]
    #[rustfmt::skip]
    fn structured_subset_is_owned_ordered_and_semantically_pinned() {
        let contract = parse(STRUCTURED.parse().unwrap()).unwrap();
        assert_eq!(contract.data.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["Empty", "Mood", "Profile"]);
        let DataShape::Struct(fields) = &contract.data[2].shape else { panic!("profile shape") };
        assert_eq!(fields.iter().map(|field| (&*field.name, field.ty.canonical_spelling())).collect::<Vec<_>>(), [("name", "String".into()), ("scores", "Vec<u32>".into()), ("mood", "Option<Mood>".into()), ("history", "Option<Vec<Mood>>".into())]);
        assert_eq!(contract.capabilities[0].input_type, TypeExpression::Local("Profile".into()));
        assert_eq!(contract.capabilities[0].output_type.canonical_spelling(), "Option<Vec<Profile>>");
        assert_eq!(hex(&canonical_semantic_bytes(&contract)), "626f786f6c6f67792e636f6e74726163742d73656d616e7469637300000000010000000000000005030000000000000000000000000000000005456d70747900000000000000000400000000000000000000000000000000044d6f6f640000000000000002000000000000000000000000000000000443616c6d00000000000000000000000000000000044275737903000000000000000000000000000000000750726f66696c65000000000000000400000000000000000000000000000000046e616d650000000000000006537472696e67000000000000000000000000000000000673636f72657300000000000000085665633c7533323e00000000000000000000000000000000046d6f6f64000000000000000c4f7074696f6e3c4d6f6f643e0000000000000000000000000000000007686973746f727900000000000000114f7074696f6e3c5665633c4d6f6f643e3e0100000000000000000000000000000000054661756c740000000000000001000000000000000000000000000000000003426164020000000000000000000000000000000004736176650000000000000005696e707574000000000000000750726f66696c6500000000000000144f7074696f6e3c5665633c50726f66696c653e3e00000000000000054661756c740000000000000009636f64655f6f6e6c7900000000000000046e6f6e65");
        assert_eq!(hex(&semantic_digest(&contract)), "ed88106d788c4813320fa9ce00584a95bbc2ffd79385a83713954fd242c1c111");
    }
    #[test]
    fn capability_metadata_accept_matrix() {
        #[rustfmt::skip]
        let cases = [
            ("exposure=code_only", ExposureLevel::CodeOnly, Idempotency::None),
            ("exposure=internal", ExposureLevel::Internal, Idempotency::None),
            ("exposure=external", ExposureLevel::External, Idempotency::None),
            ("idempotency=none", ExposureLevel::CodeOnly, Idempotency::None),
            ("idempotency=inherent", ExposureLevel::CodeOnly, Idempotency::Inherent),
            ("exposure=code_only,idempotency=none", ExposureLevel::CodeOnly, Idempotency::None),
            ("exposure=code_only,idempotency=inherent", ExposureLevel::CodeOnly, Idempotency::Inherent),
            ("exposure=internal,idempotency=none", ExposureLevel::Internal, Idempotency::None),
            ("exposure=internal,idempotency=inherent", ExposureLevel::Internal, Idempotency::Inherent),
            ("exposure=external,idempotency=none", ExposureLevel::External, Idempotency::None),
            ("exposure=external,idempotency=inherent", ExposureLevel::External, Idempotency::Inherent),
            ("idempotency=inherent,exposure=internal", ExposureLevel::Internal, Idempotency::Inherent),
            ("exposure=external,", ExposureLevel::External, Idempotency::None),
            ("", ExposureLevel::CodeOnly, Idempotency::None),
            // Bare `#[capability]` is Meta::Path; empty args above is Meta::List([]).
            ("#", ExposureLevel::CodeOnly, Idempotency::None),
        ];
        for (args, exposure, idempotency) in cases {
            let marker = match args {
                "#" => "#[capability]".to_owned(),
                "" => "#[capability()]".to_owned(),
                _ => format!("#[capability({args})]"),
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
    fn capability_metadata_rejections_have_precise_spans() {
        let rejected = [
            (
                "#[capability(unknown=external)]",
                "unknown capability metadata",
                "unknown",
            ),
            (
                "#[capability(name=\"greet\")]",
                "unknown capability metadata",
                "name",
            ),
            (
                "#[capability(exposure=external,exposure=internal)]",
                "duplicate capability metadata",
                "exposure",
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
    }
    #[test]
    fn non_default_metadata_semantic_encoding_is_pinned() {
        let contract = parse(
            "#[error] pub enum E { V } #[capability(exposure=internal,idempotency=inherent)] pub async fn rescued(x:String)->Result<String,E>;"
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
        // `{CAP} {CAP} {ERROR}` above fails at parse_error (first item is not `#[error]`).
        // `capability_error_mismatch_and_duplicate_names_fail_closed` already reaches the
        // uniqueness pass and proves it errors; this row pins which message it errors with.
        let duplicate_wire = format!("{ERROR} {CAP} {CAP}");
        let error = parse(duplicate_wire.parse().unwrap()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("capability names must be unique"),
            "{error}"
        );
        for boundary in [
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
    fn unsupported_nested_input_type_has_a_stable_diagnostic() {
        let source = format!(
            "{ERROR} {}",
            CAP.replace("name:String", "name:Vec<Option<u8>>")
        );
        let diagnostic = parse(source.parse().unwrap()).unwrap_err();
        assert_eq!(
            diagnostic.to_string(),
            "boundary type must be an unqualified scalar or earlier local declaration"
        );
    }
    #[test]
    fn unsupported_nested_output_type_has_a_stable_diagnostic() {
        let source = format!(
            "{ERROR} {}",
            CAP.replace("Result<String", "Result<Vec<Option<u8>>")
        );
        let diagnostic = parse(source.parse().unwrap()).unwrap_err();
        assert_eq!(
            diagnostic.to_string(),
            "boundary type must be an unqualified scalar or earlier local declaration"
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
            assert_eq!(input.capabilities[0].input_type.canonical_spelling(), name);
            assert_eq!(
                input.capabilities[0].output_type.canonical_spelling(),
                "String"
            );
            let output = parse(
                format!(
                    "{ERROR} {}",
                    CAP.replace("Result<String", &format!("Result<{name}"))
                )
                .parse()
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                output.capabilities[0].output_type.canonical_spelling(),
                name
            );
            assert_eq!(
                output.capabilities[0].input_type.canonical_spelling(),
                "String"
            );
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
        assert_eq!(
            contract.capabilities[0].input_type,
            CanonicalType::U32.into()
        );
        assert_eq!(
            contract.capabilities[0].output_type,
            CanonicalType::Bool.into()
        );
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
        assert_eq!(
            contract.capabilities[0].output_type,
            CanonicalType::String.into()
        );
        assert_eq!(
            contract.capabilities[1].output_type,
            CanonicalType::Bool.into()
        );
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
            "contract identifiers must be ordinary non-raw Rust identifiers"
        );
        assert_eq!(&source[diagnostic.span().byte_range()], "gen");
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
