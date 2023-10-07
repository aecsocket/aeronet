#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]
//! # Getting started
//! 
//! To create a client, use [`create_client`] and use [`WebTransportClient`].
//! 
//! To create a server, use [`create_server`] and use [`WebTransportServer`].

mod client;
mod server;
mod shared;
mod stream;
mod transport;

pub use wtransport;

pub use client::{back::WebTransportClientBackend, create_client, front::WebTransportClient};
pub use server::{
    back::WebTransportServerBackend, create_server, front::WebTransportServer, RemoteClientInfo,
};
pub use stream::{ClientStream, ServerStream, StreamId, TransportStream, TransportStreams};
pub use transport::{EndpointInfo, OnStream, OnStreamError, SendOn, StreamError, StreamMessage};
