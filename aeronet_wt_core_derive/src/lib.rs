#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod channels;
mod on_channel;

/// Defines the different app-specific channels used by messages in this app.
///
/// * `#[channel_kind(kind)]` determines which kind of channel this variant
///   represents, where `kind` is a variant of [`ChannelKind`].
///
/// # Usage
///
/// ## Struct
///
/// The struct requires the attribute `#[channel_kind(..)]`.
/// ```
/// #[derive(Channels)]
/// #[channel_kind(Datagram)]
/// struct AppChannel;
/// ```
///
/// ## Enum
///
/// All variants require the attribute `#[channel_kind(..)]`.
/// ```
/// #[derive(Channels)]
/// enum AppChannel {
///     #[channel_kind(Datagram)]
///     LowPriority,
///     #[channel_kind(Stream)]
///     HighPriority,
/// }
/// ```
/// [`ChannelKind`]: aeronet_wt_core::ChannelKind
#[proc_macro_derive(Channels, attributes(channel_kind))]
pub fn derive_channels(input: TokenStream) -> TokenStream {
    let node = parse_macro_input!(input as DeriveInput);
    channels::derive(&node)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Defines along what variant of a [`Channels`] a message is sent.
///
/// * `#[channel_type(type)]` determines what `type` implementing [`Channels`]
///   variants of this type can be sent along.
/// * `#[on_channel(value)]` determines what value of type `type` this variant
///   is sent along.
///
/// # Usage
///
/// ## Struct
///
/// The type requires the attributes `#[channel_type(..)]` and
/// `#[on_channel(..)]`.
/// ```
/// #[derive(Channels)]
/// #[channel_kind(Datagram)]
/// struct AppChannel;
///
/// #[derive(OnChannel)]
/// #[channel_type(AppChannel)]
/// #[on_channel(AppChannel)]
/// struct AppMessage(pub String);
/// ```
///
/// ## Enum
///
/// The type requires the attribute `#[channel_type(..)]`.
///
/// All variants require the attribute `#[on_channel(..)]`.
/// ```
/// #[derive(Channels)]
/// enum AppChannel {
///     #[channel_kind(Datagram)]
///     LowPriority,
///     #[channel_kind(Stream)]
///     HighPriority,
/// }
///
/// #[derive(OnChannel)]
/// #[channel_type(AppChannel)]
/// enum AppMessage {
///     #[on_channel(AppChannel::LowPriority)]
///     Move(f32),
///     #[on_channel(AppChannel::HighPriority)]
///     Shoot,
///     #[on_channel(AppChannel::HighPriority)]
///     Chat { msg: String },
/// }
/// ```
/// [`Channels`]: aeronet_wt_core::Channels
#[proc_macro_derive(OnChannel, attributes(channel_type, on_channel))]
pub fn derive_on_channel(input: TokenStream) -> TokenStream {
    let node = parse_macro_input!(input as DeriveInput);
    on_channel::derive(&node)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
