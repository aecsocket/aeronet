#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod lane_key;
mod message;
mod on_lane;
mod util;

/// Implements `aeronet::message::Message` for the given type.
///
/// This is just a marker trait, so no logic is actually implemented.
///
/// # Usage
///
/// ```ignore
/// #[derive(Message)]
/// struct MyMessage(String);
/// ```
#[proc_macro_derive(Message)]
pub fn message(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    message::derive(&input).into()
}

/// Defines a type of key used to represent the different app-specific lanes
/// that can be used to send messages.
///
/// # Attributes
///
/// * `#[lane_kind(kind)]` determines which kind of lane this variant
///   represents, where `kind` is a variant of `LaneKind`.
/// * `#[drop_after(value)]` sets `LaneConfig::drop_after` for this variant.
/// * `#[resend_after(value)]` sets `LaneConfig::resend_after` for this variant.
/// * `#[ack_timeout(value)]` sets `LaneConfig::ack_timeout` for this variant.
///
/// # Usage
///
/// ## Struct
///
/// The type requires the attribute `#[lane_kind(..)]`. All other attributes are
/// optional.
///
/// ```ignore
/// #[derive(LaneKey)]
/// #[lane_kind(UnreliableUnordered)]
/// struct AppLane;
/// ```
///
/// ## Enum
///
/// All variants require the attribute `#[lane_kind(..)]`. All other attributes
/// are optional.
///
/// ```ignore
/// #[derive(LaneKey)]
/// enum AppLane {
///     #[lane_kind(UnreliableUnsequenced)]
///     LowPriority,
///     #[lane_kind(ReliableOrdered)]
///     HighPriority,
/// }
/// ```
#[proc_macro_derive(LaneKey, attributes(lane_kind, drop_after, resend_after, ack_timeout))]
pub fn lane_key(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    lane_key::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

const LANE_KIND: &str = "lane_kind";
const DROP_AFTER: &str = "drop_after";
const RESEND_AFTER: &str = "resend_after";
const ACK_TIMEOUT: &str = "ack_timeout";

/// Defines along what lane a message is sent.
///
/// # Attributes
///
/// * `#[on_lane(lane)]` determines what lane this variant maps to.
///   This can be any expression which can be passed to `LaneIndex::from`.
///
/// # Usage
///
/// ## Struct
///
/// The type requires the attribute `#[on_lane(..)]`.
///
/// ```ignore
/// #[derive(LaneKey)]
/// #[lane_kind(UnreliableUnsequenced)]
/// struct AppLane;
///
/// #[derive(OnLane)]
/// #[on_lane(AppLane)]
/// struct AppMessage(pub String);
/// ```
///
/// ## Enum
///
/// All variants require the attribute `#[on_lane(..)]`.
///
/// ```ignore
/// #[derive(LaneKey)]
/// enum AppLane {
///     #[lane_kind(UnreliableUnsequenced)]
///     LowPriority,
///     #[lane_kind(ReliableOrdered)]
///     HighPriority,
/// }
///
/// #[derive(OnLane)]
/// enum AppMessage {
///     #[on_lane(AppLane::LowPriority)]
///     Move(f32),
///     #[on_lane(AppLane::HighPriority)]
///     Shoot,
///     #[on_lane(AppLane::HighPriority)]
///     Chat { msg: String },
/// }
/// ```
#[proc_macro_derive(OnLane, attributes(on_lane))]
pub fn on_lane(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    on_lane::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

const ON_LANE: &str = "on_lane";
