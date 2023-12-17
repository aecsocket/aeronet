#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use crate::TransportProtocol;

/// Allows connecting to, and transporting messages to/from, a server.
///
/// See the [crate-level docs](crate).
pub trait TransportClient<P>
where
    P: TransportProtocol,
{
    /// Error returned from operations on this client.
    type Error: Send + Sync + 'static;

    /// Info on this client's connection status, returned by
    /// [`TransportClient::state`].
    type ConnectionInfo;

    /// Type of event raised by this client.
    ///
    /// This event type must be able to be potentially converted into a
    /// [`ClientEvent`]. If an event value cannot cleanly map to a single
    /// generic [`ClientEvent`], its [`Into`] impl must return [`None`].
    type Event: Into<Option<ClientEvent<P, Self>>>
    where
        Self: Sized;

    /// Gets the current state that this client is in.
    /// 
    /// This can be used to get information about the connection if it is
    /// connected, or to check if the client is connected at all.
    fn state(&self) -> ClientState<Self::ConnectionInfo>;

    /// Attempts to send a message to the connected server.
    ///
    /// # Errors
    ///
    /// If the client cannot even attempt to send a message to the server (e.g.
    /// if the client knows that it is already disconnected), this returns an
    /// error
    ///
    /// However, since errors may occur later in the transport process after
    /// this function has already returned (e.g. in an async task), this will
    /// return [`Ok`] if the client has successfully *tried* to send a message,
    /// not if the client actually *has* sent the message.
    ///
    /// If an error occurs later during the transport process, the server will
    /// forcefully disconnect the client and emit a
    /// [`ClientEvent::Disconnected`].
    fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), Self::Error>;

    /// Polls events and receives messages from this transport.
    ///
    /// This will consume messages and events if the client is connected. Events
    /// must be continuously received to allow this transport to do its internal
    /// work, so this should be run in the main loop of your program.
    ///
    /// This returns an iterator over the events received, which may be used in
    /// two ways:
    /// * used as-is, if you know the concrete type of the transport
    ///   * transports may expose their own event type, which allows you to
    ///     listen to specialized events
    /// * converted into a generic [`ClientEvent`] via its
    ///   `Into<Option<ClientEvent>>` implementation
    ///   * useful for generic code which must abstract over different transport
    ///     implementations
    ///   * a single event returned from this is not guaranteed to map to a
    ///     specific [`ClientEvent`]
    fn recv<'a>(&mut self) -> impl Iterator<Item = Self::Event> + 'a
    where
        Self: Sized;

    /// Forces this client to disconnect from its currently connected server.
    ///
    /// This function does not guarantee that the client is gracefully
    /// disconnected in any way, so you must use your own mechanism for graceful
    /// disconnection if you need this feature.
    ///
    /// Disconnecting using this function will also raise a
    /// [`ClientEvent::Disconnected`].
    ///
    /// # Errors
    ///
    /// If the client cannot even attempt to disconnect (e.g. if the client
    /// knows that it is already disconnected), this returns an error.
    fn disconnect(&mut self) -> Result<(), Self::Error>;
}

/// Current state of a [`TransportClient`].
#[derive(Debug, Clone, Default)]
pub enum ClientState<I> {
    /// The client is not connected to a server, and is making no attempts to
    /// connect.
    #[default]
    Disconnected,
    /// The client is attempting to connect to a server, but the connection has
    /// not been established yet.
    Connecting,
    /// The client has fully connected to a server, and information about the
    /// connection is now available.
    Connected(I),
}

/// Event raised by a [`TransportClient`].
#[derive(Debug, Clone)]
pub enum ClientEvent<P, T>
where
    P: TransportProtocol,
    T: TransportClient<P>,
{
    /// This client has started connecting to a server.
    /// 
    /// This may be followed by a [`ClientEvent::Connected`] or a
    /// [`ClientEvent::Disconnected`].
    Connecting,
    /// This client has fully connected to a server.
    ///
    /// Use this event to do setup logic, e.g. start loading the level.
    Connected,
    /// The server sent a message to this client.
    Recv {
        /// The message received.
        msg: P::S2C,
    },
    /// This client has lost connection from its previously connected server,
    /// which cannot be recovered from.
    ///
    /// Use this event to do teardown logic, e.g. changing state to the main
    /// menu.
    Disconnected {
        /// The reason why the client lost connection.
        cause: T::Error,
    },
}
