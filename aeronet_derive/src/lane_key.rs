use const_format::formatcp;
use proc_macro2::{Ident, TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Meta, Result};

use crate::{LANE_KIND, LANE_PRIORITY};

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
    let priority = get_lane_priority(&input.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet::LaneKey for #name #type_generics #where_clause {
            const VARIANTS: &'static [Self] = &[
                Self
            ];

            fn variant(&self) -> usize {
                0
            }

            fn kind(&self) -> ::aeronet::LaneKind {
                #kind
            }

            fn priority(&self) -> i32 {
                #priority
            }
        }
    })
}

fn on_enum(input: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    struct Variant<'a> {
        ident: &'a Ident,
        kind: TokenStream,
        priority: TokenStream,
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
            let priority = get_lane_priority(&variant.attrs)?;
            Ok(Variant {
                ident: &variant.ident,
                kind,
                priority,
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
    let priority_body = variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            let priority = &variant.priority;
            quote! { Self::#pattern => #priority }
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

            fn priority(&self) -> i32 {
                match *self {
                    #(#priority_body),*
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

fn parse_lane_priority(attrs: &[Attribute]) -> Result<Option<TokenStream>> {
    let mut lane_priority = None;
    for attr in attrs {
        if !attr.path().is_ident(LANE_PRIORITY) {
            continue;
        }

        if lane_priority.is_some() {
            return Err(Error::new_spanned(
                attr,
                formatcp!("duplicate #[{LANE_PRIORITY}] attribute"),
            ));
        }

        let Meta::List(list) = &attr.meta else {
            return Err(Error::new_spanned(
                attr,
                formatcp!("missing value in #[{LANE_PRIORITY}(value)]"),
            ));
        };

        lane_priority = Some(list.tokens.clone());
    }

    Ok(lane_priority)
}

fn get_lane_priority(attrs: &[Attribute]) -> Result<TokenStream> {
    Ok(parse_lane_priority(attrs)?.unwrap_or(quote! { 0 }))
}
