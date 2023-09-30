#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod error;

mod client;
mod message;
mod server;
mod transport;

#[cfg(feature = "bevy-tokio-rt")]
mod runtime;

pub use client::{
    ClientEvent, ClientRemoteAddr, ClientRtt, ClientTransport, ClientTransportConfig,
};
pub use message::{RecvMessage, SendMessage};
#[cfg(feature = "bevy")]
pub use server::plugin::{
    ClientConnected, ClientDisconnected, ClientIncoming, DisconnectClient, FromClient,
    ServerTransportPlugin, ServerTransportSet, ToClient,
};
pub use server::{
    ClientId, ServerEvent, ServerRemoteAddr, ServerRtt, ServerTransport, ServerTransportConfig,
};
pub use transport::{RecvError, SessionError};

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::AsyncRuntime;
