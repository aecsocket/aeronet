#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod lane_key;
mod message;
mod on_lane;

/// Implements `aeronet::Message` for the given type.
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
///
/// # Usage
///
/// ## Struct
///
/// The type requires the attribute `#[lane_kind(..)]`.
///
/// ```ignore
/// #[derive(LaneKey)]
/// #[lane_kind(UnreliableUnordered)]
/// struct AppLane;
/// ```
///
/// ## Enum
///
/// All variants require the attribute `#[lane_kind(..)]`.
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
#[proc_macro_derive(LaneKey, attributes(lane_kind))]
pub fn lane_key(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    lane_key::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

/// Defines along what variant of a [`LaneKey`] a message is sent.
///
/// # Attributes
///
/// * `#[lane_type(type)]` determines what `type` implementing [`LaneKey`] this
///   message is sent along.
/// * `#[on_lane(lane)]` determines what variant of `lane_type` this variant
///   maps to.
///
/// # Usage
///
/// ## Struct
///
/// The type requires the attributes `#[lane_type(..)]` and
/// `#[on_lane(..)]`.
///
/// ```ignore
/// #[derive(LaneKey)]
/// #[lane_kind(UnreliableUnsequenced)]
/// struct AppLane;
///
/// #[derive(OnLane)]
/// #[lane_type(AppLane)]
/// #[on_lane(AppLane)]
/// struct AppMessage(pub String);
/// ```
///
/// ## Enum

/// The type requires the attribute `#[lane_type(..)]`.
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
/// #[lane_type(AppLane)]
/// enum AppMessage {
///     #[on_lane(AppLane::LowPriority)]
///     Move(f32),
///     #[on_lane(AppLane::HighPriority)]
///     Shoot,
///     #[on_lane(AppLane::HighPriority)]
///     Chat { msg: String },
/// }
/// ```
#[proc_macro_derive(OnLane, attributes(lane_type, on_lane))]
pub fn on_lane(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    on_lane::derive(&input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

const LANE_KIND: &str = "lane_kind";
const LANE_TYPE: &str = "lane_type";
const ON_LANE: &str = "on_lane";
