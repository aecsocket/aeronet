use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{Attribute, DeriveInput, Result};

use crate::{util, C2S, S2C};

pub(super) fn derive(input: &DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let c2s = parse_c2s(input, &input.attrs)?;
    let s2c = parse_s2c(input, &input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::protocol::TransportProtocol for #name #type_generics #where_clause {
            type C2S = #c2s;

            type S2C = #s2c;
        }
    })
}

// attributes

fn parse_c2s(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    util::require_attr_with_one_arg(C2S, tokens, attrs)
}

fn parse_s2c(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    util::require_attr_with_one_arg(S2C, tokens, attrs)
}
