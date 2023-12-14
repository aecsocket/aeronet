#![doc = include_str!("../README.md")]

pub use aeronet_derive::*;

pub mod error;

mod channel;
mod client;
mod message;
mod server;
mod transport;

#[cfg(feature = "bevy-tokio-rt")]
mod runtime;

pub use {channel::*, client::*, message::*, server::*, transport::*};

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::*;
