use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Result};

use crate::{util, ON_LANE};

pub(super) fn derive(input: &DeriveInput) -> Result<TokenStream> {
    match &input.data {
        Data::Struct(_) => on_struct(input),
        Data::Enum(data) => on_enum(input, data),
        Data::Union(_) => Err(Error::new_spanned(
            input,
            "union as OnLane is not supported",
        )),
    }
}

fn on_struct(input: &DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let on_lane = parse_on_lane(input, &input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::lane::OnLane for #name #type_generics #where_clause {
            fn lane_index(&self) -> ::aeronet::lane::LaneIndex {
                ::aeronet::lane::LaneIndex::from(#on_lane)
            }
        }
    })
}

fn on_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    struct Variant<'a> {
        ident: &'a Ident,
        destruct: TokenStream,
        on_lane: &'a TokenStream,
    }

    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let variants = data
        .variants
        .iter()
        .map(|variant| {
            Ok(Variant {
                ident: &variant.ident,
                destruct: match variant.fields {
                    Fields::Unit => quote! {},
                    Fields::Named(_) => quote! { { .. } },
                    Fields::Unnamed(_) => quote! { (..) },
                },
                on_lane: parse_on_lane(variant, &variant.attrs)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let lane_index_body = variants
        .iter()
        .map(
            |Variant {
                 ident,
                 destruct,
                 on_lane,
             }| {
                quote! {
                    Self::#ident #destruct => ::aeronet::lane::LaneIndex::from(#on_lane)
                }
            },
        )
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #impl_generics ::aeronet::lane::OnLane for #name #type_generics #where_clause {
            fn lane_index(&self) -> ::aeronet::lane::LaneIndex {
                match *self {
                    #(#lane_index_body),*
                }
            }
        }
    })
}

// attributes

fn parse_on_lane(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    util::require_attr_with_one_arg(ON_LANE, tokens, attrs)
}
