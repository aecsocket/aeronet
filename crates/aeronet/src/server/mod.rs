//! Server-side traits and items.

#[cfg(feature = "bevy")]
mod bevy;

#[cfg(feature = "bevy")]
pub use bevy::*;
use web_time::Duration;

use std::{error::Error, fmt::Debug, hash::Hash};

use bytes::Bytes;
use derivative::Derivative;

use crate::{
    client::{ClientState, DisconnectReason},
    lane::LaneIndex,
};

/// Allows listening to client connections and transporting data between this
/// server and connected clients.
///
/// See the [crate-level documentation](crate).
pub trait ServerTransport {
    /// Error type of operations performed on this transport.
    type Error: Error + Send + Sync;

    /// Server state when it is in [`ServerState::Opening`].
    type Opening<'this>
    where
        Self: 'this;

    /// Server state when it is in [`ServerState::Open`].
    type Open<'this>
    where
        Self: 'this;

    /// A client's state when it is in [`ClientState::Connecting`].
    type Connecting<'this>
    where
        Self: 'this;

    /// A client's state when it is in [`ClientState::Connected`].
    type Connected<'this>
    where
        Self: 'this;

    /// Key uniquely identifying a client.
    ///
    /// If the same physical client (i.e. the same user ID or network socket)
    /// disconnects and reconnects, this must be treated as a new client, and
    /// the client must be given a new key.
    type ClientKey: Send + Sync + Debug + Clone + PartialEq + Eq + Hash;

    /// Key uniquely identifying a sent message.
    ///
    /// If the implementation does not support getting the state of a sent
    /// message, this may be `()`.
    ///
    /// See [`ServerTransport::send`].
    type MessageKey: Send + Sync + Debug + Clone + PartialEq + Eq + Hash;

    /// Gets the current state of this server.
    ///
    /// See [`ServerState`].
    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>>;

