use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{Data, DataEnum, DeriveInput, Error, Fields, Meta, Result};

pub fn derive(node: &DeriveInput) -> Result<TokenStream> {
    match &node.data {
        Data::Enum(data) => on_enum(node, data),
        _ => Err(Error::new_spanned(
            node,
            "non-enum as OnStream is not supported",
        )),
    }
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

const ON_STREAM: &str = "on_stream";

fn stream_type(node: &DeriveInput) -> Result<&TokenStream> {
    let mut stream_type = None;
    for attr in &node.attrs {
        if attr.path().is_ident(ON_STREAM) {
            if stream_type.is_some() {
                return Err(Error::new_spanned(attr, "duplicate #[on_stream] attribute"));
            }
            let Meta::List(list) = &attr.meta else {
                return Err(Error::new_spanned(
                    attr,
                    "missing type in #[on_stream(type)]",
                ));
            };
            stream_type = Some(&list.tokens);
        }
    }

    match stream_type {
        Some(stream_type) => Ok(stream_type),
        None => Err(Error::new_spanned(node, "missing #[on_stream] attribute")),
    }
}

fn variants(data: &DataEnum) -> Result<Vec<Variant<'_>>> {
    data.variants
        .iter()
        .map(|node| {
            let mut variant = None;
            for attr in &node.attrs {
                if attr.path().is_ident(ON_STREAM) {
                    if variant.is_some() {
                        return Err(Error::new_spanned(attr, "duplicate #[on_stream] attribute"));
                    }
                    let Meta::List(list) = &attr.meta else {
                        return Err(Error::new_spanned(
                            attr,
                            "missing variant in #[on_stream(variant)]",
                        ));
                    };
                    variant = Some(&list.tokens);
                }
            }

            match variant {
                Some(variant) => Ok(Variant {
                    ident: &node.ident,
                    fields: &node.fields,
                    variant,
                }),
                None => Err(Error::new_spanned(node, "missing #[on_stream] attribute")),
            }
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
