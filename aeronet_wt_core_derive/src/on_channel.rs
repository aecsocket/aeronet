use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Meta, Result};

pub fn derive(node: &DeriveInput) -> Result<TokenStream> {
    match &node.data {
        Data::Struct(_) => on_struct(node),
        Data::Enum(data) => on_enum(node, data),
        Data::Union(_) => Err(Error::new_spanned(
            node,
            "union as OnChannel is not supported",
        )),
    }
}

fn on_struct(node: &DeriveInput) -> Result<TokenStream> {
    let name = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let channel_type = channel_type(node, &node.attrs)?;
    let on_channel = on_channel(node, &node.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet_wt_core::OnChannel for #name #type_generics #where_clause {
            type Channel = #channel_type;

            fn channel(&self) -> #channel_type {
                #on_channel
            }
        }
    })
}

struct Variant<'a> {
    ident: &'a Ident,
    fields: &'a Fields,
    on_channel: &'a TokenStream,
}

fn on_enum(node: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    let name = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let channel_type = channel_type(node, &node.attrs)?;
    let on_channels = data
        .variants
        .iter()
        .map(|node| {
            on_channel(node, &node.attrs).map(|on_channel| Variant {
                ident: &node.ident,
                fields: &node.fields,
                on_channel,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let match_body = match_body(&on_channels);

    Ok(quote! {
        impl #impl_generics ::aeronet_wt_core::OnChannel for #name #type_generics #where_clause {
            type Channel = #channel_type;

            fn channel(&self) -> #channel_type {
                match self {
                    #(#match_body),*
                }
            }
        }
    })
}

const CHANNEL_TYPE: &str = "channel_type";

fn channel_type(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    let mut channel_type = None;
    for attr in attrs {
        if attr.path().is_ident(CHANNEL_TYPE) {
            if channel_type.is_some() {
                return Err(Error::new_spanned(
                    attr,
                    "duplicate #[channel_type] attribute",
                ));
            }
            let Meta::List(list) = &attr.meta else {
                return Err(Error::new_spanned(
                    attr,
                    "missing type in #[channel_type(type)]",
                ));
            };
            channel_type = Some(&list.tokens);
        }
    }
    channel_type.ok_or(Error::new_spanned(
        tokens,
        "missing #[channel_type] attribute",
    ))
}

const ON_CHANNEL: &str = "on_channel";

fn on_channel(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    let mut on_channel = None;
    for attr in attrs {
        if attr.path().is_ident(ON_CHANNEL) {
            if on_channel.is_some() {
                return Err(Error::new_spanned(
                    attr,
                    "duplicate #[on_channel] attribute",
                ));
            }
            let Meta::List(list) = &attr.meta else {
                return Err(Error::new_spanned(
                    attr,
                    "missing value in #[on_channel(value)]",
                ));
            };
            on_channel = Some(&list.tokens);
        }
    }
    on_channel.ok_or(Error::new_spanned(
        tokens,
        "missing #[on_channel] attribute",
    ))
}

fn match_body(variants: &Vec<Variant<'_>>) -> Vec<TokenStream> {
    variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            let destruct = match variant.fields {
                Fields::Unit => quote! {},
                Fields::Named(_) => quote! { { .. } },
                Fields::Unnamed(_) => quote! { (..) },
            };
            let on_channel = variant.on_channel;
            quote! {
                Self::#pattern #destruct => #on_channel
            }
        })
        .collect()
}
