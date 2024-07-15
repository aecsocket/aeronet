use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Result};

use crate::{util, LANE_KIND};

pub fn derive(input: &DeriveInput) -> Result<TokenStream> {
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

    let kind = parse_lane_kind(input, &input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::lane::LaneKey for #name #type_generics #where_clause {
            const ALL: &'static [::aeronet::lane::LaneKind] = {
                use ::aeronet::lane::LaneKind::*;
                &[#kind]
            };

            fn index(&self) -> ::aeronet::lane::LaneIndex {
                ::aeronet::lane::LaneIndex::from_raw(0)
            }

            fn kind(&self) -> ::aeronet::lane::LaneKind {
                use ::aeronet::lane::LaneKind::*;
                #kind
            }
        }
    })
}

fn on_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    struct Variant<'a> {
        ident: &'a Ident,
        kind: &'a TokenStream,
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
            Ok(Variant {
                ident: &variant.ident,
                kind: parse_lane_kind(variant, &variant.attrs)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let all_body = variants
        .iter()
        .map(|Variant { kind, .. }| {
            quote! { #kind }
        })
        .collect::<Vec<_>>();
    let index_body = variants
        .iter()
        .enumerate()
        .map(|(index, Variant { ident, .. })| {
            quote! {
                Self::#ident => ::aeronet::lane::LaneIndex::from_raw(#index)
            }
        })
        .collect::<Vec<_>>();
    let kind_body = variants
        .iter()
        .map(|Variant { ident, kind, .. }| {
            quote! { Self::#ident => #kind }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #impl_generics ::aeronet::lane::LaneKey for #name #type_generics #where_clause {
            const ALL: &'static [::aeronet::lane::LaneKind] = {
                use ::aeronet::lane::LaneKind::*;
                &[#(#all_body),*]
            };

            fn index(&self) -> ::aeronet::lane::LaneIndex {
                match *self {
                    #(#index_body),*
                }
            }

            fn kind(&self) -> ::aeronet::lane::LaneKind {
                use ::aeronet::lane::LaneKind::*;
                match *self {
                    #(#kind_body),*
                }
            }
        }
    })
}

// attributes

fn parse_lane_kind(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    util::require_attr_with_one_arg(LANE_KIND, tokens, attrs)
}
