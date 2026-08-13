//! Compiler boundary for controlled Boxology contracts.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, ReturnType, Type, visit::Visit};

/// Validates a controlled contract and exposes its generated public boundary types.
#[proc_macro]
pub fn contract(input: TokenStream) -> TokenStream {
    contract_expansion(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn contract_expansion(input: TokenStream2) -> syn::Result<TokenStream2> {
    let model = boxology_contract_syntax::parse(input)?;
    let dependency = Ident::new(&model.dependency_crate, Span::call_site());
    let data = model
        .data
        .iter()
        .map(|declaration| Ident::new(&declaration.name, Span::call_site()))
        .collect::<Vec<_>>();
    let error = Ident::new(&model.error.name, Span::call_site());
    let facade = if data.is_empty() {
        quote!(pub use ::#dependency::#error;)
    } else {
        quote!(pub use ::#dependency::{#(#data,)* #error};)
    };
    let expected = boxology_contract_syntax::semantic_digest(&model);
    let comparisons = expected
        .iter()
        .enumerate()
        .map(|(index, byte)| quote!(::#dependency::__BOXOLOGY_SEMANTIC_DIGEST[#index] == #byte));
    Ok(quote! {
        #facade
        #[doc(hidden)]
        #[macro_export]
        macro_rules! __boxology_check_local_implementation {
            ($receiver:ty; $($methods:tt)*) => {
                ::#dependency::__boxology_check_implementation!($receiver; $($methods)*);
            };
        }
        const _: () = {
            if !(#(#comparisons)&&*) {
                panic!("Boxology generated contract is stale");
            }
        };
    })
}

/// Preserves one ordinary inherent implementation and asks generated glue to check its contract.
#[proc_macro_attribute]
pub fn implementation(arguments: TokenStream, input: TokenStream) -> TokenStream {
    let original = TokenStream2::from(input.clone());
    if !arguments.is_empty() {
        return append_error(
            original,
            syn::Error::new(Span::call_site(), "implementation accepts no arguments"),
        );
    }
    let item = match syn::parse2::<ItemImpl>(original.clone()) {
        Ok(item) => item,
        Err(error) => return append_error(original, error),
    };
    let error = if item.trait_.is_some() {
        Some(syn::Error::new_spanned(
            &item.self_ty,
            "implementation requires an inherent impl",
        ))
    } else if !item.generics.params.is_empty() {
        Some(syn::Error::new_spanned(
            &item.generics,
            "implementation impl cannot be generic",
        ))
    } else if item.generics.where_clause.is_some() {
        Some(syn::Error::new_spanned(
            &item.generics,
            "implementation impl cannot have a where clause",
        ))
    } else {
        None
    };
    if let Some(error) = error {
        return append_error(original, error);
    }
    let receiver = &item.self_ty;
    let methods = item.items.iter().filter_map(|member| {
        let ImplItem::Fn(method) = member else {
            return None;
        };
        let name = &method.sig.ident;
        let validity = Ident::new(
            if valid_signature(method) {
                "valid"
            } else {
                "invalid"
            },
            name.span(),
        );
        Some(quote!(#name #validity;))
    });
    quote! {
        #original
        __boxology_check_local_implementation!(#receiver; #(#methods)*);
    }
    .into()
}

fn append_error(original: TokenStream2, error: syn::Error) -> TokenStream {
    let error = error.into_compile_error();
    quote!(#original #error).into()
}

fn valid_signature(method: &syn::ImplItemFn) -> bool {
    let signature = &method.sig;
    let inputs = signature.inputs.iter().collect::<Vec<_>>();
    let receiver = matches!(inputs.first(), Some(FnArg::Receiver(receiver))
        if receiver.mutability.is_none()
            && matches!(receiver.kind, syn::ReceiverKind::Reference(_, None, None)));
    let typed_tail = inputs[1.min(inputs.len())..]
        .iter()
        .all(|input| matches!(input, FnArg::Typed(_)));
    let mut finder = ImplTraitFinder(false);
    finder.visit_signature(signature);
    signature.asyncness.is_some()
        && signature.constness.is_none()
        && !matches!(signature.safety, syn::Safety::Unsafe(_))
        && signature.abi.is_none()
        && signature.variadic.is_none()
        && signature.generics.params.is_empty()
        && signature.generics.where_clause.is_none()
        && inputs.len() == 3
        && receiver
        && typed_tail
        && matches!(signature.output, ReturnType::Type(..))
        && !finder.0
}

struct ImplTraitFinder(bool);

impl<'ast> Visit<'ast> for ImplTraitFinder {
    fn visit_type(&mut self, node: &'ast Type) {
        if matches!(node, Type::ImplTrait(_)) {
            self.0 = true;
        }
        syn::visit::visit_type(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_facade_reexports_structured_types_in_declaration_order() {
        let expanded = contract_expansion(quote! {
            pub struct Empty {}
            pub enum Mood { Calm }
            pub struct Profile { pub mood: Mood }
            #[error] pub enum Fault { Bad }
            #[capability] pub async fn save(input: Profile) -> Result<Profile, Fault>;
        })
        .unwrap()
        .to_string();
        let positions = ["Empty", "Mood", "Profile", "Fault"].map(|name| {
            expanded
                .find(name)
                .unwrap_or_else(|| panic!("missing facade name {name}: {expanded}"))
        });
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(expanded.matches("pub use").count(), 1);

        let scalar = contract_expansion(quote! {
            #[error] pub enum Fault { Bad }
            #[capability] pub async fn save(input: String) -> Result<String, Fault>;
        })
        .unwrap()
        .to_string();
        assert!(scalar.contains("pub use :: boxology_generated_contract :: Fault ;"));
    }

    #[test]
    fn contract_facade_accepts_a_project_local_dependency_name() {
        let expanded = contract_expansion(quote! {
            contract_crate = review_contract;
            #[error] pub enum Fault { Bad }
            #[capability] pub async fn save(input: String) -> Result<String, Fault>;
        })
        .unwrap()
        .to_string();

        assert!(expanded.contains("pub use :: review_contract :: Fault"));
        assert!(expanded.contains(":: review_contract :: __boxology_check_implementation"));
        assert!(!expanded.contains(":: boxology_generated_contract"));
    }
}
