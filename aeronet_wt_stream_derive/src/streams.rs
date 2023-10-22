use proc_macro2::{Ident, TokenStream, TokenTree};
use quote::quote;
use syn::{Data, DataEnum, DeriveInput, Error, Meta, Result};

pub fn derive(node: &DeriveInput) -> Result<TokenStream> {
    match &node.data {
        Data::Enum(data) => on_enum(node, data),
        _ => Err(Error::new_spanned(
            node,
            "non-enum as Streams is not supported",
        )),
    }
}

const STREAM_KIND: &str = "stream_kind";

#[derive(Debug, Clone, Copy)]
enum StreamId {
    Datagram,
    Bi(usize),
}

struct Variant<'a> {
    ident: &'a Ident,
    stream_id: StreamId,
}

fn on_enum(node: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    let name = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let (variants, num_bi) = variants(data)?;
    let match_body = match_body(&variants);

    Ok(quote! {
        impl #impl_generics ::aeronet_wt_stream::Streams for #name #type_generics #where_clause {
            fn stream_id(&self) -> ::aeronet_wt_stream::StreamId {
                match self {
                    #(#match_body),*
                }
            }

            fn num_bi() -> usize {
                #num_bi
            }
        }
    })
}

fn variants(data: &DataEnum) -> Result<(Vec<Variant<'_>>, usize)> {
    let mut num_bi = 0;

    let variants = data
        .variants
        .iter()
        .map(|node| {
            let mut stream_id = None;

            for attr in &node.attrs {
                if attr.path().is_ident(STREAM_KIND) {
                    if stream_id.is_some() {
                        return Err(Error::new_spanned(
                            attr,
                            "duplicate #[stream_kind] attribute",
                        ));
                    }
                    let Meta::List(list) = &attr.meta else {
                        return Err(Error::new_spanned(
                            attr,
                            "missing kind in #[stream_kind(kind)]",
                        ));
                    };
                    // TODO this `.clone()` sucks
                    let Some(TokenTree::Ident(kind_ident)) = list.tokens.clone().into_iter().next()
                    else {
                        return Err(Error::new_spanned(
                            attr,
                            "missing kind in #[stream_kind(kind)]",
                        ));
                    };

                    let kind = kind_ident.to_string();
                    stream_id = Some(match kind.as_str() {
                        "Datagram" => StreamId::Datagram,
                        "Bi" => {
                            let id = StreamId::Bi(num_bi);
                            num_bi += 1;
                            id
                        }
                        _ => {
                            return Err(Error::new_spanned(
                                kind_ident,
                                format!("invalid stream kind `{}`", kind),
                            ))
                        }
                    });
                }
            }

            match stream_id {
                Some(stream_id) => Ok(Variant {
                    ident: &node.ident,
                    stream_id,
                }),
                None => Err(Error::new_spanned(node, "missing #[stream_kind] attribute")),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((variants, num_bi))
}

fn match_body(variants: &Vec<Variant<'_>>) -> Vec<TokenStream> {
    variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            let stream_id = match variant.stream_id {
                StreamId::Datagram => quote! { Datagram },
                StreamId::Bi(i) => quote! { Bi(#i) },
            };
            quote! {
                Self::#pattern => ::aeronet_wt_stream::StreamId::#stream_id
            }
        })
        .collect()
}
