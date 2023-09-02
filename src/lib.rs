#![warn(clippy::all)]
#![warn(clippy::cargo)]

mod client;
#[cfg(feature = "bevy")]
mod client_bevy;
mod server;
#[cfg(feature = "bevy")]
mod server_bevy;
mod transport;
mod util;

pub use generational_arena::{Arena, Index};

pub use client::{ClientDisconnectedError, ClientTransport, ClientTransportEvent};
#[cfg(feature = "bevy")]
pub use client_bevy::{
    ClientRecvEvent, ClientSendEvent, ClientTransportError, ClientTransportPlugin,
};
pub use server::{ServerClientsError, ServerTransport, ServerTransportEvent};
#[cfg(feature = "bevy")]
pub use server_bevy::{
    ClientSet, ServerRecvEvent, ServerSendEvent, ServerTransportError, ServerTransportPlugin, ServerDisconnectClientEvent,
};
pub use transport::{ClientId, DisconnectReason, Message, TransportSettings};
