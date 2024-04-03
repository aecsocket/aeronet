//! Client-side traits and items.

#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{error::Error, fmt::Debug, hash::Hash, time::Duration};

use derivative::Derivative;

use crate::protocol::TransportProtocol;

/// Allows connecting to a server and transporting data between this client and
/// the server.
///
/// See the [crate-level documentation](crate).
pub trait ClientTransport<P: TransportProtocol>: Sized {
    /// Error type of operations performed on this transport.
    type Error: Error + Send + Sync;

    /// Client state when it is in [`ClientState::Connecting`].
    type Connecting<'t>
    where
        Self: 't;

    /// Client state when it is in [`ClientState::Connected`].
    type Connected<'t>
    where
        Self: 't;

    /// Key uniquely identifying a sent message.
    ///
    /// If the implementation does not support getting the state of a sent
    /// message, this may be `()`.
    ///
    /// See [`ClientTransport::send`].
    type MessageKey: Send + Sync + Debug + Clone + PartialEq + Eq + Hash;

    /// Gets the current state of this client.
    ///
    /// This can be used to access statistics on the connection, such as number
    /// of bytes sent or [round-trip time], if the transport exposes it.
    ///
    /// [round-trip time]: crate::stats::Rtt
    fn state(&self) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>>;

    /// Attempts to send a message to the currently connected server.
    ///
    /// This returns a key uniquely identifying the sent message. This can be
    /// used to query the state of the message, such as if it was acknowledged
    /// by the peer, if the implementation supports it.
    ///
    /// The implementation may choose to buffer the message before sending it
    /// out - therefore, you should always call [`ClientTransport::flush`] to
    /// ensure that all buffered messages are sent, e.g. at the end of each app
    /// tick.
    ///
    /// # Errors
    ///
    /// Errors if the transport failed to *attempt to* send the message, e.g.
    /// if it is not connected to a server.
    ///
    /// If a transmission error occurs later after this function's scope has
    /// finished, then this will still return [`Ok`].
    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<Self::MessageKey, Self::Error>;

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

    /// Updates the internal state of this transport by receiving messages from
    /// peers, returning the events that it emitted while updating.
    ///
    /// This should be called in your app's main update loop, passing in the
    /// time elapsed since the last `poll` call.
    ///
    /// If this emits an event which changes the transport's state, then after
    /// this function, the transport is guaranteed to be in this new state. Only
    /// up to one state-changing event will be produced by this function per
    /// function call.
    fn poll(&mut self, delta_time: Duration) -> impl Iterator<Item = ClientEvent<P, Self>>;
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

pub type ClientStateFor<'t, P, T> = ClientState<
    <T as ClientTransport<P>>::Connecting<'t>,
    <T as ClientTransport<P>>::Connected<'t>,
>;

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
    Debug(bound = "T::Error: Debug, P::S2C: Debug"),
    Clone(bound = "T::Error: Clone, P::S2C: Clone")
)]
pub enum ClientEvent<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
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
        error: T::Error,
    },

    // info
    /// The client received a message from the server.
    Recv {
        /// The message received.
        msg: P::S2C,
    },
    /// The peer acknowledged that they have fully received a message sent by
    /// us.
    Ack {
        /// Key of the sent message, obtained by [`ClientTransport::send`].
        msg_key: T::MessageKey,
    },
    /// The client has experienced a non-fatal connection error.
    ///
    /// The connection is still active until [`ClientEvent::Disconnected`] is
    /// emitted.
    ConnectionError {
        /// Error which occurred.
        error: T::Error,
    },
}
