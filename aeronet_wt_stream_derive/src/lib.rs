use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod on_stream;
mod stream;

#[proc_macro_derive(Stream, attributes(stream))]
pub fn derive_stream(input: TokenStream) -> TokenStream {
    let node = parse_macro_input!(input as DeriveInput);
    stream::derive(&node)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

#[proc_macro_derive(OnStream, attributes(on_stream))]
pub fn derive_on_stream(input: TokenStream) -> TokenStream {
    let node = parse_macro_input!(input as DeriveInput);
    on_stream::derive(&node)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
