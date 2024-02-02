#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{error::Error, fmt::Debug};

use derivative::Derivative;

use crate::{ClientKey, ClientState, TransportProtocol};

/// Allows listening to client connections and transporting data between this
/// server and connected clients.
///
/// See the [crate-level docs](crate).
pub trait ServerTransport<P: TransportProtocol> {
    /// Error type of operations performed on this transport.
    type Error: Error + Send + Sync + 'static;

    /// Info on this server when it is in [`ServerState::Opening`].
    type OpeningInfo;

    /// Info on this server when it is in [`ServerState::Open`].
    type OpenInfo;

    /// Info on clients connected to this server when they are in
    /// [`ClientState::Connecting`].
    type ConnectingInfo;

    /// Info on clients connected to this server when they are in
    /// [`ClientState::Connected`].
    type ConnectedInfo;

    /// Reads the current state of this server.
    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo>;

    /// Reads the current state of a client.
    ///
    /// If the client does not exist, [`ClientState::Disconnected`] is returned.
    fn client_state(
        &self,
        client_key: ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo>;

    /// Iterator over the keys of all clients currently recognized by this
    /// server.
    ///
    /// There is no guarantee about what state each client in this iterator is
    /// in, it's just guaranteed that the server is tracking some sort of state
    /// about it.
    fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_;

    /// Attempts to send a message to a connected client.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* send the message, e.g.
    /// if the server is not open, or if the client is not connected. If a
    /// transmission error occurs later after this function's scope has
    /// finished, then this will still return [`Ok`].
    fn send(&mut self, client_key: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error>;

    /// Forces a client to disconnect from this server.
    ///
    /// This does *not* guarantee any graceful shutdown of the connection. If
    /// you want this to be handled gracefully, you must implement a mechanism
    /// for this yourself.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* disconnect the client,
    /// e.g. if the server already knows that the client is disconnected.
    fn disconnect(&mut self, client_key: ClientKey) -> Result<(), Self::Error>;

    /// Updates the internal state of this transport, returning an iterator over
    /// the events that it emitted while updating.
    ///
    /// This should be called in your app's main update loop.
    ///
    /// If this emits an event which changes the transport's state, then after
    /// this function, the transport is guaranteed to be in this new state. Only
    /// up to one state-changing event will be produced by this function per
    /// function call.
    fn update(&mut self) -> impl Iterator<Item = ServerEvent<P, Self::Error>>;
}

/// State of a [`ServerTransport`].
#[derive(Debug, Clone)]
pub enum ServerState<A, B> {
    /// Not listening to client connections, and making no attempts to start
    /// listening.
    Closed,
    /// Attempting to start listening for client connections, but is not
    /// ready to accept connections yet.
    Opening(A),
    /// Ready to accept client connections and transport data between clients.
    Open(B),
}

impl<A, B> ServerState<A, B> {
    /// Gets if this is a [`ServerState::Closed`].
    pub fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Gets if this is a [`ServerState::Opening`].
    pub fn is_opening(&self) -> bool {
        matches!(self, Self::Opening(_))
    }

    /// Gets if this is a [`ServerState::Open`].
    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open(_))
    }
}

/// Event emitted by a [`ServerTransport`].
#[derive(Derivative)]
#[derivative(
    Debug(bound = "P::C2S: Debug, E: Debug"),
    Clone(bound = "P::C2S: Clone, E: Clone")
)]
pub enum ServerEvent<P: TransportProtocol, E> {
    // server state
    /// The server has changed state to [`ServerState::Open`].
    Opened,
    /// The server has changed state to [`ServerState::Closed`].
    Closed {
        /// Why the server closed.
        reason: E,
    },

    // client state
    /// A remote client has requested to connect to this server.
    ///
    /// The client has been given a key, and the server is trying to establish
    /// communication with this client, but messages cannot be transported yet.
    ///
    /// This event can be followed by [`ServerEvent::Connected`] or
    /// [`ServerEvent::Disconnected`].
    Connecting {
        /// Key of the client.
        client: ClientKey,
    },
    /// A remote client has fully established a connection to this server.
    ///
    /// This event can be followed by [`ServerEvent::Recv`] or
    /// [`ServerEvent::Disconnected`].
    ///
    /// After this event, you can run your player initialization logic such as
    /// spawning the player's model in the world.
    Connected {
        /// Key of the client.
        client: ClientKey,
    },
    /// A remote client has unrecoverably lost connection from this server.
    ///
    /// This event is not raised when the server forces a client to disconnect.
    Disconnected {
        /// Key of the client.
        client: ClientKey,
        /// Why the client lost connection.
        reason: E,
    },

    // messages
    /// The server received a message from a remote client.
    Recv {
        /// Key of the client.
        client: ClientKey,
        /// The message received.
        msg: P::C2S,
    },
}
