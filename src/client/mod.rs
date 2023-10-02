#[cfg(feature = "bevy")]
pub mod plugin;

use crate::{
    message::{RecvMessage, SendMessage},
    transport::{RecvError, SessionError},
};

/// A client-to-server layer responsible for sending user messages to the other side.
///
/// The client transport attempts to connect to a server when created, handles sending and
/// receiving messages, as well as forwarding disconnections and errors to the app.
///
/// Different transport implementations will use different methods to
/// transport the data across, such as through memory or over a network. This means that a
/// transport does not necessarily work over the internet! If you want to get details such as
/// RTT or remote address, see [`crate::TransportRtt`] and [`crate::TransportRemoteAddr`].
///
/// The `C` parameter allows configuring which types of messages are sent and received by this
/// transport (see [`ClientTransportConfig`]).
pub trait ClientTransport<C: ClientTransportConfig> {
    /// The info that [`ClientTransport::info`] provides.
    type Info;

    /// Attempts to receive a queued event from the transport.
    ///
    /// # Usage
    ///
    /// ```
    /// # use aeronet::{RecvError, ClientTransport, ClientTransportConfig, ClientEvent};
    /// # fn update<C: ClientTransportConfig, T: ClientTransport<C>>(mut transport: T) {
    /// loop {
    ///     match transport.recv() {
    ///         Ok(ClientEvent::Recv { msg }) => println!("Received a message"),
    ///         Ok(_) => {},
    ///         // ...
    ///         Err(RecvError::Empty) => break,
    ///         Err(RecvError::Closed) => {
    ///             println!("Client closed");
    ///             return;
    ///         }
    ///     }
    /// }
    /// # }
    /// ```
    fn recv(&mut self) -> Result<ClientEvent<C::S2C>, RecvError>;

    /// Sends a message to the connected server.
    fn send(&mut self, msg: impl Into<C::C2S>);

    /// Gets transport info on the current connection.
    ///
    /// If this transport is not connected to a server, [`None`] is returned.
    fn info(&self) -> Option<Self::Info>;

    /// Gets if this transport has a connection to a server.
    fn is_connected(&self) -> bool {
        self.info().is_some()
    }
}

/// Configures the types used by a client-side transport implementation.
///
/// A transport is abstract over the exact message type that it uses, instead letting the user
/// decide. This trait allows configuring the message types both for:
/// * client-to-server messages ([`ClientTransportConfig::C2S`])
///   * the client must be able to serialize these into a payload ([`SendMessage`])
/// * server-to-client messages ([`ClientTransportConfig::S2C`])
///   * the client must be able to deserialize these from a payload ([`RecvMessage`])
///
/// The types used for C2S and S2C may be different.
///
/// # Examples
///
/// ```
/// use aeronet::ClientTransportConfig;
///
/// #[derive(Debug, Clone)]
/// pub enum C2S {
///     Ping(u64),
/// }
/// # impl aeronet::SendMessage for C2S {
/// #     fn into_payload(self) -> anyhow::Result<Vec<u8>> { unimplemented!() }
/// # }
///
/// #[derive(Debug, Clone)]
/// pub enum S2C {
///     Pong(u64),
/// }
/// # impl aeronet::RecvMessage for S2C {
/// #     fn from_payload(buf: &[u8]) -> anyhow::Result<Self> { unimplemented!() }
/// # }
///
/// pub struct AppTransportConfig;
///
/// impl ClientTransportConfig for AppTransportConfig {
///     type C2S = C2S;
///     type S2C = S2C;
/// }
/// ```
pub trait ClientTransportConfig: Send + Sync + 'static {
    /// The client-to-server message type.
    ///
    /// The client will only send messages of this type, requiring [`SendMessage`].
    type C2S: SendMessage;

    /// The server-to-client message type.
    ///
    /// The client will only receive messages of this type, requiring [`RecvMessage`].
    type S2C: RecvMessage;
}

/// An event received from a [`ClientTransport`].
///
/// Under Bevy this also implements `Event`, however this type cannot be used in an event
/// reader or writer using the inbuilt plugins. `Event` is implemented to allow user code to use
/// this type as an event if they wish to manually implement transport handling.
#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Event))]
pub enum ClientEvent<S2C> {
    /// The client has started connecting to a server at the app's request.
    ///
    /// This event may not be sent in some implementations.
    Connecting,
    /// The client successfully connected to the server that was requested when creating the
    /// transport.
    ///
    /// This should be used as a signal to transition into the next app state, such as entering the
    /// level loading menu in a game.
    Connected,
    /// The connected server sent data to the client.
    Recv {
        /// The message sent by the server.
        msg: S2C,
    },
    /// The connection to the server was closed for any reason.
    ///
    /// This is called for both transport errors (such as losing connection) and for the transport
    /// being forcefully disconnected by the server.
    ///
    /// This should be used as a signal to transition into the next app state, such as entering the
    /// main menu after exiting a server.
    Disconnected {
        /// Why the connection was lost.
        reason: SessionError,
    },
}
