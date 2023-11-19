#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod channel_key;
mod on_channel;

/// Defines a type of key used to represent the different app-specific channels
/// that can be used to send messages.
///
/// # Attributes
///
/// * `#[channel_kind(kind)]` determines which kind of channel this variant
///   represents, where `kind` is a variant of `ChannelKind`.
///
/// # Usage
///
/// ## Struct
///
/// The type requires the attribute `#[channel_kind(..)]`.
///
/// ```ignore
/// #[derive(ChannelKey)]
/// #[channel_kind(Unreliable)]
/// struct AppChannel;
/// ```
///
/// ## Enum
///
/// All variants require the attribute `#[channel_kind(..)]`.
///
/// ```ignore
/// #[derive(Channels)]
/// enum AppChannel {
///     #[channel_kind(Datagram)]
///     LowPriority,
///     #[channel_kind(Stream)]
///     HighPriority,
/// }
/// ```
#[proc_macro_derive(ChannelKey, attributes(channel_kind))]
pub fn channel_key(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    channel_key::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Defines along what variant of a [`ChannelKey`] a message is sent.
///
/// # Attributes
///
/// * `#[channel_type(type)]` determines what `type` implementing [`ChannelKey`]
///   this message is sent along.
///
/// # Usage
///
/// ## Struct
///
/// The type requires the attributes `#[channel_type(..)]` and
/// `#[on_channel(..)]`.
///
/// ```ignore
/// #[derive(ChannelKey)]
/// #[channel_kind(Unreliable)]
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
///
/// ```ignore
/// #[derive(ChannelKey)]
/// enum AppChannel {
///     #[channel_kind(Unreliable)]
///     LowPriority,
///     #[channel_kind(ReliableOrdered)]
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
#[proc_macro_derive(OnChannel, attributes(channel_type, on_channel))]
pub fn on_channel(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    on_channel::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

const CHANNEL_KIND: &str = "channel_kind";
const CHANNEL_TYPE: &str = "channel_type";
const ON_CHANNEL: &str = "on_channel";
