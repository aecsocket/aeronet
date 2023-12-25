use const_format::formatcp;
use proc_macro2::{Ident, TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Meta, Result};

use crate::CHANNEL_KIND;

pub(super) fn derive(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(_) => on_struct(input),
        Data::Enum(data) => on_enum(input, data),
        Data::Union(_) => Err(Error::new_spanned(
            input,
            "union as ChannelKey is not supported",
        )),
    }
}

fn on_struct(input: &DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let channel_kind = parse_channel_kind(input, &input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::ChannelKey for #name #type_generics #where_clause {
            const ALL: &'static [Self] = &[
                Self
            ];

            fn index(&self) -> usize {
                0
            }

            fn kind(&self) -> ::aeronet::ChannelKind {
                #channel_kind
            }
        }
    })
}

fn on_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    struct Variant<'a> {
        ident: &'a Ident,
        kind: TokenStream,
    }

    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let variants = data
        .variants
        .iter()
        .map(|variant| {
            let Fields::Unit = variant.fields else {
                return Err(Error::new_spanned(
                    &variant.fields,
                    "variant must not have fields",
                ));
            };

            parse_channel_kind(variant, &variant.attrs).map(|kind| Variant {
                ident: &variant.ident,
                kind,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let all_variants = variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            quote! { Self::#pattern }
        })
        .collect::<Vec<_>>();
    let index_body = variants
        .iter()
        .enumerate()
        .map(|(index, variant)| {
            let pattern = variant.ident;
            quote! { Self::#pattern => #index }
        })
        .collect::<Vec<_>>();
    let kind_body = variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            let kind = &variant.kind;
            quote! { Self::#pattern => #kind }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #impl_generics ::aeronet::ChannelKey for #name #type_generics #where_clause {
            const ALL: &'static [Self] = &[
                #(#all_variants),*
            ];

            fn index(&self) -> usize {
                match *self {
                    #(#index_body),*
                }
            }

            fn kind(&self) -> ::aeronet::ChannelKind {
                match *self {
                    #(#kind_body),*
                }
            }
        }
    })
}

// attributes

fn parse_channel_kind(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<TokenStream> {
    let mut channel_kind = None;
    for attr in attrs {
        if !attr.path().is_ident(CHANNEL_KIND) {
            continue;
        }

        if channel_kind.is_some() {
            return Err(Error::new_spanned(
                attr,
                formatcp!("duplicate #[{CHANNEL_KIND}] attribute"),
            ));
        }

        let Meta::List(list) = &attr.meta else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing kind in #[{CHANNEL_KIND}(kind)]"),
            ));
        };

        let Some(TokenTree::Ident(kind_ident)) = list.tokens.clone().into_iter().next() else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing kind in #[{CHANNEL_KIND}(kind)]"),
            ));
        };

        channel_kind = Some(match kind_ident.to_string().as_str() {
            "Unreliable" => quote! { ::aeronet::ChannelKind::Unreliable },
            "ReliableUnordered" => quote! { ::aeronet::ChannelKind::ReliableUnordered },
            "ReliableOrdered" => quote! { ::aeronet::ChannelKind::ReliableOrdered },
            kind => {
                return Err(Error::new_spanned(
                    kind_ident,
                    format!("invalid channel kind `{kind}`"),
                ))
            }
        });
    }

    channel_kind.ok_or(Error::new_spanned(
        tokens,
        formatcp!("missing #[{CHANNEL_KIND}] attribute"),
    ))
}
