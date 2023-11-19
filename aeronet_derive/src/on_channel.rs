use const_format::formatcp;
use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Meta, Result};

use crate::{CHANNEL_TYPE, ON_CHANNEL};

pub(super) fn derive(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(_) => on_struct(input),
        Data::Enum(data) => on_enum(input, data),
        Data::Union(_) => Err(Error::new_spanned(
            input,
            "union as OnChannel is not supported",
        )),
    }
}

fn on_struct(input: &DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let channel_type = parse_channel_type(input, &input.attrs)?;
    let on_channel = parse_on_channel(input, &input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::OnChannel for #name #type_generics #where_clause {
            type Channel = #channel_type;

            fn channel(&self) -> Self::Channel {
                #on_channel
            }
        }
    })
}

fn on_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    struct Variant<'a> {
        ident: &'a Ident,
        fields: &'a Fields,
        on_channel: &'a TokenStream,
    }

    let channel_type = parse_channel_type(input, &input.attrs)?;
    let variants = data
        .variants
        .iter()
        .map(|variant| {
            parse_on_channel(variant, &variant.attrs).map(|on_channel| Variant {
                ident: &variant.ident,
                fields: &variant.fields,
                on_channel,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let match_body = variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            let destruct = match variant.fields {
                Fields::Unit => quote! {},
                Fields::Named(_) => quote! { { .. } },
                Fields::Unnamed(_) => quote! { (..) },
            };
            let on_channel = variant.on_channel;
            quote! { Self::#pattern #destruct => #on_channel }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #impl_generics ::aeronet::OnChannel for #name #type_generics #where_clause {
            type Channel = #channel_type;

            fn channel(&self) -> Self::Channel {
                match *self {
                    #(#match_body),*
                }
            }
        }
    })
}

// attributes

fn parse_channel_type(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    let mut channel_type = None;
    for attr in attrs {
        if !attr.path().is_ident(CHANNEL_TYPE) {
            continue;
        }

        if channel_type.is_some() {
            return Err(Error::new_spanned(
                attr,
                formatcp!("duplicate #[{CHANNEL_TYPE}] attribute"),
            ));
        }

        let Meta::List(list) = &attr.meta else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing type in #[{CHANNEL_TYPE}(type)]"),
            ));
        };

        channel_type = Some(&list.tokens);
    }

    channel_type.ok_or(Error::new_spanned(
        tokens,
        formatcp!("missing #[{CHANNEL_TYPE}] attribute"),
    ))
}

fn parse_on_channel(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    let mut on_channel = None;
    for attr in attrs {
        if !attr.path().is_ident(ON_CHANNEL) {
            continue;
        }

        if on_channel.is_some() {
            return Err(Error::new_spanned(
                attr,
                formatcp!("duplicate #[{ON_CHANNEL}] attribute"),
            ));
        }

        let Meta::List(list) = &attr.meta else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing value in #[{ON_CHANNEL}(value)]"),
            ));
        };

        on_channel = Some(&list.tokens);
    }

    on_channel.ok_or(Error::new_spanned(
        tokens,
        formatcp!("missing #[{ON_CHANNEL}] attribute"),
    ))
}
