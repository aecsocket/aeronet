#![doc = include_str!("../README.md")]

use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod lane_key;
mod util;

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

const LANE_KIND: &str = "lane_kind";
