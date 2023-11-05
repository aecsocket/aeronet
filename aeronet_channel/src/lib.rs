#![warn(clippy::all)]
//#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod client;
mod server;

pub use client::{ClientState, ChannelClient, Connected, Disconnected};
pub use server::{ChannelServer, ClientKey};
