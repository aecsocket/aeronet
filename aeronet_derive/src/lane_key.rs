use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Result};

use crate::{util, ACK_TIMEOUT, DROP_AFTER, LANE_KIND, RESEND_AFTER};

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

    let kind = parse_lane_kind(input, &input.attrs)?;
    let drop_after = parse_drop_after(&input.attrs)?;
    let resend_after = parse_resend_after(&input.attrs)?;
    let ack_timeout = parse_ack_timeout(&input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::lane::LaneKey for #name #type_generics #where_clause {
            const CONFIGS: &'static [::aeronet::lane::LaneConfig] = {
                use ::core::time::Duration;
                use ::aeronet::lane::LaneKind::*;

                &[
                    ::aeronet::lane::LaneConfig {
                        kind: #kind,
                        drop_after: #drop_after,
                        resend_after: #resend_after,
                        ack_timeout: #ack_timeout,
                    }
                ]
            };

            fn lane_index(&self) -> ::aeronet::lane::LaneIndex {
                ::aeronet::lane::LaneIndex::from_raw(0)
            }
        }
    })
}

fn on_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    struct Variant<'a> {
        ident: &'a Ident,
        kind: &'a TokenStream,
        drop_after: TokenStream,
        resend_after: TokenStream,
        ack_timeout: TokenStream,
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
                drop_after: parse_drop_after(&variant.attrs)?,
                resend_after: parse_resend_after(&variant.attrs)?,
                ack_timeout: parse_ack_timeout(&variant.attrs)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let lane_index_body = variants
        .iter()
        .enumerate()
        .map(|(index, Variant { ident, .. })| {
            quote! {
                Self::#ident => ::aeronet::lane::LaneIndex::from_raw(#index)
            }
        })
        .collect::<Vec<_>>();
    let configs_body = variants
        .iter()
        .map(
            |Variant {
                 kind,
                 drop_after,
                 resend_after,
                 ack_timeout,
                 ..
             }| {
                quote! {
                    ::aeronet::lane::LaneConfig {
                        kind: #kind,
                        drop_after: #drop_after,
                        resend_after: #resend_after,
                        ack_timeout: #ack_timeout,
                    }
                }
            },
        )
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #impl_generics ::aeronet::lane::LaneKey for #name #type_generics #where_clause {
            const CONFIGS: &'static [::aeronet::lane::LaneConfig] = {
                use ::core::time::Duration;
                use ::aeronet::lane::LaneKind::*;

                &[#(#configs_body),*]
            };

            fn lane_index(&self) -> ::aeronet::lane::LaneIndex {
                match *self {
                    #(#lane_index_body),*
                }
            }
        }
    })
}

// attributes

fn parse_lane_kind(tokens: impl ToTokens, attrs: &[Attribute]) -> Result<&TokenStream> {
    util::require_attr_with_one_arg(LANE_KIND, tokens, attrs)
}

fn parse_drop_after(attrs: &[Attribute]) -> Result<TokenStream> {
    let value = util::parse_attr_with_one_arg(DROP_AFTER, attrs)?;
    Ok(value.cloned().unwrap_or(quote! {
        ::aeronet::lane::LaneConfig::new().drop_after
    }))
}

fn parse_resend_after(attrs: &[Attribute]) -> Result<TokenStream> {
    let value = util::parse_attr_with_one_arg(RESEND_AFTER, attrs)?;
    Ok(value.cloned().unwrap_or(quote! {
        ::aeronet::lane::LaneConfig::new().resend_after
    }))
}

fn parse_ack_timeout(attrs: &[Attribute]) -> Result<TokenStream> {
    let value = util::parse_attr_with_one_arg(ACK_TIMEOUT, attrs)?;
    Ok(value.cloned().unwrap_or(quote! {
        ::aeronet::lane::LaneConfig::new().ack_timeout
    }))
}
