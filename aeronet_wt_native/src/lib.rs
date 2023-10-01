#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod client;
mod server;
mod stream;

pub use client::WebTransportClient;
pub use server::{
    back::WebTransportServerBackend, create_server, front::WebTransportServer, ClientInfo,
    OnServerStream, OnStreamError, SendOnServerStream, ServerStreamMessage, StreamError,
};
pub use stream::{ClientStream, ServerStream, StreamId, TransportStream, TransportStreams};
