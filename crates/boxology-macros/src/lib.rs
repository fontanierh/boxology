//! Compiler boundary for controlled Boxology contracts.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::quote;

/// Validates a controlled contract and exposes its generated public error type.
#[proc_macro]
pub fn contract(input: TokenStream) -> TokenStream {
    let model = match boxology_contract_syntax::parse(input.into()) {
        Ok(model) => model,
        Err(error) => return error.into_compile_error().into(),
    };
    let error = Ident::new(&model.error.name, Span::call_site());
    let expected = boxology_contract_syntax::semantic_digest(&model);
    let comparisons = expected.iter().enumerate().map(|(index, byte)| {
        quote!(::boxology_generated_contract::__BOXOLOGY_SEMANTIC_DIGEST[#index] == #byte)
    });
    quote! {
        pub use ::boxology_generated_contract::#error;
        const _: () = {
            if !(#(#comparisons)&&*) {
                panic!("Boxology generated contract is stale");
            }
        };
    }
    .into()
}
