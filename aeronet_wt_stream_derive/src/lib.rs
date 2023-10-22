#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod on_stream;
mod streams;

/// Defines the different app-specific streams used by messages in this app.
///
/// This may only be derived on an enum, and each variant must have the
/// attribute `#[stream(stream_kind)]`, where `stream_kind` is a
/// [`StreamKind`] variant. This attribute determines which
/// type of stream this variant represents.
///
/// # Example
///
/// ```
/// #[derive(Streams)]
/// enum AppStream {
///     #[stream_kind(Datagram)]
///     LowPriority,
///     #[stream_kind(Bi)]
///     HighPriority,
/// }
/// ```
///
/// [`StreamKind`]: aeronet_wt_stream::StreamKind
#[proc_macro_derive(Streams, attributes(stream_kind))]
pub fn derive_streams(input: TokenStream) -> TokenStream {
    let node = parse_macro_input!(input as DeriveInput);
    streams::derive(&node)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Defines along what stream a message is sent.
///
/// Once an app defines a [`Streams`] enum to determine which streams are
/// available for communication, this macro allows determining along which
/// variant of that [`Streams`] this message is sent.
///
/// Use the attributes `#[stream_type(..)]` and `#[stream_variant(..)]` to
/// define the type and variant respectively.
///
/// # Examples
///
/// Assuming the following [`Streams`]:
/// ```
/// #[derive(Streams)]
/// enum AppStream {
///     #[stream_kind(Datagram)]
///     LowPriority,
///     #[stream_kind(Bi)]
///     HighPriority,
/// }
/// ```
///
/// ## On a struct
///
/// ```
/// # #[derive(Streams)]
/// # enum AppStream {
/// #     #[stream_kind(Datagram)]
/// #     LowPriority,
/// #     #[stream_kind(Bi)]
/// #     HighPriority,
/// # }
/// #[derive(OnStream)]
/// #[stream_type(AppStream)]
/// #[stream_variant(LowPriority)]
/// struct ChatMessage(pub String);
/// ```
///
/// ## On an enum
///
/// ```
/// # #[derive(Streams)]
/// # enum AppStream {
/// #     #[stream_kind(Datagram)]
/// #     LowPriority,
/// #     #[stream_kind(Bi)]
/// #     HighPriority,
/// # }
/// #[derive(OnStream)]
/// #[stream_type(AppStream)]
/// enum ClientMessage {
///     #[stream_variant(LowPriority)]
///     Move(f32),
///     #[stream_variant(HighPriority)]
///     Shoot,
///     #[stream_variant(HighPriority)]
///     Chat { msg: String },
/// }
/// ```
#[proc_macro_derive(OnStream, attributes(stream_type, stream_variant))]
pub fn derive_on_stream(input: TokenStream) -> TokenStream {
    let node = parse_macro_input!(input as DeriveInput);
    on_stream::derive(&node)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
