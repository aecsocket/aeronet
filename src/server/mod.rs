//! Server-side transport API, handling incoming clients, and sending/receiving messages
//! to/from clients.

#[cfg(feature = "bevy")]
pub mod plugin;

use std::{fmt::Display, net::SocketAddr, time::Duration};

use generational_arena::Index;

use crate::{message::{RecvMessage, SendMessage}, transport::{RecvError, SessionError}};

/// A server-to-client layer responsible for sending user messages to the other side.
///
/// The server transport accepts incoming connections, sending and receiving messages, and handling
/// disconnections and errors.
///
/// Different transport implementations will use different methods to
/// transport the data across, such as through memory or over a network. This means that a
/// transport does not necessarily work over the internet! If you want info on networking, see
/// related traits like [`GetRtt`] and [`GetRemoteAddr`].
///
/// The `C` parameter allows configuring which types of messages are sent and received by this
/// transport (see [`TransportConfig`]).
pub trait Transport<C: TransportConfig> {
    /// Attempts to receive a queued event from the transport.
    ///
    /// # Usage
    ///
    /// ```
    /// # use aeronet::{transport::RecvError, server::{Transport, TransportConfig, Event}};
    /// # fn update<C: TransportConfig, T: Transport<C>>(transport: T) {
    /// loop {
    ///     match transport.recv() {
    ///         Ok(Event::Connected { client }) => println!("Client {client} connected"),
    ///         Ok(_) => {},
    ///         // ...
    ///         Err(RecvError::Empty) => break,
    ///         Err(RecvError::Closed) => {
    ///             println!("Server closed");
    ///             return;
    ///         }
    ///     }
    /// }
    /// # }
    /// ```
    fn recv(&mut self) -> Result<Event<C::C2S>, RecvError>;

    /// Sends a message to a connected client.
    fn send(&mut self, client: ClientId, msg: impl Into<C::S2C>);

    /// Forces a client to disconnect from the server.
    ///
    /// This will issue an [`Event::Disconnected`] with reason [`SessionError::ForceDisconnect`].
    fn disconnect(&mut self, client: ClientId);
}

/// A [`Transport`] that allows access to the round-trip time of a connected client.
///
/// Since not all transports will use a network with a round-trip time, this trait is separate
/// from [`Transport`].
pub trait GetRtt {
    /// Gets the round-trip time of a connected client.
    ///
    /// The round-trip time is defined as the time taken for the following to happen:
    /// * client sends data
    /// * server receives the data and sends a response
    ///   * the processing time is assumed to be instant
    /// * client receives data
    ///
    /// If no client exists for the given ID, [`None`] is returned.
    fn rtt(&self, client: ClientId) -> Option<Duration>;
}

/// A [`Transport`] that allows access to the remote socket address of a connected client.
///
/// Since not all transports will use a network with clients connecting from a socket address, this
/// trait is separate from [`Transport`].
pub trait GetRemoteAddr {
    /// Gets the remote socket address of a connected client.
    ///
    /// If no client exists for the given ID, [`None`] is returned.
    fn remote_addr(&self, client: ClientId) -> Option<SocketAddr>;
}

/// Configures the types used by a server-side transport implementation.
///
/// A transport is abstract over the exact message type that it uses, instead letting the user
/// decide. This trait allows configuring the message types both for:
/// * client-to-server messages ([`TransportConfig::C2S`])
///   * the server must be able to deserialize these from a payload ([`RecvMessage`])
/// * server-to-client messages ([`TransportConfig::S2C`])
///   * the server must be able to serialize these into a payload ([`SendMessage`])
///
/// The types used for C2S and S2C may be different.
///
/// # Examples
///
/// ```
/// use aeronet::server::TransportConfig;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// pub enum C2S {
///     Ping(u64),
/// }
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// pub enum S2C {
///     Pong(u64),
/// }
///
/// pub struct AppTransportConfig;
///
/// impl TransportConfig for AppTransportConfig {
///     type C2S = C2S;
///     type S2C = S2C;
/// }
/// ```
pub trait TransportConfig: Send + Sync + 'static {
    /// The client-to-server message type.
    ///
    /// The server will only receive messages of this type, requiring [`RecvMessage`].
    type C2S: RecvMessage;

    /// The server-to-client message type.
    ///
    /// The server will only send messages of this type, requiring [`SendMessage`].
    type S2C: SendMessage;
}

/// An event received from a [`Transport`].
/// 
/// Under [`bevy`] this also implements `Event`, however this type cannot be used in an event
/// reader or writer using the inbuilt plugins. `Event` is implemented to allow user code to use
/// this type as an event if they wish to manually implement transport handling.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum Event<C2S> {
    /// A client requested a connection and has been assigned a client ID.
    Incoming {
        /// The ID assigned to the incoming connection.
        client: ClientId,
    },
    /// A client has established a connection to the server and can now send/receive data.
    ///
    /// This should be used as a signal to start client setup, such as loading the client's data
    /// from a database.
    Connected {
        /// The ID of the connected client.
        client: ClientId,
    },
    /// A connected client sent data to the server.
    Recv {
        /// The ID of the sender.
        client: ClientId,
        /// The message sent by the client.
        msg: C2S,
    },
    /// A client was lost and the connection was closed for any reason.
    ///
    /// This is called for both transport errors (such as losing connection) and for the transport
    /// forcefully disconnecting the client via [`Transport::disconnect`].
    ///
    /// This should be used as a signal to start client teardown and removing them from the app.
    Disconnected {
        /// The ID of the disconnected client.
        client: ClientId,
        /// Why the connection was lost.
        reason: SessionError,
    },
}

/// A unique identifier for a client connected to a server.
///
/// This uses an [`Index`] under the hood, as it is expected that transport layers use a
/// [`generational_arena::Arena`] to store clients.
/// This avoids the problem of one client disconnecting with an ID, and another client later
/// connecting with the same ID, causing some code to mistake client 2 for client 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(Index);

impl ClientId {
    /// Creates an ID from the raw generational index.
    ///
    /// Passing an arbitrary index which was not previously made from [`Self::into_raw`] may
    /// result in a client ID which does not point to a valid client.
    pub fn from_raw(index: Index) -> Self {
        Self(index)
    }

    /// Converts an ID into its raw generational index.
    ///
    /// Used by transport implementations, which are expected to store clients in a
    /// [`generational_arena::Arena`], to index into that arena.
    pub fn into_raw(self) -> Index {
        self.0
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (index, gen) = self.0.into_raw_parts();
        write!(f, "{index}v{gen}")
    }
}
