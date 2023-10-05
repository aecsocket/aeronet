#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]
//! # Getting started
//!
//! First, you will need a transport implementation to use. Select one from the list above that
//! suits your needs. Afterwards, use the [`ClientTransport`] and [`ServerTransport`] traits to
//! interact with the transport, to do functions such as sending and receiving data.
//!
//! [`ClientTransport`]: crate::ClientTransport
//! [`ServerTransport`]: crate::ServerTransport

pub mod error;

mod client;
mod message;
mod server;
mod transport;

#[cfg(feature = "bevy-tokio-rt")]
mod runtime;

pub use client::{ClientEvent, ClientTransport};
pub use message::{Message, TryFromBytes, TryIntoBytes};
pub use server::{ClientId, ServerEvent, ServerTransport};
pub use transport::{RecvError, RemoteAddr, Rtt, SessionError};

#[cfg(feature = "bevy")]
pub use client::plugin::{
    ClientTransportPlugin, ClientTransportSet, FromServer, LocalClientConnected,
    LocalClientConnecting, LocalClientDisconnected, ToServer, client_connected,
};
#[cfg(feature = "bevy")]
pub use server::plugin::{
    DisconnectClient, FromClient, RemoteClientConnected, RemoteClientConnecting,
    RemoteClientDisconnected, ServerTransportPlugin, ServerTransportSet, ToClient,
};

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::AsyncRuntime;
