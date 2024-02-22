use const_format::formatcp;
use proc_macro2::{Ident, TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Meta, Result};

use crate::LANE_KIND;

pub(super) fn derive(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(_) => on_struct(input),
        Data::Enum(data) => on_enum(input, data),
        Data::Union(_) => Err(Error::new_spanned(
            input,
            "union as LaneKey is not supported",
        )),
    }
}

fn on_struct(input: &DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let kind = get_lane_kind(&input.attrs, input)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::LaneIndex for #name #type_generics #where_clause {
            fn index(&self) -> usize {
                0
            }
        }

        impl #impl_generics ::aeronet::LaneKey for #name #type_generics #where_clause {
            const VARIANTS: &'static [Self] = &[
                Self
            ];

            fn kind(&self) -> ::aeronet::LaneKind {
                #kind
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

            let kind = get_lane_kind(&variant.attrs, variant)?;
            Ok(Variant {
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
        impl #impl_generics ::aeronet::LaneIndex for #name #type_generics #where_clause {
            fn index(&self) -> usize {
                match *self {
                    #(#index_body),*
                }
            }
        }

        impl #impl_generics ::aeronet::LaneKey for #name #type_generics #where_clause {
            const VARIANTS: &'static [Self] = &[
                #(#all_variants),*
            ];

            fn kind(&self) -> ::aeronet::LaneKind {
                match *self {
                    #(#kind_body),*
                }
            }
        }
    })
}

// attributes

fn parse_lane_kind(attrs: &[Attribute]) -> Result<Option<TokenStream>> {
    let mut lane_kind = None;
    for attr in attrs {
        if !attr.path().is_ident(LANE_KIND) {
            continue;
        }

        if lane_kind.is_some() {
            return Err(Error::new_spanned(
                attr,
                formatcp!("duplicate #[{LANE_KIND}] attribute"),
            ));
        }

        let Meta::List(list) = &attr.meta else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing kind in #[{LANE_KIND}(kind)]"),
            ));
        };

        let Some(TokenTree::Ident(kind_ident)) = list.tokens.clone().into_iter().next() else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing kind in #[{LANE_KIND}(kind)]"),
            ));
        };

        lane_kind = Some(match kind_ident.to_string().as_str() {
            "UnreliableUnsequenced" => quote! { ::aeronet::LaneKind::UnreliableUnsequenced },
            "UnreliableSequenced" => quote! { ::aeronet::LaneKind::UnreliableSequenced },
            "ReliableUnordered" => quote! { ::aeronet::LaneKind::ReliableUnordered },
            "ReliableOrdered" => quote! { ::aeronet::LaneKind::ReliableOrdered },
            kind => {
                return Err(Error::new_spanned(
                    kind_ident,
                    format!("invalid lane kind `{kind}`"),
                ))
            }
        });
    }

    Ok(lane_kind)
}

fn get_lane_kind(attrs: &[Attribute], tokens: impl ToTokens) -> Result<TokenStream> {
    parse_lane_kind(attrs)?.ok_or(Error::new_spanned(
        tokens,
        formatcp!("missing #[{LANE_KIND}] attribute"),
    ))
}
