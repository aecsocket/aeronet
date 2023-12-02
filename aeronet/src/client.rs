use crate::Message;

/// Allows connecting to a server, and transporting messages to/from the server.
/// 
/// # Transport
/// 
/// This trait does not necessarily represent a **networked** client, which is
/// one that communicates to other computers probably using the internet.
/// Instead, a transport client may also work using in-memory channels or some
/// other non-networked method.
pub trait TransportClient<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    /// Error returned from operations on this client.
    type Error: Send + Sync + 'static;

    /// Info on this client's connection status, returned by
    /// [`TransportClient::connection_info`].
    type ConnectionInfo;

    /// Type of event raised by this client.
    type Event: Into<Option<ClientEvent<S2C, Self::Error>>>;

    /// Iterator over events raised by this client, returned by
    /// [`TransportClient::recv`].
    type RecvIter<'a>: Iterator<Item = Self::Event> + 'a
    where
        Self: 'a;

    /// Gets the current connection information and statistics if this client
    /// is connected.
    /// 
    /// See [`TransportServer`] for the definition of "connected".
    fn connection_info(&self) -> Option<Self::ConnectionInfo>;

    /// Gets if this client is currently connected.
    /// 
    /// See [`TransportServer`] for the definition of "connected".
    fn connected(&self) -> bool {
        self.connection_info().is_some()
    }

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
    fn send<M: Into<C2S>>(&mut self, msg: M) -> Result<(), Self::Error>;

    /// Polls events and receives messages from this transport.
    /// 
    /// This will consume messages and events if the client is connected. Events
    /// must be continuously received to allow this transport to do its internal
    /// work.
    /// 
    /// This returns an iterator over the events received, which may be used in
    /// two ways:
    /// * converted into a generic [`ClientEvent`] via its [`Into`]
    /// implementation
    ///   * useful for generic code which must abstract over different transport
    ///     implementations
    /// * used as-is, if you know the concrete type of the transport
    ///   * transports may expose their own event tyoe, which allows you to
    ///     listen to specialized events
    fn recv(&mut self) -> (Self::RecvIter<'_>, Result<(), Self::Error>);
}

/// An event which is raised by a [`TransportClient`].
#[derive(Debug, Clone)]
pub enum ClientEvent<S2C, E> {
    /// This client has fully connected to a server.
    /// 
    /// See [`TransportServer`] for the definition of "connected".
    /// 
    /// Use this event to do setup logic, e.g. start loading the level.
    Connected,
    /// The server sent a message to this client.
    Recv {
        /// The message.
        msg: S2C,
    },
    /// This client has lost connection from its previously connected server,
    /// which cannot be recovered from.
    /// 
    /// Use this event to do teardown logic, e.g. changing state to the main
    /// menu.
    Disconnected {
        /// The reason why the client lost connection.
        cause: E,
    },
}
