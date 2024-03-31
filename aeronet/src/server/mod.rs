//! Server-side traits and items.

#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{error::Error, fmt::Debug, hash::Hash};

use derivative::Derivative;

use crate::{client::ClientState, protocol::TransportProtocol};

/// Allows listening to client connections and transporting data between this
/// server and connected clients.
///
/// See the [crate-level documentation](crate).
pub trait ServerTransport<P: TransportProtocol> {
    /// Error type of operations performed on this transport.
    type Error: Error + Send + Sync;

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

    /// Key uniquely identifying a client.
    ///
    /// If a physical client disconnects and connects, a new key must be used
    /// to represent the new session.
    type ClientKey: Send + Sync + Debug + Clone + PartialEq + Eq + Hash;

    /// Key uniquely identifying a sent message.
    ///
    /// If the implementation does not support getting the state of a sent
    /// message, this may be `()`.
    ///
    /// See [`ServerTransport::send`].
    type MessageKey: Send + Sync + Debug + Clone + PartialEq + Eq + Hash;

    /// Reads the current state of this server.
    ///
    /// This can be used to access info such as the server's [local address],
    /// if the transport exposes it.
    ///
    /// [local address]: crate::stats::LocalAddr
    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo>;

    /// Reads the current state of a client.
    ///
    /// This can be used to access statistics on the connection, such as number
    /// of bytes sent or [round-trip time], if the transport exposes it.
    ///
    /// If the client does not exist, [`ClientState::Disconnected`] is returned.
    ///
    /// [round-trip time]: crate::stats::Rtt
    fn client_state(
        &self,
        client_key: Self::ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo>;

    /// Iterator over the keys of all clients currently recognized by this
    /// server.
    ///
    /// There is no guarantee about what state each client in this iterator is
    /// in, it's just guaranteed that the server is tracking some sort of state
    /// about it.
    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_;

    /// Attempts to send a message to a connected client.
    ///
    /// This returns a key uniquely identifying the sent message. This can be
    /// used to query the state of the message, such as if it was acknowledged
    /// by the peer, if the implementation supports it.
    ///
    /// The implementation may choose to buffer the message before sending it
    /// out - therefore, you should always call [`ServerTransport::flush`] to
    /// ensure that all buffered messages are sent, e.g. at the end of each app
    /// tick.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* send the message, e.g.
    /// if the server is not open, or if the client is not connected. If a
    /// transmission error occurs later after this function's scope has
    /// finished, then this will still return [`Ok`].
    fn send(
        &mut self,
        client_key: Self::ClientKey,
        msg: impl Into<P::S2C>,
    ) -> Result<Self::MessageKey, Self::Error>;

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
    fn disconnect(&mut self, client_key: Self::ClientKey) -> Result<(), Self::Error>;

    /// Updates the internal state of this transport by receiving messages from
    /// peers, returning the events that it emitted while updating.
    ///
    /// This should be called in your app's main update loop.
    ///
    /// If this emits an event which changes the transport's state, then after
    /// this function, the transport is guaranteed to be in this new state. Only
    /// up to one state-changing event will be produced by this function per
    /// function call.
    fn poll(
        &mut self,
    ) -> impl Iterator<Item = ServerEvent<P, Self::Error, Self::ClientKey, Self::MessageKey>>;

    /// Sends all messages previously buffered by [`ServerTransport::send`] to
    /// peers.
    ///
    /// If this transport is not connected, this will return [`Ok`].
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* flush messages, e.g. if
    /// the connection has already been closed.
    ///
    /// If a transmission error occurs later after this function's scope has
    /// finished, then this will still return [`Ok`].
    fn flush(&mut self) -> Result<(), Self::Error>;
}

/// State of a [`ServerTransport`].
///
/// See [`ServerTransport::state`].
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
    Debug(bound = "P::C2S: Debug, E: Debug, C: Debug, M: Debug"),
    Clone(bound = "P::C2S: Clone, E: Clone, C: Clone, M: Clone")
)]
pub enum ServerEvent<P: TransportProtocol, E, C, M> {
    // server state
    /// The server has completed setup and is ready to accept client
    /// connections, changing state to [`ServerState::Open`].
    Opened,
    /// The server can no longer handle client connections, changing state to
    /// [`ServerState::Closed`].
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
        client_key: C,
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
        client_key: C,
    },
    /// A remote client has unrecoverably lost connection from this server.
    ///
    /// This event is not raised when the server forces a client to disconnect.
    Disconnected {
        /// Key of the client.
        client_key: C,
        /// Why the client lost connection.
        reason: E,
    },

    // messages
    /// The server received a message from a remote client.
    Recv {
        /// Key of the client.
        client_key: C,
        /// The message received.
        msg: P::C2S,
    },
    /// A client acknowledged that they have fully received a message sent by
    /// us.
    Ack {
        /// Key of the client.
        client_key: C,
        /// Key of the sent message, obtained by [`ServerTransport::send`].
        msg_key: M,
    },
}

/// Type alias for [`ServerEvent`] which takes a [`TransportProtocol`] and a
/// [`ServerTransport`] accepting that protocol.
pub type ServerEventFor<P, T> = ServerEvent<
    P,
    <T as ServerTransport<P>>::Error,
    <T as ServerTransport<P>>::ClientKey,
    <T as ServerTransport<P>>::MessageKey,
>;
