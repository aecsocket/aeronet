//! Client-side traits and items.
//!
//! See the [crate-level docs](crate).

#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{
    error::Error,
    fmt::{self, Debug},
};

use derivative::Derivative;

use crate::{MessageState, TransportProtocol};

/// Allows connecting to a server and transporting data between this client and
/// the server.
///
/// See the [crate-level docs](crate).
pub trait ClientTransport<P: TransportProtocol> {
    /// Error type of operations performed on this transport.
    type Error: Error + Send + Sync;

    /// Info on this client when it is in [`ClientState::Connecting`].
    type ConnectingInfo;

    /// Info on this client when it is in [`ClientState::Connected`].
    type ConnectedInfo;

    /// Key uniquely identifying a sent message.
    ///
    /// If the implementation does not support getting the state of a sent
    /// message, this may be `()`.
    ///
    /// See [`ClientTransport::send`].
    type MessageKey: Send + Sync;

    /// Gets the current state of this client.
    ///
    /// This can be used to access statistics on the connection, such as number
    /// of bytes sent or [round-trip time](crate::Rtt), if the transport exposes
    /// it.
    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo>;

    /// Gets the current state of a message sent via [`ClientTransport::send`].
    ///
    /// If the transport does not support this, or the message key does not
    /// represent a valid sent message, this returns [`None`].
    fn message_state(&self, msg_key: Self::MessageKey) -> Option<MessageState>;

    /// Attempts to send a message to the currently connected server.
    ///
    /// This returns a key uniquely identifying the sent message. This can be
    /// used to query the state of the message, such as if it was acknowledged
    /// by the peer, if the implementation supports it.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* send the message, e.g.
    /// if it is not connected to a server. If a transmission error occurs later
    /// after this function's scope has finished, then this will still return
    /// [`Ok`].
    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error>;

    /// Updates the internal state of this transport, returning an iterator over
    /// the events that it emitted while updating.
    ///
    /// This should be called in your app's main update loop.
    ///
    /// If this emits an event which changes the transport's state, then after
    /// this function, the transport is guaranteed to be in this new state. Only
    /// up to one state-changing event will be produced by this function per
    /// function call.
    fn poll(&mut self) -> impl Iterator<Item = ClientEvent<P, Self::Error, Self::MessageKey>>;
}

slotmap::new_key_type! {
    /// Key identifying a unique client connected to a server.
    ///
    /// This key is unique for each individual session that a server accepts,
    /// even if a new client takes the slot/allocation of a previous client. To
    /// enforce this behavior, the key is implemented as a
    /// [`slotmap::new_key_type`] and intended to be used in a
    /// [`slotmap::SlotMap`].
    ///
    /// New sessions coming from the same physical client (e.g. the same socket
    /// address) get different keys.
    pub struct ClientKey;
}

impl fmt::Display for ClientKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// State of a [`ClientTransport`].
///
/// See [`ClientTransport::state`].
#[derive(Debug, Clone)]
pub enum ClientState<A, B> {
    /// Not connected to a server, and making no attempts to connect to one.
    Disconnected,
    /// Attempting to establish a connection to a server, but is not ready for
    /// transporting data yet.
    Connecting(A),
    /// Ready to transport data to/from a server.
    Connected(B),
}

impl<A, B> ClientState<A, B> {
    /// Gets if this is a [`ClientState::Disconnected`].
    ///
    /// This should be used to determine if the user is allowed to start
    /// connecting to a server.
    pub fn is_disconnected(&self) -> bool {
        matches!(self, Self::Disconnected)
    }

    /// Gets if this is a [`ClientState::Connecting`].
    pub fn is_connecting(&self) -> bool {
        matches!(self, Self::Connecting(_))
    }

    /// Gets if this is a [`ClientState::Connected`].
    ///
    /// This should be used to determine if the user is allowed to send messages
    /// to the server.
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }
}

/// Event emitted by a [`ClientTransport`].
#[derive(Derivative)]
#[derivative(
    Debug(bound = "P::S2C: Debug, E: Debug, M: Debug"),
    Clone(bound = "P::S2C: Clone, E: Clone, M: Clone")
)]
pub enum ClientEvent<P: TransportProtocol, E, M> {
    // state
    /// The client has fully established a connection to the server.
    ///
    /// This event can be followed by [`ClientEvent::Recv`] or
    /// [`ClientEvent::Disconnected`].
    ///
    /// After this event, you can run your game initialization logic such as
    /// receiving the initial world state and e.g. showing a spawn screen.
    Connected,
    /// The client has unrecoverably lost connection from its previously
    /// connected server.
    ///
    /// This event is not raised when the app invokes a disconnect.
    Disconnected {
        /// Why the client lost connection.
        reason: E,
    },

    // messages
    /// The client received a message from the server.
    Recv {
        /// The message received.
        msg: P::S2C,
    },
    /// The peer acknowledged that they have fully received a message sent by
    /// us.
    Ack {
        /// Key of the sent message, obtained by [`ClientTransport::send`].
        msg_key: M,
    },
}
