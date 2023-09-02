//! The client-side functionality of this crate.

#[cfg(feature = "bevy")]
pub mod plugin;

use anyhow::Result;

use crate::{DisconnectReason, TransportSettings};

/// Sent when a [`ClientTransport`] receives some sort of non-message event, such as a connection
/// or disconnection.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ClientTransportEvent {
    /// The client connected to a server.
    Connect,
    /// The client lost connection from its previously connected server.
    Disconnect {
        /// The reason why the connection was lost.
        reason: DisconnectReason,
    },
}

/// The main client-side interface for transmitting data to, and receiving data from, a server.
///
/// The server may be local or remote; the transport is just an interface to allow communication
/// between the two.
///
/// Consuming or sending data using this transport will never panic, however an error may be
/// emitted.
///
/// # Consuming data
///
/// The transport's functions must be called in a specific order to ensure it stays in an optimal
/// state. A suboptimal state is not *invalid*, however will result in transmission errors being
/// emitted that can easily be avoided by keeping the correct state (for example, if you attempt
/// to receive data from the server *before* checking if you've lost connection, then that would
/// probably emit an error).
///
/// During a single update cycle (e.g. a single frame in a game loop):
/// - call [`Self::pop_event`] until all events are consumed
///   - if [`ClientTransportEvent::Disconnect`] is emitted, stop all processing
/// - call [`Self::recv`] until either an error or `Ok(None)`
///   - do this *before* your main game logic (in Bevy, this would be in `PreUpdate`)
/// - call [`Self::send`] for all the data you want to send
///   - do this *after* your main game logic (in Bevy, this would be in `PostUpdate`)
pub trait ClientTransport<S: TransportSettings> {
    /// Consumes a single event from this transport's event buffer.
    ///
    /// This should be called in the order defined in the [trait docs](trait.ClientTransport.html).
    fn pop_event(&mut self) -> Option<ClientTransportEvent>;

    /// Consumes a single message from this transport's server-to-client message buffer.
    ///
    /// This should be called until either `Err` or `Ok(None)` are returned, at which point you
    /// should stop receiving any more data.
    ///
    /// This should be called in the order defined in the [trait docs](trait.ClientTransport.html).
    fn recv(&mut self) -> Result<Option<S::S2C>>;

    /// Sends a single message into this transport's client-to-server message buffer.
    ///
    /// This should be called in the order defined in the [trait docs](trait.ClientTransport.html).
    fn send(&mut self, msg: impl Into<S::C2S>) -> Result<()>;
}
