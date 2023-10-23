#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod bindings;
mod client;

pub use aeronet_wt_core::*;

pub use client::WebTransportClient;
