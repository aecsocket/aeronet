use const_format::formatcp;
use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Meta, Result};

use crate::{LANE_TYPE, ON_LANE};

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

    let lane_type = parse_lane_type(input, &input.attrs)?;
    let on_lane = parse_on_lane(input, &input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::lane::OnLane for #name #type_generics #where_clause {
            type Lane = #lane_type;

            fn lane(&self) -> Self::Lane {
                #on_lane
            }
        }
    })
}

fn on_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    struct Variant<'a> {
        ident: &'a Ident,
        fields: &'a Fields,
        on_lane: &'a TokenStream,
    }

    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let lane_type = parse_lane_type(input, &input.attrs)?;
    let variants = data
        .variants
        .iter()
        .map(|variant| {
            parse_on_lane(variant, &variant.attrs).map(|on_lane| Variant {
                ident: &variant.ident,
                fields: &variant.fields,
                on_lane,
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
            let on_lane = variant.on_lane;
            quote! { Self::#pattern #destruct => #on_lane }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #impl_generics ::aeronet::lane::OnLane for #name #type_generics #where_clause {
            type Lane = #lane_type;

            fn lane(&self) -> Self::Lane {
                match *self {
                    #(#match_body),*
                }
            }
        }
    })
}

// attributes

fn parse_lane_type(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    let mut lane_type = None;
    for attr in attrs {
        if !attr.path().is_ident(LANE_TYPE) {
            continue;
        }

        if lane_type.is_some() {
            return Err(Error::new_spanned(
                attr,
                formatcp!("duplicate #[{LANE_TYPE}] attribute"),
            ));
        }

        let Meta::List(list) = &attr.meta else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing type in #[{LANE_TYPE}(type)]"),
            ));
        };

        lane_type = Some(&list.tokens);
    }

    lane_type.ok_or(Error::new_spanned(
        tokens,
        formatcp!("missing #[{LANE_TYPE}] attribute"),
    ))
}

fn parse_on_lane(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    let mut on_lane = None;
    for attr in attrs {
        if !attr.path().is_ident(ON_LANE) {
            continue;
        }

        if on_lane.is_some() {
            return Err(Error::new_spanned(
                attr,
                formatcp!("duplicate #[{ON_LANE}] attribute"),
            ));
        }

        let Meta::List(list) = &attr.meta else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing value in #[{ON_LANE}(value)]"),
            ));
        };

        on_lane = Some(&list.tokens);
    }

    on_lane.ok_or(Error::new_spanned(
        tokens,
        formatcp!("missing #[{ON_LANE}] attribute"),
    ))
}
