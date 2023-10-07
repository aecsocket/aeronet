#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]
//! # Getting started
//! 
//! The entry point to the crate is the server transport, as you cannot have a client without
//! first creating a server. See [`ChannelTransportServer`] to get started.

mod client;
mod server;
mod shared;

pub use client::ChannelTransportClient;
pub use server::ChannelTransportServer;
pub use shared::DisconnectedError;
