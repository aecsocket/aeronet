#[cfg(feature = "bevy")]
mod plugin;

#[cfg(feature = "bevy")]
pub use plugin::*;

use crate::TransportProtocol;

/// Allows listening for client connections, and transporting messages to/from
/// the clients connected to this server.
///
/// See the [crate-level docs](crate).
pub trait TransportServer<P>
where
    P: TransportProtocol,
{
    /// Key type that this server uses to uniquely identify clients.
    type Client: Send + Sync + Clone + 'static;

    /// Error returned from operations on this server.
    type Error: Send + Sync + 'static;

    /// Info on a given client's connection status, returned by
    /// [`TransportServer::connection_info`].
    type ConnectionInfo;

    /// Type of event raised by this server.
    ///
    /// This event type must be able to be potentially converted into a
    /// [`ServerEvent`]. If an event value cannot cleanly map to a single
    /// generic [`ServerEvent`], its [`Into`] impl must return [`None`].
    type Event: Into<Option<ServerEvent<P, Self>>>
    where
        Self: Sized;

    /// Gets the current connection information and statistics on a connected
    /// client.
    ///
    /// The data that this function returns is left up to the implementation,
    /// but in general this allows accessing:
    /// * the round-trip time, or ping ([`Rtt`])
    /// * the remote socket address ([`RemoteAddr`])
    ///
    /// [`Rtt`]: crate::Rtt
    /// [`RemoteAddr`]: crate::RemoteAddr
    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo>;

    /// Gets if the given client is currently connected.
    fn connected(&self, client: Self::Client) -> bool {
        self.connection_info(client).is_some()
    }

    /// Gets all clients connected to this server.
    ///
    /// Each client key returned by this iterator is guaranteed to be connected,
    /// however the implementation may have some clients which *could* be
    /// connected, but not recognised as connected yet.
    fn connected_clients(&self) -> impl Iterator<Item = Self::Client>;

    /// Attempts to send a message to the given client.
    ///
    /// # Errors
    ///
    /// If the server cannot even attempt to send a message to the client (e.g.
    /// if the server knows that this client is already disconnected), this
    /// returns an error.
    ///
    /// However, since errors may occur later in the transport process after
    /// this function has already returned (e.g. in an async task), this will
    /// return [`Ok`] if the server has successfully *tried* to send a message,
    /// not if the server actually *has* sent the message.
    ///
    /// If an error occurs later during the transport process, the server will
    /// forcefully disconnect the client and emit a
    /// [`ServerEvent::Disconnected`].
    fn send(&mut self, client: Self::Client, msg: impl Into<P::S2C>) -> Result<(), Self::Error>;

    /// Polls events and receives messages from this transport.
    ///
    /// This will consume messages and events from connected clients. Events
    /// must be continuously received to allow this transport to do its internal
    /// work, so this should be run in the main loop of your program.
    ///
    /// This returns an iterator over the events received, which may be used in
    /// two ways:
    /// * used as-is, if you know the concrete type of the transport
    ///   * transports may expose their own event type, which allows you to
    ///     listen to specialized events
    /// * converted into a generic [`ServerEvent`] via its
    ///   `Into<Option<ServerEvent>>` implementation
    ///   * useful for generic code which must abstract over different transport
    ///     implementations
    ///   * a single event returned from this is not guaranteed to map to a
    ///     specific [`ServerEvent`]
    fn recv<'a>(&mut self) -> impl Iterator<Item = Self::Event> + 'a
    where
        Self: Sized;

    /// Forces a client to disconnect from this server.
    ///
    /// This function does not guarantee that the client is gracefully
    /// disconnected in any way, so you must use your own mechanism for graceful
    /// disconnection if you need this feature.
    ///
    /// Disconnecting a client using this function will also raise a
    /// [`ServerEvent::Disconnected`].
    ///
    /// # Errors
    ///
    /// If the server cannot even attempt to disconnect this client (e.g. if the
    /// server knows that this client is already disconnected), this returns an
    /// error.
    fn disconnect(&mut self, client: impl Into<Self::Client>) -> Result<(), Self::Error>;
}

/// An event which is raised by a [`TransportServer`].
#[derive(Debug, Clone)]
pub enum ServerEvent<P, T>
where
    P: TransportProtocol,
    T: TransportServer<P>,
{
    /// A client has fully connected to this server.
    ///
    /// Use this event to do client setup logic, e.g. start loading player data.
    Connected {
        /// The key of the connected client.
        client: T::Client,
    },
    /// A client sent a message to this server.
    Recv {
        /// The key of the client which sent the message.
        client: T::Client,
        /// The message received.
        msg: P::C2S,
    },
    /// A client has lost connection from this server, which cannot be recovered
    /// from.
    ///
    /// Use this event to do client teardown logic, e.g. removing the player
    /// from the world.
    Disconnected {
        /// The key of the client.
        client: T::Client,
        /// The reason why the client lost connection.
        cause: T::Error,
    },
}
