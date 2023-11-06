#![warn(clippy::all)]
//#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod client;
mod server;

pub use client::{TransportClient, ChannelClient, Connected as ConnectedClient, Disconnected as DisconnectedClient};
pub use server::{ChannelServer, ClientKey};
