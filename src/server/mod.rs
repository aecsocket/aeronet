#[cfg(feature = "bevy")]
pub mod plugin;

use std::fmt::Display;

use crate::{Message, SessionError};

/// A server-to-client layer responsible for sending user messages to the other
/// side.
///
/// The server transport accepts incoming connections, sending and receiving
/// messages, and handling disconnections and errors.
///
/// Different transport implementations will use different methods to
/// transport the data across, such as through memory or over a network. This
/// means that a transport does not necessarily work over the internet! If you
/// want to get details such as RTT or remote address, see [`Rtt`] and
/// [`RemoteAddr`].
///
/// The type parameters allows configuring which types of messages are sent and
/// received by this transport (see [`Message`]).
///
/// [`Rtt`]: crate::Rtt
/// [`RemoteAddr`]: crate::RemoteAddr
pub trait ServerTransport<C2S, S2C>
where
    C2S: Message,
    S2C: Message,
{
    /// The info that [`ServerTransport::client_info`] provides.
    type ClientInfo;

    /// Instructs the transport to receive incoming events and update its
    /// internal state.
    ///
    /// This should be called before [`ServerTransport::take_events`].
    fn recv(&mut self);

    /// Takes ownership of all queued events in this transport.
    fn take_events(&mut self) -> impl Iterator<Item = ServerEvent<C2S>> + '_;

    /// Sends a message to a connected client.
    fn send(&mut self, client: ClientId, msg: impl Into<S2C>);

    /// Forces a client to disconnect from the server.
    ///
    /// This will issue a [`ServerEvent::Disconnected`] with reason
    /// [`SessionError::ForceDisconnect`].
    fn disconnect(&mut self, client: ClientId);

    /// Gets transport info on a connected client.
    ///
    /// If the specified client is not connected, [`None`] is returned.
    fn client_info(&self, client: ClientId) -> Option<Self::ClientInfo>;

    /// Gets if the specified client is connected to this server.
    fn connected(&self, client: ClientId) -> bool;
}

/// An event received from a [`ServerTransport`].
///
/// Under Bevy this also implements `Event`, however this type cannot be used in
/// an event reader or writer using the inbuilt plugins. `Event` is implemented
/// to allow user code to use this type as an event if they wish to manually
/// implement transport handling.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ServerEvent<C2S> {
    /// A client has established a connection to the server and can now
    /// send/receive data.
    ///
    /// This should be used as a signal to start client setup, such as loading
    /// the client's data from a database.
    Connected(ClientId),
    /// A connected client sent data to the server.
    Recv(ClientId, C2S),
    /// A client was lost and the connection was closed for any reason.
    ///
    /// This is called for both transport errors (such as losing connection) and
    /// for the transport forcefully disconnecting the client via
    /// [`ServerTransport::disconnect`].
    ///
    /// This should be used as a signal to start client teardown and removing
    /// them from the app.
    Disconnected(ClientId, SessionError),
}

/// A unique identifier for a client connected to a server.
///
/// This uses a [`usize`] under the hood, however it is up to the implementation
/// on how to use this exactly. One possible approach is to use an
/// auto-incrementing integer and store that in a hash map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(usize);

impl ClientId {
    /// Creates an ID from the raw value provided.
    ///
    /// Passing an arbitrary value which was not previously made from
    /// [`Self::into_raw`] may result in a client ID which does not point to
    /// a valid client.
    pub fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    /// Converts an ID into its raw value.
    pub fn into_raw(self) -> usize {
        self.0
    }
}

impl Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
