#![warn(clippy::all)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]
//! # Getting started
//!
//! First, you will need a transport implementation to use. Select one from the list above that
//! suits your needs. Afterwards, use the [`ClientTransport`] and [`ServerTransport`] traits to
//! interact with the transport, to do functions such as sending and receiving data.
#![cfg_attr(
    feature = "bevy",
    doc = r##"

# With Bevy

Enable the `bevy` feature flag to enable support for the [`bevy`](https://docs.rs/bevy) game
engine, allowing you to use the built-in transport plugins and events.

```
use bevy::prelude::*;
use aeronet::{ServerTransportConfig, ServerTransportPlugin};

#[derive(Debug, Clone)]
pub struct AppMessage;
# impl aeronet::SendMessage for AppMessage {
#     fn into_payload(self) -> anyhow::Result<Vec<u8>> { unimplemented!() }
# }
# impl aeronet::RecvMessage for AppMessage {
#     fn from_payload(buf: &[u8]) -> anyhow::Result<Self> { unimplemented!() }
# }

pub struct AppTransportConfig;

impl ServerTransportConfig for AppTransportConfig {
    type C2S = AppMessage;
    type S2C = AppMessage;
}

# fn run<MyTransport>()
# where
#     MyTransport: ServerTransport<AppTransportConfig> {
App::new()
    .add_plugins((
        DefaultPlugins,
        ServerTransportPlugin::<AppTransportConfig, MyTransport>::default(),
    ))
    .run();
# }
```
"##
)]
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
pub use message::{Message, MessageTypes, TryFromBytes, TryIntoBytes};
pub use server::{ClientId, ServerEvent, ServerTransport};
pub use transport::{RecvError, SessionError, TransportRemoteAddr, TransportRtt};

#[cfg(feature = "bevy")]
pub use client::plugin::{
    ClientTransportPlugin, ClientTransportSet, FromServer, LocalClientConnected,
    LocalClientConnecting, LocalClientDisconnected, ToServer,
};
#[cfg(feature = "bevy")]
pub use server::plugin::{
    DisconnectClient, FromClient, RemoteClientConnected, RemoteClientConnecting,
    RemoteClientDisconnected, ServerTransportPlugin, ServerTransportSet, ToClient,
};

#[cfg(feature = "bevy-tokio-rt")]
pub use runtime::AsyncRuntime;
