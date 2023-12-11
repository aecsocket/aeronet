use crate::Message;

/// Allows listening for client connections, and transporting messages to/from
/// the clients connected to this server.
///
/// # Transport
///
/// This trait does not necessarily represent a **networked** server, which is
/// one that communicates to other computers probably using the internet.
/// Instead, a transport server may also work using in-memory channels or some
/// other non-networked method.
///
/// This trait also provides no guarantees about how the messages are
/// transported. The values may be sent directly (in the case of in-memory MPSC
/// channels), or may have to be serialized to/from a byte form before being
/// transported. In this case, [`TryFromBytes`] and [`TryIntoBytes`] may be
/// useful.
///
/// # Connection
///
/// Although clients may go through several different stages of connection,
/// this trait abstracts over this process and only represents clients as either
/// **connected** or **not connected**.
///
/// A client is considered connected if the transport between this server and
/// the client has been finalized - this means that all setup, such as opening
/// streams and configuration, has been finished, and both sides can now send
/// messages to each other.
///
/// Note that implementations of this transport *may* expose more info on the
/// current connection state (e.g. "client incoming") but is not required to.
///
/// [`TryFromBytes`]: crate::TryFromBytes
/// [`TryIntoBytes`]: crate::TryIntoBytes
pub trait TransportServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    /// Key type that this server uses to uniquely identify clients.
    type Client: Send + Sync + 'static;

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
    type Event: Into<Option<ServerEvent<C2S, Self::Client, Self::Error>>>;

    /// Iterator over events raised by this server, returned by
    /// [`TransportServer::recv`].
    type RecvIter<'a>: Iterator<Item = Self::Event> + 'a
    where
        Self: 'a;

    /// Gets the current connection information and statistics on a connected
    /// client.
    ///
    /// See [`TransportServer`] for the definition of "connected".
    ///
    /// The data that this function returns is left up to the implementation,
    /// but in general this allows accessing:
    /// * the round-trip time, or ping ([`crate::Rtt`])
    /// * the remote socket address ([`crate:RemoteAddr`])
    fn connection_info(&self, client: Self::Client) -> Option<Self::ConnectionInfo>;

    /// Gets if the given client is currently connected.
    ///
    /// See [`TransportServer`] for the definition of "connected".
    fn connected(&self, client: Self::Client) -> bool {
        self.connection_info(client).is_some()
    }

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
    fn send<M: Into<S2C>>(&mut self, to: Self::Client, msg: M) -> Result<(), Self::Error>;

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
    fn recv(&mut self) -> Self::RecvIter<'_>;

    /// Forces a client to disconnect from this server.
    ///
    /// This function does not guarantee that the client is gracefully
    /// disconnected in any way, so you must use your own mechanism for graceful
    /// disconnection if you need this feature.
    ///
    /// Disconnecting a client using this function will not raise a
    /// [`ServerEvent::Disconnected`].
    ///
    /// # Errors
    ///
    /// If the server cannot even attempt to disconnect this client (e.g. if the
    /// server knows that this client is already disconnected), this returns an
    /// error.
    fn disconnect<C: Into<Self::Client>>(&mut self, target: C) -> Result<(), Self::Error>;
}

/// An event which is raised by a [`TransportServer`].
#[derive(Debug, Clone)]
pub enum ServerEvent<C2S, C, E> {
    /// A client has fully connected to this server.
    ///
    /// See [`TransportServer`] for the definition of "connected".
    ///
    /// Use this event to do client setup logic, e.g. start loading player data.
    Connected {
        /// The key of the connected client.
        client: C,
    },
    /// A client sent a message to this server.
    Recv {
        /// The key of the client which sent the message.
        from: C,
        /// The message.
        msg: C2S,
    },
    /// A client has lost connection from this server, which cannot be recovered
    /// from.
    ///
    /// Use this event to do client teardown logic, e.g. removing the player
    /// from the world.
    Disconnected {
        /// The key of the client.
        client: C,
        /// The reason why the client lost connection.
        cause: E,
    },
}
