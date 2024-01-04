#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{
    error::Error,
    fmt::{self, Debug},
    time::Instant,
};

use derivative::Derivative;

use crate::TransportProtocol;

pub trait ClientTransport<P>
where
    P: TransportProtocol,
{
    type Error: Error + Send + Sync + 'static;

    type ConnectingInfo: Send + Sync + 'static;

    type ConnectedInfo: Send + Sync + 'static;

    fn state(&self) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo>;

    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error>;

    /// If this emits an event which changes the transport's state, then after
    /// this call, the transport will be in this new state.
    fn update(
        &mut self,
    ) -> impl Iterator<Item = ClientEvent<P, Self::ConnectedInfo, Self::Error>> + '_;
}

slotmap::new_key_type! {
    /// Unique key identifying a client connected to a server.
    ///
    /// This key is unique for each individual session that a server accepts,
    /// even if a new client takes the slot/allocation of a previous client. To
    /// enforce this behavior, the key is implemented as a
    /// [`slotmap::new_key_type`] and intended to be used in a
    /// [`slotmap::SlotMap`].
    pub struct ClientKey;
}

impl fmt::Display for ClientKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// State of a [`ClientTransport`].
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
    pub fn is_disconnected(&self) -> bool {
        match self {
            Self::Disconnected => true,
            _ => false,
        }
    }

    pub fn is_connecting(&self) -> bool {
        match self {
            Self::Connecting(_) => true,
            _ => false,
        }
    }

    pub fn is_connected(&self) -> bool {
        match self {
            Self::Connected(_) => true,
            _ => false,
        }
    }
}

/// Event emitted by a [`ClientTransport`].
#[derive(Derivative)]
#[derivative(
    Debug(bound = "P::S2C: Debug, B: Debug, E: Debug"),
    Clone(bound = "P::S2C: Clone, B: Clone, E: Clone")
)]
pub enum ClientEvent<P, B, E>
where
    P: TransportProtocol,
    E: Error,
{
    // state
    /// The client has fully established a connection to the server.
    ///
    /// This event can be followed by [`ClientEvent::Recv`] or
    /// [`ClientEvent::Disconnected`].
    ///
    /// After this event, you can run your game initialization logic such as
    /// receiving the initial world state and e.g. showing a spawn screen.
    Connected {
        /// Info on the connection.
        ///
        /// This is the same data as held by [`ClientState::Connecting`].
        info: B,
    },
    /// The client has unrecoverably lost connection from its previously
    /// connected server.
    ///
    /// This event is not raised when the user invokes a disconnect.
    Disconnected {
        /// Why the client lost connection.
        reason: E,
    },

    // messages
    /// The client received a message from the server.
    Recv {
        /// The message received.
        msg: P::S2C,
        /// When the message was first received.
        ///
        /// Since the transport may use e.g. an async task to receive data, the
        /// time at which the message was polled using
        /// [`ClientTransport::update`] is not necessarily when the app first
        /// became aware of this message.
        ///
        /// This value can be used for calculating an estimate of the round-trip
        /// time.
        at: Instant,
    },
}
