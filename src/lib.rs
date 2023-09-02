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

pub use client::ClientTransport;
#[cfg(feature = "bevy")]
pub use client_bevy::{ClientRecvEvent, ClientSendEvent, ClientTransportPlugin, ClientTransportError};
pub use server::{ServerTransport, ServerTransportEvent};
#[cfg(feature = "bevy")]
pub use server_bevy::{ClientSet, ServerRecvEvent, ServerSendEvent, ServerTransportPlugin, ServerTransportError};
pub use transport::{Message, TransportSettings};
pub use util::{ClientId, InvalidClientError};
