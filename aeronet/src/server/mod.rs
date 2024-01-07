#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use std::{error::Error, fmt::Debug, time::Instant};

use derivative::Derivative;

use crate::{ClientKey, ClientState, TransportProtocol};

pub trait ServerTransport<P>
where
    P: TransportProtocol,
{
    type Error: Error + Send + Sync + 'static;

    type OpeningInfo: Send + Sync + 'static;

    type OpenInfo: Send + Sync + 'static;

    type ConnectingInfo: Send + Sync + 'static;

    type ConnectedInfo: Send + Sync + 'static;

    fn state(&self) -> ServerState<Self::OpeningInfo, Self::OpenInfo>;

    fn client_state(
        &self,
        client: ClientKey,
    ) -> ClientState<Self::ConnectingInfo, Self::ConnectedInfo>;

    fn clients(&self) -> impl Iterator<Item = ClientKey> + '_;

    fn send(&mut self, client: ClientKey, msg: impl Into<P::S2C>) -> Result<(), Self::Error>;

    fn disconnect(&mut self, client: ClientKey) -> Result<(), Self::Error>;

    fn update(
        &mut self,
    ) -> impl Iterator<Item = ServerEvent<P, Self::ConnectingInfo, Self::ConnectedInfo, Self::Error>> + '_;
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
    Debug(bound = "P::C2S: Debug, A: Debug, B: Debug, E: Debug"),
    Clone(bound = "P::C2S: Clone, A: Clone, B: Clone, E: Clone")
)]
pub enum ServerEvent<P, A, B, E>
where
    P: TransportProtocol,
    E: Error,
{
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
        /// Info on the connection.
        ///
        /// This is the same data as held by [`ClientState::Connecting`].
        info: A,
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
        /// Info on the connection.
        ///
        /// This is the same data as held by [`ClientState::Connected`].
        info: B,
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
        /// When the message was first received.
        ///
        /// Since the transport may use e.g. an async task to receive data, the
        /// time at which the message was polled using
        /// [`ServerTransport::update`] is not necessarily when the app first
        /// became aware of this message.
        at: Instant,
    },
}
