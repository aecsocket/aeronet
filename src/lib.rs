#![warn(clippy::all)]
#![warn(clippy::cargo)]

mod client;
mod server;
mod transport;
mod util;

pub use generational_arena::{Arena, Index};

pub use client::{ClientTransport, ClientTransportEvent};
#[cfg(feature = "bevy")]
pub use client::plugin::{
    ClientRecvEvent, ClientSendEvent, ClientTransportError, ClientTransportPlugin,
};
pub use server::{ServerClientsError, ServerTransport, ServerTransportEvent};
#[cfg(feature = "bevy")]
pub use server::plugin::{
    ClientSet, ServerDisconnectClientEvent, ServerRecvEvent, ServerSendEvent, ServerTransportError,
    ServerTransportPlugin,
};
pub use transport::{ClientId, DisconnectReason, Message, TransportSettings};
