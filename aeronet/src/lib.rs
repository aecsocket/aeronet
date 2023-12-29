#![doc = include_str!("../README.md")]

pub use aeronet_derive::*;

mod client;
mod lane;
mod message;
mod server;
mod transport;
mod protocol;

pub use {client::*, lane::*, message::*, server::*, transport::*, protocol::*};

#[cfg(feature = "bevy-tokio-rt")]
pub mod runtime;

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::*;
