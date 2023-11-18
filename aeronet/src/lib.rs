#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod error;

//mod client;
mod message;
//mod server;
mod transport;

#[cfg(feature = "bevy-tokio-rt")]
mod runtime;

//pub use client::{ClientEvent, ClientTransport};
pub use message::{Message, TryFromBytes, TryIntoBytes};
//pub use server::{ClientId, ServerEvent, ServerTransport};
pub use transport::{RemoteAddr, Rtt};

// #[cfg(feature = "bevy")]
// pub use client::plugin::{
//     client_connected, ClientTransportPlugin, ClientTransportSet, FromServer, LocalClientConnected,
//     LocalClientDisconnected, ToServer,
// };
// #[cfg(feature = "bevy")]
// pub use server::plugin::{
//     DisconnectClient, FromClient, RemoteClientConnected, RemoteClientDisconnected,
//     ServerTransportPlugin, ServerTransportSet, ToClient,
// };

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::AsyncRuntime;
