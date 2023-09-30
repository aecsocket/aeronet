#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod client;
mod server;
mod stream;

pub use client::WebTransportClient;
pub use server::{
    back::WebTransportBackend, create_server, front::WebTransportServer, ClientInfo,
    OnStreamError, ServerMessage, ServerStream, StreamError, OnServerStream,
};
pub use stream::{StreamDefinitions, StreamId, StreamKind};
