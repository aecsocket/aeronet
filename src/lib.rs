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

pub use client::{ClientEvent, ClientTransport, ClientTransportConfig};
pub use message::{RecvMessage, SendMessage};
pub use server::{ClientId, ServerEvent, ServerTransport, ServerTransportConfig};
pub use transport::{RecvError, SessionError, TransportRemoteAddr, TransportRtt};

#[cfg(feature = "bevy")]
pub use client::plugin::{
    ClientTransportPlugin, ClientTransportSet, FromServer, LocalClientConnected,
    LocalClientDisconnected, ToServer,
};
#[cfg(feature = "bevy")]
pub use server::plugin::{
    DisconnectClient, FromClient, RemoteClientConnected, RemoteClientConnecting,
    RemoteClientDisconnected, ServerTransportPlugin, ServerTransportSet, ToClient,
};

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::AsyncRuntime;
