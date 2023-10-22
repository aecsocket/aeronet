use proc_macro2::{Ident, TokenStream};
use quote::quote;
use syn::{Data, DataEnum, DeriveInput, Error, Meta, Result};

pub fn derive(node: &DeriveInput) -> Result<TokenStream> {
    match &node.data {
        Data::Enum(data) => on_enum(node, data),
        _ => Err(Error::new_spanned(
            node,
            "non-enum as Stream is not supported",
        )),
    }
}

const STREAM: &str = "stream";

struct Variant<'a> {
    ident: &'a Ident,
    kind: &'a TokenStream,
}

fn on_enum(node: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    let name = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let variants = variants(data)?;
    let match_body = match_body(&variants);

    Ok(quote! {
        impl #impl_generics ::aeronet_wt_stream::Stream for #name #type_generics #where_clause {
            fn kind(&self) -> ::aeronet_wt_stream::StreamKind {
                match self {
                    #(#match_body),*
                }
            }
        }
    })
}

fn variants(data: &DataEnum) -> Result<Vec<Variant<'_>>> {
    data.variants
        .iter()
        .map(|node| {
            let mut kind: Option<&TokenStream> = None;

            for attr in &node.attrs {
                if attr.path().is_ident(STREAM) {
                    if kind.is_some() {
                        return Err(Error::new_spanned(attr, "duplicate #[stream] attribute"));
                    }
                    let Meta::List(list) = &attr.meta else {
                        return Err(Error::new_spanned(attr, "missing kind in #[stream(kind)]"));
                    };
                    kind = Some(&list.tokens);
                }
            }

            match kind {
                Some(kind) => Ok(Variant {
                    ident: &node.ident,
                    kind,
                }),
                None => Err(Error::new_spanned(node, "missing #[stream] attribute")),
            }
        })
        .collect()
}

fn match_body(variants: &Vec<Variant<'_>>) -> Vec<TokenStream> {
    variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            let kind = variant.kind;
            quote! {
                Self::#pattern => ::aeronet_wt_stream::StreamKind::#kind
            }
        })
        .collect()
}
