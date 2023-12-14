mod backend;
mod frontend;

use std::{fmt::Debug, io, net::SocketAddr};

use aeronet::{OnChannel, TryFromBytes, TryIntoBytes, TransportProtocol, TransportClient};
use derivative::Derivative;
use tokio::sync::{mpsc, oneshot};

use crate::{EndpointInfo, WebTransportProtocol};

type WebTransportError<P> = crate::WebTransportError<
    P,
    <P as TransportProtocol>::C2S,
    <P as TransportProtocol>::S2C,
>;

/// Implementation of [`TransportClient`] using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
#[derive(Debug, Derivative)]
#[derivative(Default)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    state: State<P>,
}

/// Event raised by a [`WebTransportClient`].
#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
pub enum ClientEvent<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    /// This client has fully connected to a server.
    ///
    /// This is equivalent to [`aeronet::ClientEvent::Connected`].
    Connected,
    /// The connected server sent a message to the client.
    ///
    /// This is equivalent to [`aeronet::ClientEvent::Recv`].
    Recv {
        /// The message received.
        msg: P::S2C,
    },
    /// The client lost connection from its previous server, which could not be
    /// recovered from.
    ///
    /// This is equivalent to [`aeronet::ClientEvent::Disconnected`].
    Disconnected {
        /// The reason why the client lost connection.
        cause: WebTransportError<P>,
    },
}

impl<P, T> From<ClientEvent<P>>
    for Option<aeronet::ClientEvent<P, T>>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
    T: TransportClient<
        P,
        Error = WebTransportError<P>,
    >,
{
    fn from(value: ClientEvent<P>) -> Self {
        match value {
            ClientEvent::Connected => Some(aeronet::ClientEvent::Connected),
            ClientEvent::Recv { msg } => Some(aeronet::ClientEvent::Recv { msg }),
            ClientEvent::Disconnected { cause } => {
                Some(aeronet::ClientEvent::Disconnected { cause })
            }
        }
    }
}

#[derive(Debug, Default)]
enum State<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    #[default]
    Disconnected,
    Connecting(ConnectingClient<P>),
    Connected(ConnectedClient<P>),
}

// client states

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectingClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<P>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectedClient<P>
where
    P: WebTransportProtocol,
    P::C2S: TryIntoBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    local_addr: Result<SocketAddr, io::Error>,
    info: EndpointInfo,
    #[derivative(Debug = "ignore")]
    recv_info: mpsc::UnboundedReceiver<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_s2c: mpsc::UnboundedReceiver<P::S2C>,
    #[derivative(Debug = "ignore")]
    send_c2s: mpsc::UnboundedSender<P::C2S>,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<WebTransportError<P>>,
}

type ConnectedClientResult<P> = Result<ConnectedClient<P>, WebTransportError<P>>;
