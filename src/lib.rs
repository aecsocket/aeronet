#![warn(clippy::all)]
#![warn(clippy::cargo)]
#![warn(missing_docs)]
#![doc = include_str!("../README.md")]

mod client;
mod server;
mod transport;
mod util;

pub use generational_arena::{Arena, Index};

#[cfg(feature = "bevy")]
pub use client::plugin::{
    ClientRecvEvent, ClientSendEvent, ClientTransportError, ClientTransportPlugin,
};
pub use client::{ClientTransport, ClientTransportEvent};
#[cfg(feature = "bevy")]
pub use server::plugin::{
    ClientSet, ServerDisconnectClientEvent, ServerRecvEvent, ServerSendEvent, ServerTransportError,
    ServerTransportPlugin,
};
pub use server::{ServerClientsError, ServerTransport, ServerTransportEvent};
pub use transport::{ClientId, DisconnectReason, Message, TransportSettings};