    /// Gets the current state of a client.
    ///
    /// If the client does not exist, [`ClientState::Disconnected`] is returned.
    ///
    /// See [`ClientState`].
    fn client_state(
        &self,
        client_key: Self::ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>>;

    /// Iterator over the keys of all clients currently recognized by this
    /// server.
    ///
    /// There is no guarantee about what state each client in this iterator is
    /// in, it's just guaranteed that the server is tracking some sort of state
    /// about it.
    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_;

    /// Updates the internal state of this transport by receiving messages from
    /// peers, returning the events that it emitted while updating.
    ///
    /// This should be called in your app's main update loop, passing in the
    /// time elapsed since the last `poll` call.
    ///
    /// If this emits an event which changes the transport's state, then after
    /// this function, the transport is guaranteed to be in this new state. The
    /// transport may emit an arbitrary number of state-changing events.
    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ServerEvent<Self>>;

    /// Attempts to send a message along a specific lane to a connected client.
    ///
    /// This returns a key uniquely identifying the sent message. This can be
    /// used to query the state of the message, such as if it was acknowledged
    /// by the peer, if the implementation supports it.
    ///
    /// The implementation may choose to buffer the message before sending it
    /// out - therefore, you should call [`ServerTransport::flush`] after you
    /// have sent all of the messages you wish to send. You can run this at the
    /// end of each app tick.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* send the message, e.g.
    /// if the server is not open, or if the client is not connected.
    ///
    /// If a transmission error occurs later after this function's scope has
    /// finished, then this will still return [`Ok`].
    fn send(
        &mut self,
        client_key: Self::ClientKey,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error>;

    /// Sends all messages previously buffered by [`ServerTransport::send`] to
    /// peers.
    ///
    /// Note that implementations may choose to send messages immediately
    /// instead of buffering them. In this case, flushing will be a no-op.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* flush messages, e.g. if
    /// the transport is closed.
    ///
    /// If a transmission error occurs later after this function's scope has
    /// finished, then this will still return [`Ok`].
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// Forces a client to disconnect from this server.
    ///
    /// This is guaranteed to disconnect the client as quickly as possible, and
    /// will make a best-effort attempt to inform the other side of the
    /// user-provided disconnection reason, however it is not guaranteed that
    /// this reason will be communicated.
    ///
    /// The implementation may place limitations on the `reason`, e.g. a maximum
    /// byte length.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* disconnect the client,
    /// e.g. if the server already knows that the client is disconnected.
    fn disconnect(
        &mut self,
        client_key: Self::ClientKey,
        reason: impl Into<String>,
    ) -> Result<(), Self::Error>;

    /// Closes this server, stopping all current connections and disallowing any
    /// new connections.
    ///
    /// All clients currently connected will be disconnected with the given
    /// reason. See [`ServerTransport::disconnect`] on how this reason will be
    /// handled.
    ///
    /// # Errors
    ///
    /// Errors if the transport is already closed.
    fn close(&mut self, reason: impl Into<String>) -> Result<(), Self::Error>;

    /// Gets the default disconnect-on-drop reason if one is set.
    ///
    /// When this server is dropped without being explicitly closed, and the
    /// default disconnect reason is [`Some`], this server will disconnect all
    /// of its currently connected clients with this reason.
    ///
    /// See [`ServerTransport::disconnect`] for how the disconnect reason is
    /// handled.
    fn default_disconnect_reason(&self) -> Option<&str>;

    /// Sets the default disconnect-on-drop reason.
    fn set_default_disconnect_reason(&mut self, reason: impl Into<String>);

    /// Unsets the default disconnect-on-drop reason.
    fn unset_default_disconnect_reason(&mut self);

    /// Returns `self` with a modified disconnect-on-drop reason.
    #[must_use]
    fn with_default_disconnect_reason(mut self, reason: impl Into<String>) -> Self
    where
        Self: Sized,
    {
        self.set_default_disconnect_reason(reason);
        self
    }
}

/// Implementation-specific state details of a [`ServerTransport`].
///
/// This can be used to access info such as the server's [local address], if the
/// transport exposes it.
///
/// [local address]: crate::stats::LocalAddr
#[derive(Debug, Clone, Default)]
pub enum ServerState<A, B> {
    /// Not listening to client connections, and making no attempts to start
    /// listening.
    #[default]
    Closed,
    /// Attempting to start listening for client connections, but is not
    /// ready to accept connections yet.
    Opening(A),
    /// Ready to accept client connections and transport data between clients.
    Open(B),
}

/// Shortcut for getting the [`ServerState`] type used by a [`ServerTransport`].
pub type ServerStateFor<'t, T> =
    ServerState<<T as ServerTransport>::Opening<'t>, <T as ServerTransport>::Open<'t>>;

/// Shortcut for getting the [`ClientState`] type used by a [`ServerTransport`].
pub type ClientStateFor<'t, T> =
    ClientState<<T as ServerTransport>::Connecting<'t>, <T as ServerTransport>::Connected<'t>>;

impl<A, B> ServerState<A, B> {
    /// Gets if this is a [`ServerState::Closed`].
    ///
    /// This should be used to determine if the user is allowed to start a new
    /// server.
    pub const fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Gets if this is a [`ServerState::Opening`].
    pub const fn is_opening(&self) -> bool {
        matches!(self, Self::Opening(_))
    }

    /// Gets if this is a [`ServerState::Open`].
    ///
    /// This should be used to determine if the app is ready to server clients.
    pub const fn is_open(&self) -> bool {
        matches!(self, Self::Open(_))
    }

    /// Converts from `&ServerState<A, B>` to `ServerState<&A, &B>`.
    ///
    /// Analogous to [`Option::as_ref`].
    pub const fn as_ref(&self) -> ServerState<&A, &B> {
        match self {
            Self::Closed => ServerState::Closed,
            Self::Opening(a) => ServerState::Opening(a),
            Self::Open(b) => ServerState::Open(b),
        }
    }

    /// Converts from `ServerState<A, B>` to `ServerState<A2, B2>`.
    pub fn map<A2, B2>(
        self,
        fa: impl FnOnce(A) -> A2,
        fb: impl FnOnce(B) -> B2,
    ) -> ServerState<A2, B2> {
        match self {
            Self::Closed => ServerState::Closed,
            Self::Opening(a) => ServerState::Opening(fa(a)),
            Self::Open(b) => ServerState::Open(fb(b)),
        }
    }
}

/// Event emitted by a [`ServerTransport`].
#[derive(Derivative)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub enum ServerEvent<T: ServerTransport + ?Sized> {
    // server state
    /// The server has completed setup and is ready to accept client
    /// connections, changing state to [`ServerState::Open`].
    Opened,
    /// The server can no longer handle client connections, changing state to
    /// [`ServerState::Closed`].
    Closed {
        /// Why the server closed.
        reason: CloseReason<T::Error>,
    },

    // client state
    /// A remote client has requested to connect to this server.
    ///
    /// The client has been given a key, and the server is trying to establish
    /// communication with this client, but messages cannot be transported yet.
    ///
    /// For a given client, the transport is guaranteed to emit this event
    /// before [`ServerEvent::Connected`].
    Connecting {
        /// Key of the client.
        client_key: T::ClientKey,
    },
    /// A remote client has fully established a connection to this server,
    /// changing the client's state to [`ClientState::Connected`].
    ///
    /// After this event, you can start sending messages to and receiving
    /// messages from the client.
    Connected {
        /// Key of the client.
        client_key: T::ClientKey,
    },
    /// A remote client has unrecoverably lost connection from this server.
    ///
    /// This is emitted for *any* reason that the client may be disconnected,
    /// including user code calling [`ServerTransport::disconnect`], therefore
    /// this may be used as a signal to tear down the client's state.
    Disconnected {
        /// Key of the client.
        client_key: T::ClientKey,
        /// Why the client lost connection.
        reason: DisconnectReason<T::Error>,
    },

    // messages
    /// The server received a message from a remote client.
    Recv {
        /// Key of the client.
        client_key: T::ClientKey,
        /// The message received.
        msg: Bytes,
        /// Lane on which the message was received.
        lane: LaneIndex,
    },
    /// A client acknowledged that they have fully received a message sent by
    /// us.
    Ack {
        /// Key of the client.
        client_key: T::ClientKey,
        /// Key of the sent message, obtained by [`ServerTransport::send`].
        msg_key: T::MessageKey,
    },
    /// Our server believes that an unreliable message sent to a client has
    /// probably been lost in transit.
    ///
    /// An implementation is allowed to not emit this event if it is not able
    /// to.
    Nack {
        /// Key of the client.
        client_key: T::ClientKey,
        /// Key of the sent message, obtained by [`ServerTransport::send`].
        msg_key: T::MessageKey,
    },
}

impl<Error, ClientKey, MessageKey, T> ServerEvent<T>
where
    T: ServerTransport<Error = Error, ClientKey = ClientKey, MessageKey = MessageKey>,
{
    /// Remaps this `ServerEvent<T>` into a `ServerEvent<R>` where `T` and `R`
    /// are [`ServerTransport`]s which share the same `Error`, `ClientKey`, and
    /// `MessageKey` types.
    pub fn remap<R>(self) -> ServerEvent<R>
    where
        R: ServerTransport<Error = Error, ClientKey = ClientKey, MessageKey = MessageKey>,
    {
        match self {
            Self::Opened => ServerEvent::Opened,
            Self::Closed { reason } => ServerEvent::Closed { reason },
            Self::Connecting { client_key } => ServerEvent::Connecting { client_key },
            Self::Connected { client_key } => ServerEvent::Connected { client_key },
            Self::Disconnected { client_key, reason } => {
                ServerEvent::Disconnected { client_key, reason }
            }
            Self::Recv {
                client_key,
                msg,
                lane,
            } => ServerEvent::Recv {
                client_key,
                msg,
                lane,
            },
            Self::Ack {
                client_key,
                msg_key,
            } => ServerEvent::Ack {
                client_key,
                msg_key,
            },
            Self::Nack {
                client_key,
                msg_key,
            } => ServerEvent::Nack {
                client_key,
                msg_key,
            },
        }
    }
}

/// Why a [`ServerTransport`] was closed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CloseReason<E> {
    /// Server was closed by user code, via a call to
    /// [`ServerTransport::close`].
    ///
    /// The closing reason is provided.
    #[error("disconnected locally: {0}")]
    Local(String),
    /// Encountered a fatal error.
    ///
    /// This is mostly raised while the server is still opening if there is an
    /// error preventing the server from receiving client connections, e.g. a
    /// port that the server wanted to use is already in use by another process.
    ///
    /// While the server is open, errors usually should not tear down the entire
    /// server, just the connection of the specific client who caused the error.
    #[error("connection error")]
    Error(
        #[source]
        #[from]
        E,
    ),
}
