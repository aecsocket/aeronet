use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

pub(super) fn derive(input: &DeriveInput) -> TokenStream {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics ::aeronet::message::Message for #name #type_generics #where_clause {}
    }
}
