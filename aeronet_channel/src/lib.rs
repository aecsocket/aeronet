#![warn(clippy::all)]
//#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod client;
mod server;

pub use client::{
    ChannelClient, Connected as ConnectedClient, Disconnected as DisconnectedClient,
    TransportClient,
};
pub use server::{ChannelServer, ClientKey};
