mod backend;
mod frontend;

use aeronet::{
    ChannelProtocol, OnChannel, TransportProtocol, TransportServer, TryAsBytes, TryFromBytes,
};

use std::{fmt::Debug, io, net::SocketAddr};

use derivative::Derivative;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};

use crate::{ClientKey, EndpointInfo};

type WebTransportError<P> =
    crate::WebTransportError<P, <P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

/// Implementation of [`TransportServer`] using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
#[derive(Derivative)]
#[derivative(
    Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"),
    Default(bound = "")
)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportServer<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    state: State<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
enum State<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    Closed,
    Opening(OpeningServer<P>),
    Open(OpenServer<P>),
}

impl<P> Default for State<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    fn default() -> Self {
        Self::Closed
    }
}

/// Event raised by a [`WebTransportServer`].
#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
pub enum ServerEvent<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
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
        msg: P::C2S,
    },
    /// A client has lost connection from this server, which could not be
    /// recovered from.
    ///
    /// This is equivalent to [`aeronet::ServerEvent::Disconnected`].
    Disconnected {
        /// The key of the client.
        client: ClientKey,
        /// The reason why the client lost connection.
        cause: WebTransportError<P>,
    },
    /// The server backend has been shut down, all client connections have been
    /// dropped, and the backend must be re-opened.
    Closed {
        /// The reason why the backend was closed.
        cause: WebTransportError<P>,
    },
}

impl<P, T> From<ServerEvent<P>> for Option<aeronet::ServerEvent<P, T>>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
    T: TransportServer<P, Client = ClientKey, Error = WebTransportError<P>>,
{
    fn from(value: ServerEvent<P>) -> Self {
        match value {
            ServerEvent::Connected { client } => Some(aeronet::ServerEvent::Connected { client }),
            ServerEvent::Recv { client, msg } => Some(aeronet::ServerEvent::Recv { client, msg }),
            ServerEvent::Disconnected { client, cause } => {
                Some(aeronet::ServerEvent::Disconnected { client, cause })
            }
            ServerEvent::Opened
            | ServerEvent::Incoming { .. }
            | ServerEvent::Accepted { .. }
            | ServerEvent::Closed { .. } => None,
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
struct OpeningServer<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    #[derivative(Debug = "ignore")]
    recv_open: oneshot::Receiver<OpenServerResult<P>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
struct OpenServer<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    local_addr: Result<SocketAddr, io::Error>,
    clients: SlotMap<ClientKey, RemoteClient<P>>,
    #[derivative(Debug = "ignore")]
    recv_client: mpsc::UnboundedReceiver<UntrackedClient<P>>,
    #[derivative(Debug = "ignore")]
    #[allow(dead_code)]
    send_closed: mpsc::Sender<()>,
}

type OpenServerResult<P> = Result<OpenServer<P>, WebTransportError<P>>;

// client states

#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
enum RemoteClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    Untracked(UntrackedClient<P>),
    Incoming(IncomingClient<P>),
    Accepted(AcceptedClient<P>),
    Connected(ConnectedClient<P>),
    Disconnected,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct UntrackedClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    #[derivative(Debug = "ignore")]
    send_key: Option<oneshot::Sender<ClientKey>>,
    #[derivative(Debug = "ignore")]
    recv_incoming: oneshot::Receiver<IncomingClient<P>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct IncomingClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    #[derivative(Debug = "ignore")]
    recv_accepted: oneshot::Receiver<AcceptedClientResult<P>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct AcceptedClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<P>>,
}

type AcceptedClientResult<P> = Result<AcceptedClient<P>, WebTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectedClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    info: EndpointInfo,
    #[derivative(Debug = "ignore")]
    recv_info: mpsc::UnboundedReceiver<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_c2s: mpsc::UnboundedReceiver<P::C2S>,
    #[derivative(Debug = "ignore")]
    send_s2c: mpsc::UnboundedSender<P::S2C>,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<WebTransportError<P>>,
}

type ConnectedClientResult<P> = Result<ConnectedClient<P>, WebTransportError<P>>;
