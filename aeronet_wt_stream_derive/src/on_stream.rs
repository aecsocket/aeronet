use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Fields, Meta, Result};

pub fn derive(node: &DeriveInput) -> Result<TokenStream> {
    match &node.data {
        Data::Struct(_) => on_struct(node),
        Data::Enum(data) => on_enum(node, data),
        _ => Err(Error::new_spanned(
            node,
            "non-enum as OnStream is not supported",
        )),
    }
}

fn on_struct(node: &DeriveInput) -> Result<TokenStream> {
    let name = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let stream_type = stream_type(node)?;
    let stream_variant = stream_variant(node, &node.attrs)?;

    Ok(quote! {
        impl #impl_generics ::aeronet_wt_stream::OnStream<#stream_type> for #name #type_generics #where_clause {
            fn on_stream(&self) -> #stream_type {
                #stream_type::#stream_variant
            }
        }
    })
}

struct Variant<'a> {
    ident: &'a Ident,
    fields: &'a Fields,
    variant: &'a TokenStream,
}

fn on_enum(node: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    let name = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let stream_type = stream_type(node)?;
    let variants = variants(data)?;
    let match_body = match_body(stream_type, &variants);

    Ok(quote! {
        impl #impl_generics ::aeronet_wt_stream::OnStream<#stream_type> for #name #type_generics #where_clause {
            fn on_stream(&self) -> #stream_type {
                match self {
                    #(#match_body),*
                }
            }
        }
    })
}

const STREAM_TYPE: &str = "stream_type";

fn stream_type(node: &DeriveInput) -> Result<&TokenStream> {
    let mut stream_type = None;
    for attr in &node.attrs {
        if attr.path().is_ident(STREAM_TYPE) {
            if stream_type.is_some() {
                return Err(Error::new_spanned(
                    attr,
                    "duplicate #[stream_type] attribute",
                ));
            }
            let Meta::List(list) = &attr.meta else {
                return Err(Error::new_spanned(
                    attr,
                    "missing type in #[stream_type(type)]",
                ));
            };
            stream_type = Some(&list.tokens);
        }
    }

    match stream_type {
        Some(stream_type) => Ok(stream_type),
        None => Err(Error::new_spanned(node, "missing #[stream_type] attribute")),
    }
}

const STREAM_VARIANT: &str = "stream_variant";

fn stream_variant<T: ToTokens>(tokens: T, attrs: &[Attribute]) -> Result<&TokenStream> {
    let mut variant = None;
    for attr in attrs {
        if attr.path().is_ident(STREAM_VARIANT) {
            if variant.is_some() {
                return Err(Error::new_spanned(
                    attr,
                    "duplicate #[stream_variant] attribute",
                ));
            }
            let Meta::List(list) = &attr.meta else {
                return Err(Error::new_spanned(
                    attr,
                    "missing variant in #[stream_variant(variant)]",
                ));
            };
            variant = Some(&list.tokens);
        }
    }

    variant.ok_or(Error::new_spanned(
        tokens,
        "missing #[stream_variant] attribute",
    ))
}

fn variants(data: &DataEnum) -> Result<Vec<Variant<'_>>> {
    data.variants
        .iter()
        .map(|node| {
            stream_variant(node, &node.attrs).map(|variant| Variant {
                ident: &node.ident,
                fields: &node.fields,
                variant,
            })
        })
        .collect()
}

fn match_body(stream_type: &TokenStream, variants: &Vec<Variant<'_>>) -> Vec<TokenStream> {
    variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            let destruct = match variant.fields {
                Fields::Unit => quote! {},
                Fields::Named(_) => quote! { { .. } },
                Fields::Unnamed(_) => quote! { (..) },
            };
            let variant = variant.variant;
            quote! {
                Self::#pattern #destruct => #stream_type::#variant
            }
        })
        .collect()
}
