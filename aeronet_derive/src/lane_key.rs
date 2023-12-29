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

    let lane_kind = parse_lane_kind(input, &input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::LaneKey for #name #type_generics #where_clause {
            const VARIANTS: &'static [Self] = &[
                Self
            ];

            fn variant(&self) -> usize {
                0
            }

            fn kind(&self) -> ::aeronet::LaneKind {
                #lane_kind
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

            parse_lane_kind(variant, &variant.attrs).map(|kind| Variant {
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
    let variant_body = variants
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
        impl #impl_generics ::aeronet::LaneKey for #name #type_generics #where_clause {
            const VARIANTS: &'static [Self] = &[
                #(#all_variants),*
            ];

            fn variant(&self) -> usize {
                match *self {
                    #(#variant_body),*
                }
            }

            fn kind(&self) -> ::aeronet::LaneKind {
                match *self {
                    #(#kind_body),*
                }
            }
        }
    })
}

// attributes

fn parse_lane_kind(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<TokenStream> {
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
            "UnreliableUnordered" => quote! { ::aeronet::LaneKind::UnreliableUnordered },
            "UnreliableOrdered" => quote! { ::aeronet::LaneKind::UnreliableOrdered },
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

    lane_kind.ok_or(Error::new_spanned(
        tokens,
        formatcp!("missing #[{LANE_KIND}] attribute"),
    ))
}
