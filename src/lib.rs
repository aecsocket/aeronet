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
pub use server::{
    ClientId, ServerEvent, ServerRemoteAddr, ServerRtt, ServerTransport, ServerTransportConfig,
};
pub use transport::{RecvError, SessionError};

#[cfg(feature = "bevy")]
pub use client::plugin::{
    ClientTransportPlugin, ClientTransportSet, FromServer, LocalClientConnected,
    LocalClientDisconnected, ToServer,
};
#[cfg(feature = "bevy")]
pub use server::plugin::{
    DisconnectClient, FromClient, RemoteClientConnected, RemoteClientDisconnected,
    RemoteClientIncoming, ServerTransportPlugin, ServerTransportSet, ToClient,
};

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::AsyncRuntime;
