use proc_macro2::{Ident, TokenStream, TokenTree};
use quote::{quote, ToTokens};
use syn::{Attribute, Data, DataEnum, DeriveInput, Error, Meta, Result};

pub fn derive(node: &DeriveInput) -> Result<TokenStream> {
    match &node.data {
        Data::Struct(_) => on_struct(node),
        Data::Enum(data) => on_enum(node, data),
        Data::Union(_) => Err(Error::new_spanned(
            node,
            "union as Channels is not supported",
        )),
    }
}

#[derive(Debug, Clone, Copy)]
enum ChannelId {
    Datagram,
    Stream(usize),
}

impl ChannelId {
    fn fqn(&self) -> TokenStream {
        let variant = match self {
            Self::Datagram => quote! { Datagram },
            Self::Stream(i) => quote! { Stream(#i) },
        };
        quote! { ::aeronet_wt_core::ChannelId::#variant }
    }
}

const CHANNEL_KIND: &str = "channel_kind";

fn on_struct(node: &DeriveInput) -> Result<TokenStream> {
    let name = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let mut num_streams = 0;
    let channel_id = parse_channel_id(node, &node.attrs, &mut num_streams)?;
    let channel_id_body = channel_id.fqn();

    Ok(quote! {
        impl #impl_generics ::aeronet_wt_core::Channels for #name #type_generics #where_clause {
            fn channel_id(&self) -> ::aeronet_wt_core::ChannelId {
                #channel_id_body
            }

            fn num_streams() -> usize {
                #num_streams
            }
        }
    })
}

struct Variant<'a> {
    ident: &'a Ident,
    channel_id: ChannelId,
}

fn on_enum(node: &DeriveInput, data: &DataEnum) -> Result<TokenStream> {
    let name = &node.ident;
    let generics = &node.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    let mut num_streams = 0;
    let variants = data
        .variants
        .iter()
        .map(|node| {
            parse_channel_id(node, &node.attrs, &mut num_streams).map(|channel_id| Variant {
                ident: &node.ident,
                channel_id,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let match_body = variants
        .iter()
        .map(|variant| {
            let pattern = variant.ident;
            let body = variant.channel_id.fqn();
            quote! { Self::#pattern => #body }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        impl #impl_generics ::aeronet_wt_core::Channels for #name #type_generics #where_clause {
            fn channel_id(&self) -> ::aeronet_wt_core::ChannelId {
                match self {
                    #(#match_body),*
                }
            }

            fn num_streams() -> usize {
                #num_streams
            }
        }
    })
}

fn parse_channel_id(
    tokens: impl ToTokens,
    attrs: &[Attribute],
    num_streams: &mut usize,
) -> Result<ChannelId> {
    let mut channel_id = None;
    for attr in attrs {
        if attr.path().is_ident(CHANNEL_KIND) {
            if channel_id.is_some() {
                return Err(Error::new_spanned(
                    attr,
                    "duplicate #[channel_kind] attribute",
                ));
            }
            let Meta::List(list) = &attr.meta else {
                return Err(Error::new_spanned(
                    attr,
                    "missing kind in #[channel_kind(kind)]",
                ));
            };
            // TODO this `.clone()` sucks
            let Some(TokenTree::Ident(kind_ident)) = list.tokens.clone().into_iter().next() else {
                return Err(Error::new_spanned(
                    attr,
                    "missing kind in #[channel_kind(kind)]",
                ));
            };

            channel_id = Some(match kind_ident.to_string().as_str() {
                "Datagram" => ChannelId::Datagram,
                "Stream" => {
                    let id = ChannelId::Stream(*num_streams);
                    *num_streams += 1;
                    id
                }
                kind => {
                    return Err(Error::new_spanned(
                        kind_ident,
                        format!("invalid channel kind `{}`", kind),
                    ))
                }
            });
        }
    }

    channel_id.ok_or(Error::new_spanned(
        tokens,
        "missing #[channel_kind] atribute",
    ))
}
