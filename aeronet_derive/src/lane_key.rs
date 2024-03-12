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
            const CONFIGS: &'static [::aeronet::lane::LaneConfig] = &[
                ::aeronet::lane::LaneConfig {
                    kind: #kind,
                    drop_after: {
                        use ::core::time::Duration;
                        Duration::from(#drop_after)
                    },
                    resend_after: {
                        use ::core::time::Duration;
                        Duration::from(#resend_after)
                    },
                    ack_timeout: {
                        use ::core::time::Duration;
                        Duration::from(#ack_timeout)
                    },
                }
            ];

            fn lane_index(&self) -> ::aeronet::lane::LaneIndex {
                ::aeronet::lane::LaneIndex::from_raw(0)
            }

            fn configs(&self) -> &'static [::aeronet::lane::LaneConfig] {
                use ::core::time::Duration;

                &[::aeronet::lane::LaneConfig {
                    kind: #kind,
                    drop_after: Duration::from(#drop_after),
                    resend_after: Duration::from(#resend_after),
                    ack_timeout: Duration::from(#ack_timeout),
                }]
            }
        }
    })
}

fn on_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    struct Variant<'a> {
        ident: &'a Ident,
        kind: TokenStream,
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

            let kind = parse_lane_kind(variant, &variant.attrs)?;
            let drop_after = parse_drop_after(&variant.attrs)?;
            let resend_after = parse_resend_after(&variant.attrs)?;
            let ack_timeout = parse_ack_timeout(&variant.attrs)?;
            Ok(Variant {
                ident: &variant.ident,
                kind: kind.clone(),
                drop_after: drop_after.clone(),
                resend_after: resend_after.clone(),
                ack_timeout: ack_timeout.clone(),
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
    let config_body = variants
        .iter()
        .map(
            |Variant {
                 ident,
                 kind,
                 drop_after,
                 resend_after,
                 ack_timeout,
             }| {
                quote! {
                    Self::#ident => ::aeronet::lane::LaneConfig {
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
        impl #impl_generics ::aeronet::lane::LaneIndex for #name #type_generics #where_clause {
            fn lane_index(&self) -> usize {
                match *self {
                    #(#index_body),*
                }
            }
        }

        impl #impl_generics ::aeronet::lane::LaneKey for #name #type_generics #where_clause {
            const VARIANTS: &'static [Self] = &[
                #(#all_variants),*
            ];

            fn config(&self) -> ::aeronet::lane::LaneConfig {
                use ::aeronet::lane::LaneKind::*;
                use ::core::time::Duration;

                match *self {
                    #(#config_body),*
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
        ::aeronet::lane::LaneConfig::default().drop_after
    }))
}

fn parse_resend_after(attrs: &[Attribute]) -> Result<TokenStream> {
    let value = util::parse_attr_with_one_arg(RESEND_AFTER, attrs)?;
    Ok(value.cloned().unwrap_or(quote! {
        ::aeronet::lane::LaneConfig::default().resend_after
    }))
}

fn parse_ack_timeout(attrs: &[Attribute]) -> Result<TokenStream> {
    let value = util::parse_attr_with_one_arg(ACK_TIMEOUT, attrs)?;
    Ok(value.cloned().unwrap_or(quote! {
        ::aeronet::lane::LaneConfig::default().ack_timeout
    }))
}
