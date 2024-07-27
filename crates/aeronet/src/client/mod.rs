//! Client-side traits and items.

#[cfg(feature = "bevy")]
mod bevy;

#[cfg(feature = "bevy")]
pub use bevy::*;
use web_time::Duration;

use std::{error::Error, fmt::Debug, hash::Hash};

use bytes::Bytes;
use derivative::Derivative;

use crate::lane::LaneIndex;

/// Allows connecting to a server and transporting data between this client and
/// the server.
///
/// See the [crate-level documentation](crate).
pub trait ClientTransport {
    /// Error type for operations performed on this transport.
    type Error: Error + Send + Sync;

    /// Client state when it is in [`ClientState::Connecting`].
    type Connecting<'this>
    where
        Self: 'this;

    /// Client state when it is in [`ClientState::Connected`].
    type Connected<'this>
    where
        Self: 'this;

    /// Key uniquely identifying a sent message.
    ///
    /// If the implementation does not support getting the state of a sent
    /// message, this may be `()`.
    ///
    /// See [`ClientTransport::send`].
    type MessageKey: Send + Sync + Debug + Clone + PartialEq + Eq + Hash;

    /// Gets the current state of this client.
    ///
    /// See [`ClientState`].
    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>>;

    /// Updates the internal state of this transport by receiving messages from
    /// peers, returning the events that it emitted while updating.
    ///
    /// This should be called in your app's main update loop, passing in the
    /// time elapsed since the last `poll` call.
    ///
    /// If this emits an event which changes the transport's state, then after
    /// this function, the transport is guaranteed to be in this new state. The
    /// transport may emit an arbitrary number of state-changing events.
    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<Self>>;

    /// Attempts to send a message along a specific lane to the currently
    /// connected server.
    ///
    /// This returns a key uniquely identifying the sent message. This can be
    /// used to query the state of the message, such as if it was acknowledged
    /// by the peer, if the implementation supports it.
    ///
    /// The implementation may choose to buffer the message before sending it
    /// out - therefore, you should call [`ClientTransport::flush`] after you
    /// have sent all of the messages you wish to send. You can run this at the
    /// end of each app tick.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* send the message, e.g.
    /// if it is not connected to a server.
    ///
    /// If a transmission error occurs later after this function's scope has
    /// finished, then this will still return [`Ok`].
    fn send(
        &mut self,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<Self::MessageKey, Self::Error>;

    /// Sends all messages previously buffered by [`ClientTransport::send`] to
    /// peers.
    ///
    /// Note that implementations may choose to send messages immediately
    /// instead of buffering them. In this case, flushing will be a no-op.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* flush messages, e.g. if
    /// the transport is not connected.
    ///
    /// If a transmission error occurs later after this function's scope has
    /// finished, then this will still return [`Ok`].
    fn flush(&mut self) -> Result<(), Self::Error>;

    /// Disconnects this client from its currently connected server.
    ///
    /// This does *not* guarantee any graceful shutdown of connections. If you
    /// want this to be handled gracefully, you must implement a mechanism for
    /// this yourself.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* disconnect, e.g. if the
    /// transport has not been connected yet.
    fn disconnect(&mut self) -> Result<(), Self::Error>;
}

/// Implementation-specific state details of a [`ClientTransport`].
///
/// This can be used to access statistics on the connection, such as number
/// of bytes sent or [round-trip time], if the transport exposes it.
///
/// [round-trip time]: crate::stats::Rtt
#[derive(Debug, Clone, Default)]
pub enum ClientState<A, B> {
    /// Not connected to a server, and making no attempts to connect to one.
    #[default]
    Disconnected,
    /// Attempting to establish a connection to a server, but is not ready for
    /// transporting data yet.
    Connecting(A),
    /// Ready to transport data to/from a server.
    Connected(B),
}

/// Shortcut for getting the [`ClientState`] type used by a [`ClientTransport`].
pub type ClientStateFor<'t, T> =
    ClientState<<T as ClientTransport>::Connecting<'t>, <T as ClientTransport>::Connected<'t>>;

impl<A, B> ClientState<A, B> {
    /// Gets if this is a [`ClientState::Disconnected`].
    ///
    /// This should be used to determine if the user is allowed to start
    /// connecting to a server.
    pub const fn is_disconnected(&self) -> bool {
        matches!(self, Self::Disconnected)
    }

    /// Gets if this is a [`ClientState::Connecting`].
    pub const fn is_connecting(&self) -> bool {
        matches!(self, Self::Connecting(_))
    }

    /// Gets if this is a [`ClientState::Connected`].
    ///
    /// This should be used to determine if the user is allowed to send messages
    /// to the server.
    pub const fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    /// Converts from `&ClientState<A, B>` to `ClientState<&A, &B>`.
    ///
    /// Analogous to [`Option::as_ref`].
    pub const fn as_ref(&self) -> ClientState<&A, &B> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(a) => ClientState::Connecting(a),
            Self::Connected(b) => ClientState::Connected(b),
        }
    }

    /// Converts from `ClientState<A, B>` to `ClientState<A2, B2>`.
    pub fn map<A2, B2>(
        self,
        fa: impl FnOnce(A) -> A2,
        fb: impl FnOnce(B) -> B2,
    ) -> ClientState<A2, B2> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(a) => ClientState::Connecting(fa(a)),
            Self::Connected(b) => ClientState::Connected(fb(b)),
        }
    }
}

/// Event emitted by a [`ClientTransport`].
#[derive(Derivative)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub enum ClientEvent<T: ClientTransport + ?Sized> {
    // state
    /// The client has fully established a connection to the server,
    /// changing state to [`ClientState::Connected`].
    ///
    /// After this event, you can run your game initialization logic such as
    /// receiving the initial world state and e.g. showing a spawn screen.
    Connected,
    /// The client has unrecoverably lost connection from its previously
    /// connected server, changing state to [`ClientState::Disconnected`].
    ///
    /// This event is not raised when the client side forces a disconnect.
    Disconnected {
        /// Why the client lost connection.
        error: T::Error,
    },

    /// The client received a message from the server.
    Recv {
        /// The message received.
        msg: Bytes,
        /// Lane on which the message was received.
        lane: LaneIndex,
    },
    /// The peer acknowledged that they have fully received a message sent by
    /// us.
    Ack {
        /// Key of the sent message, obtained by [`ClientTransport::send`].
        msg_key: T::MessageKey,
    },
    /// Our client believes that an unreliable message has probably been lost
    /// in transit.
    ///
    /// An implementation is allowed to not emit this event if it is not able
    /// to.
    Nack {
        /// Key of the sent message, obtained by [`ClientTransport::send`].
        msg_key: T::MessageKey,
    },
}

impl<Error, MessageKey, T> ClientEvent<T>
where
    T: ClientTransport<Error = Error, MessageKey = MessageKey>,
{
    /// Remaps this `ClientEvent<T>` into a `ClientEvent<R>` where `T` and `R`
    /// are [`ClientTransport`]s which share the same `Error` and `MessageKey`
    /// types.
    pub fn remap<R>(self) -> ClientEvent<R>
    where
        R: ClientTransport<Error = Error, MessageKey = MessageKey>,
    {
        match self {
            Self::Connected => ClientEvent::Connected,
            Self::Disconnected { error } => ClientEvent::Disconnected { error },
            Self::Recv { msg, lane } => ClientEvent::Recv { msg, lane },
            Self::Ack { msg_key } => ClientEvent::Ack { msg_key },
            Self::Nack { msg_key } => ClientEvent::Nack { msg_key },
        }
    }
}
