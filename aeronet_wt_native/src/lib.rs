#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod client;
mod server;
mod shared;
mod stream;
mod transport;

pub use wtransport;

pub use client::{back::WebTransportClientBackend, create_client, front::WebTransportClient};
pub use server::{back::WebTransportServerBackend, create_server, front::WebTransportServer};
pub use stream::{ClientStream, ServerStream, StreamId, TransportStream, TransportStreams};
pub use transport::{EndpointInfo, OnStream, OnStreamError, SendOn, StreamError, StreamMessage};
