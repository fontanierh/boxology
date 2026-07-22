//! Shared parser and owned model for controlled Boxology contract tokens.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use proc_macro2::TokenStream;
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
    /// The exported capability declaration.
    pub capability: CapabilityDeclaration,
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
    /// Directly named in-block error type.
    pub error: String,
    /// Declared maximum exposure.
    pub exposure: &'static str,
    /// Declared idempotency, defaulting to none.
    pub idempotency: &'static str,
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
        let attrs = Attribute::parse_outer(input)?;
        let capability = parse_capability(&attrs, input)?;
        if !input.is_empty() {
            return Err(input.error("a contract supports exactly two declarations"));
        }
        if capability.error != error.name {
            return Err(
                input.error("capability error must directly name an in-block #[error] enum")
            );
        }
        Ok(Self { error, capability })
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
    if !is_ident(&arg.ty, "String") {
        return Err(error(&arg.ty, "input type must be String"));
    }
    let Some(error_name) = result_error(&output) else {
        return Err(error(
            &output,
            "output must be unqualified Result<String, Error>",
        ));
    };
    Ok(CapabilityDeclaration {
        docs,
        deprecation,
        name,
        input_name: identifier(&input_ident.ident)?,
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
fn is_ident(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.qself.is_none() && path.path.get_ident().is_some_and(|id| id==name))
}
fn result_error(ty: &Type) -> Option<String> {
    let Type::Path(result) = ty else {
        return None;
    };
    let segment = result.path.segments.first()?;
    if result.qself.is_some()
        || result.path.leading_colon.is_some()
        || result.path.segments.len() != 1
        || segment.ident != "Result"
    {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let mut values = args.args.iter();
    let (
        Some(syn::GenericArgument::Type(ok)),
        Some(syn::GenericArgument::Type(Type::Path(error))),
        None,
    ) = (values.next(), values.next(), values.next())
    else {
        return None;
    };
    let name = error.path.get_ident()?;
    (error.qself.is_none() && is_ident(ok, "String"))
        .then(|| identifier(name).ok())
        .flatten()
}
fn error(node: &impl Spanned, message: &str) -> syn::Error {
    syn::Error::new(node.span(), message)
}
fn identifier(ident: &syn::Ident) -> syn::Result<String> {
    let value = ident.to_string();
    if value.starts_with("r#") {
        return Err(error(ident, "contract identifiers must not be raw"));
    }
    Ok(value)
}
fn capability_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ERROR: &str = "#[error] pub enum GreetError { EmptyName }";
    const CAP: &str = "#[capability(exposure=external)] pub async fn greet(name:String)->Result<String,GreetError>;";
    #[test]
    fn hello_parses_to_owned_semantics() {
        fn traits<T: Send + Sync + 'static>() {}
        traits::<Contract>();
        let contract = parse(format!("{ERROR} {CAP}").parse().unwrap()).unwrap();
        assert_eq!(contract.error.name, "GreetError");
        assert_eq!(contract.error.variants[0].name, "EmptyName");
        assert_eq!(contract.capability.name, "greet");
        assert_eq!(contract.capability.input_name, "name");
        assert_eq!(contract.capability.error, "GreetError");
        assert_eq!(contract.capability.exposure, "external");
        assert_eq!(contract.capability.idempotency, "none");
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
    }
}
