// #[cfg(feature = "bevy")]
// mod plugin;

// #[cfg(feature = "bevy")]
// pub use plugin::*;

use crate::Message;

/// Allows listening for client connections, and transporting messages to/from
/// the clients connected to this server.
/// 
/// # Events
/// 
// TODO
/// 
/// # Transport
/// 
/// This trait does not necessarily represent a **networked** server, which is
/// one that communicates to other computers probably using the internet.
/// Instead, a transport server may also be working using in-memory channels or
/// some other non-networked method.
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
/// This definition applies to [`TransportServer::connection_info`] and
/// [`TransportServer::connected`].
pub trait TransportServer<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    /// Type that this server uses to uniquely identify clients.
    type Client: Send + Sync + 'static;

    /// Error returned from operations on this server.
    type Error: Send + Sync + 'static;

    /// Info on a given client's connection status, returned by
    /// [`TransportServer::connection_info`].
    type ConnectionInfo;

    /// Iterator over events raised by this server, returned by
    /// [`TransportServer::recv`].
    type RecvIter<'a>: Iterator<Item = ServerEvent<C2S, Self::Client, Self::Error>> + 'a
    where
        Self: 'a;

    /// Gets the current connection information and statistics on a connected
    /// client.
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
    // TODO
    fn recv(&mut self) -> Self::RecvIter<'_>;

    /// Forces a client to disconnect from this server.
    /// 
    /// This function does not guarantee that the client is gracefully
    /// disconnected in any way, so you must use your own mechanism for graceful
    /// disconnection if you need this feature.
    /// 
    /// Disconnecting a client using this function will not raise a
    /// [`ServerEVent::Disconnected`].
    /// 
    /// # Errors
    /// 
    /// If the server cannot even attempt to disconnect this client (e.g. if the
    /// server knows that this client is already disconnected), this returns an
    /// error.
    fn disconnect(&mut self, target: Self::Client) -> Result<(), Self::Error>;
}

pub enum ServerEvent<C2S, C, E> {
    Connected { client: C },
    Recv { from: C, msg: C2S },
    Disconnected { client: C, reason: E },
}
