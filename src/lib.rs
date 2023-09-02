mod client;
#[cfg(feature = "bevy")]
mod client_bevy;
mod server;
#[cfg(feature = "bevy")]
mod server_bevy;
mod transport;
mod util;

pub use generational_arena::{Arena, Index};

pub use client::{ClientTransport, ClientTransportError};
#[cfg(feature = "bevy")]
pub use client_bevy::{ClientRecvEvent, ClientSendEvent, ClientTransportPlugin};
pub use server::{ServerTransport, ServerTransportError};
#[cfg(feature = "bevy")]
pub use server_bevy::{ServerRecvEvent, ServerSendEvent, ServerTransportPlugin};
pub use transport::{Message, TransportSettings};
pub use util::ClientId;
