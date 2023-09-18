//! Server-side transport API, handling incoming clients, and sending/receiving messages
//! to/from clients.

#[cfg(feature = "bevy")]
pub mod plugin;

use std::{fmt::Display, net::SocketAddr, time::Duration};

use anyhow::Result;
use generational_arena::Index;

use crate::TransportConfig;

/// A server-to-client layer responsible for sending user messages to the other side.
/// 
/// The server transport accepts incoming connections, sending and receiving messages, and handling
/// disconnections and errors. Different transport implementations will use different methods to
/// transport the data across, including through memory or over a network.
/// 
/// The `C` parameter allows configuring which types of messages are sent and received by this
/// transport. See [`TransportConfig`] for details.
pub trait Transport<C: TransportConfig>: Send + Sync {
    /// Receive a queued event from the transport.
    /// 
    /// See [`Event`] on what kind of events this transport may respond with.
    /// 
    /// # Examples
    /// 
    /// ```
    /// use aeronet::{TransportConfig, server::{Transport, Event, RecvError}};
    /// 
    /// # fn update<C: TransportConfig, T: Transport<C>>(transport: T) {
    /// loop {
    ///     match transport.recv() {
    ///         Ok(Event::Connected { client }) => println!("Client {client} connected"),
    ///         Ok(_) => {},
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

    /// Send a message to a connected client.
    fn send(&mut self, client: ClientId, msg: C::S2C);

    /// Force a client to disconnect from the server.
    /// 
    /// This will issue an [`Event::Disconnected`] with reason [`SessionError::ForceDisconnect`].
    fn disconnect(&mut self, client: ClientId);
}

/// A [`Transport`] that allows access to the round-trip time of a connected client.
/// 
/// Since not all transports will use a network with a round-trip time, this trait is separate
/// from [`Transport`].
pub trait ClientRtt {
    /// Gets the round-trip time of a connected client.
    /// 
    /// The round-trip time is defined as the time taken for the following to happen:
    /// - client sends data
    /// - server receives the data and sends a response
    ///   - the processing time is assumed to be instant
    /// - client receives data
    /// 
    /// If no client exists for the given ID, [`None`] is returned.
    fn rtt(&self, client: ClientId) -> Option<Duration>;
}

/// A [`Transport`] that allows access to the remote socket address of a connected client.
/// 
/// Since not all transports will use a network with clients connecting from a socket address, this
/// trait is separate from [`Transport`].
pub trait ClientRemoteAddr {
    /// Gets the remote socket address of a connected client.
    /// 
    /// If no client exists for the given ID, [`None`] is returned.
    fn remote_addr(&self, client: ClientId) -> Option<SocketAddr>;
}

/// An error that occurrs while receiving queued [`Event`]s from a [`Transport`].
#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    /// There are no more events to receive.
    #[error("no events to receive")]
    Empty,
    /// The server is closed and no more events will ever be received.
    #[error("server closed")]
    Closed,
}

/// An event received from a [`Transport`].
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum Event<C2S> {
    /// See [`ClientRequested`].
    Requested(ClientRequested),
    /// A client successfully connected and the connection can now be used.
    Connected {
        /// The ID of the connected client.
        client: ClientId,
    },
    /// A client sent data to this server.
    Recv {
        /// The ID of the sender.
        client: ClientId,
        /// The message sent.
        msg: C2S,
    },
    /// A client disconnected from this server, caused by either a transport error or the server
    /// forcing this client off.
    Disconnected {
        /// The ID of the disconnected client.
        client: ClientId,
        /// Why the client was disconnected.
        reason: SessionError,
    },
}

/// A client requested to connect and was subsequently given an ID.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub struct ClientRequested {
    /// The ID of the requesting client.
    pub client: ClientId,
}

/// The reason why a client was disconnected from a server.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The server was closed and all open client connections have been dropped.
    #[error("server closed")]
    ServerClosed,
    /// The server forced this client to disconnect.
    #[error("forced disconnect by server")]
    ForceDisconnect,
    /// The client failed to establish a connection to the server.
    #[error("failed to connect to server")]
    Connecting(#[source] anyhow::Error),
}

/// A unique identifier for a client connected to a server.
///
/// This uses an [`Index`] under the hood, as it is expected that transport layers use a
/// generational arena to store clients. Using a [`generational_arena::Arena`] avoids the problem
/// of one client disconnecting with an ID, and another client later connecting with the same ID,
/// causing some code to mistake client 2 for client 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(Index);

impl ClientId {
    /// Creates an ID from the raw generational index.
    pub fn from_raw(index: Index) -> Self {
        Self(index)
    }

    /// Converts an ID into its raw generational index.
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
