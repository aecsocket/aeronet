mod backend;
mod frontend;

pub use frontend::*;

use std::{io, net::SocketAddr};

use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};

use crate::{ClientKey, EndpointInfo};

/// Event raised by a [`WebTransportServer`].
#[derive(Debug)]
pub enum ServerEvent<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    /// The server backend has been set up and is ready to accept connections.
    Opened,
    /// A client has requested to connect.
    ///
    /// No further data is known about the client yet.
    Incoming {
        /// The key of the client.
        client: ClientKey,
    },
    /// The server has accepted a client's request to connect.
    Accepted {
        /// The key of the client.
        client: ClientKey,
        /// See [`wtransport::endpoint::SessionRequest::authority`].
        authority: String,
        /// See [`wtransport::endpoint::SessionRequest::path`].
        path: String,
        /// See [`wtransport::endpoint::SessionRequest::origin`].
        origin: Option<String>,
        /// See [`wtransport::endpoint::SessionRequest::user_agent`].
        user_agent: Option<String>,
    },
    /// A client has fully established a connection to the server (including
    /// opening streams) and the connection is ready for messages.
    ///
    /// This is equivalent to [`aeronet::ServerEvent::Connected`].
    Connected {
        /// The key of the client.
        client: ClientKey,
    },
    /// A client sent a message to the server.
    ///
    /// This is equivalent to [`aeronet::ServerEvent::Recv`].
    Recv {
        /// The key of the client which sent the message.
        client: ClientKey,
        /// The message.
        msg: C2S,
    },
    /// A client has lost connection from this server, which could not be
    /// recovered from.
    ///
    /// This is equivalent to [`aeronet::ServerEvent::Disconnected`].
    Disconnected {
        /// The key of the client.
        client: ClientKey,
        /// The reason why the client lost connection.
        cause: WebTransportError<C2S, S2C, C>,
    },
    /// The server backend has been shut down, all client connections have been
    /// dropped, and the backend must be re-opened.
    Closed {
        /// The reason why the backend was closed.
        cause: WebTransportError<C2S, S2C, C>,
    },
}

impl<C2S, S2C, C> From<ServerEvent<C2S, S2C, C>>
    for Option<aeronet::ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    fn from(value: ServerEvent<C2S, S2C, C>) -> Self {
        match value {
            ServerEvent::Opened => None,
            ServerEvent::Incoming { .. } => None,
            ServerEvent::Accepted { .. } => None,
            ServerEvent::Connected { client } => Some(aeronet::ServerEvent::Connected { client }),
            ServerEvent::Recv { client, msg } => Some(aeronet::ServerEvent::Recv { client, msg }),
            ServerEvent::Disconnected { client, cause } => {
                Some(aeronet::ServerEvent::Disconnected { client, cause })
            }
            ServerEvent::Closed { .. } => None,
        }
    }
}

// server states

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<S2C, C2S, C>;

#[derive(Derivative)]
#[derivative(Debug)]
struct OpeningServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_open: oneshot::Receiver<OpenServerResult<C2S, S2C, C>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct OpenServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    local_addr: Result<SocketAddr, io::Error>,
    clients: SlotMap<ClientKey, ClientState<C2S, S2C, C>>,
    #[derivative(Debug = "ignore")]
    recv_client: mpsc::UnboundedReceiver<IncomingClient<C2S, S2C, C>>,
    #[derivative(Debug = "ignore")]
    #[allow(dead_code)]
    send_closed: mpsc::Sender<()>,
}

type OpenServerResult<C2S, S2C, C> =
    Result<OpenServer<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

// client states

#[derive(Debug)]
enum ClientState<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Incoming(IncomingClient<C2S, S2C, C>),
    Accepted(AcceptedClient<C2S, S2C, C>),
    Connected(ConnectedClient<C2S, S2C, C>),
    Disconnected,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct IncomingClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_accepted: oneshot::Receiver<AcceptedClientResult<C2S, S2C, C>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct AcceptedClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<C2S, S2C, C>>,
}

type AcceptedClientResult<C2S, S2C, C> =
    Result<AcceptedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    info: EndpointInfo,
    #[derivative(Debug = "ignore")]
    recv_info: mpsc::UnboundedReceiver<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_c2s: mpsc::UnboundedReceiver<C2S>,
    #[derivative(Debug = "ignore")]
    send_s2c: mpsc::UnboundedSender<S2C>,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<WebTransportError<C2S, S2C, C>>,
}

type ConnectedClientResult<C2S, S2C, C> =
    Result<ConnectedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;
