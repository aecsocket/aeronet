//! The server-side functionality of this crate.

#[cfg(feature = "bevy")]
pub mod plugin;

use anyhow::Result;

use crate::{ClientId, DisconnectReason, TransportSettings};

/// Sent when a [`ServerTransport`] receives some sort of non-message event, such as a connection
/// or a disconnection.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerTransportEvent {
    /// A client connected to this server.
    Connect {
        /// The ID of the client who connected.
        client: ClientId,
    },
    /// A client lost connection to this server.
    Disconnect {
        /// The ID of the client who lost connection.
        client: ClientId,
        /// The reason why the connection was lost.
        reason: DisconnectReason,
    },
}

/// An error involving something about a server's connected client being invalid.
/// 
/// This may be used by [`ServerTransport`] implementations when a caller passes a [`ClientId`]
/// which is invalid or does not exist in some form.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerClientsError {
    /// Attempted to use a client who was already disconnected from the server.
    #[error("client disconnected")]
    Disconnected,
    /// Attempted to use a client who does not exist.
    /// 
    /// This may either be because the client's ID never existed, or because the client's ID was
    /// removed at some point.
    #[error("invalid client id")]
    Invalid,
}

/// The main server-side interface for transmitting data to, and receiving data from, multiple
/// connected clients.
/// 
/// The clients may be local or remote; the transport is just an interface to allow communication
/// between this server and them.
/// 
/// Consuming or sending data using this transport will never panic, however an error may be
/// emitted.
/// 
/// # Consuming data
/// 
/// The transport's functions must be called in a specific order to ensure it stays in an optimal
/// state. A suboptimal state is not *invalid*, however will result in transmission errors being
/// emitted that can easily be avoided by keeping the correct state (for example, if you attempt
/// to receive data from a client *before* checking if you've lost connection, then that would
/// probably emit an error).
/// 
/// During a single update cycle (e.g. a single frame in a game loop):
/// - call [`Self::disconnect`] on all the clients you want to kick off the server
/// - call [`Self::pop_event`] until all events are consumed
///   - if [`ServerTransportEvent::Connect`] is emitted, track that client ID somewhere so you can
///     use it to receive messages from that client later
///   - if [`ServerTransportEvent::Disconnect`] is emitted, immediately stop tracking that client ID:
///     you are no longer allowed to use it to receive messages from that client
/// - iterate through all your tracked client IDs and call [`Self::recv`] on them until either an
///   error or `Ok(None)` is returned
///   - do this *before* your main game logic (in Bevy, this would be in `PreUpdate`)
/// - iterate through all the messages you want to send and call [`Self::send`]
///   - do this *after* your main game logic (in Bevy, this would be in `PostUpdate`)
pub trait ServerTransport<S: TransportSettings> {
    /// Consumes a single event from this transport's event buffer.
    /// 
    /// This should be called in the order defined in the [trait docs](trait.ServerTransport.html).
    fn pop_event(&mut self) -> Option<ServerTransportEvent>;

    /// Consumes a single message from this transport's client-to-server message buffer for this
    /// particular client ID.
    /// 
    /// This should be called until either `Err` or `Ok(None)` are returned, at which point you
    /// should stop receiving any more data from this client.
    /// 
    /// This should be called in the order defined in the [trait docs](trait.ServerTransport.html).
    fn recv(&mut self, from: ClientId) -> Result<Option<S::C2S>>;

    /// Sends a single message to a client connected to this server.
    /// 
    /// This should be called in the order defined in the [trait docs](trait.ServerTransport.html).
    fn send(&mut self, to: ClientId, msg: impl Into<S::S2C>) -> Result<()>;

    /// Disconnects a client who is connected to this server.
    /// 
    /// This should be called in the order defined in the [trait docs](trait.ServerTransport.html).
    fn disconnect(&mut self, client: ClientId) -> Result<()>;
}
