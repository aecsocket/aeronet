#![doc = include_str!("../README.md")]

pub use aeronet_derive::*;

mod client;
mod lane;
mod message;
mod protocol;
mod server;
mod transport;

pub use {client::*, lane::*, message::*, protocol::*, server::*, transport::*};

#[cfg(feature = "bevy-tokio-rt")]
pub mod runtime;

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::*;
