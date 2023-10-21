#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod bindings;
mod client;

pub use client::WebTransportClient;
