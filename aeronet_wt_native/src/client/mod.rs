mod backend;
mod frontend;

use std::{fmt::Debug, io, net::SocketAddr};

use aeronet::{
    ChannelProtocol, OnChannel, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use tokio::sync::{mpsc, oneshot};

use crate::EndpointInfo;

type WebTransportError<P> =
    crate::WebTransportError<P, <P as TransportProtocol>::C2S, <P as TransportProtocol>::S2C>;

type ClientEvent<P> = aeronet::ClientEvent<P, WebTransportClient<P>>;

/// Implementation of [`TransportClient`] using the WebTransport protocol.
///
/// See the [crate-level docs](crate).
/// 
/// [`TransportClient`]: aeronet::TransportClient
#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"), Default(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    state: State<P>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "P::C2S: Debug, P::S2C: Debug, P::Channel: Debug"))]
enum State<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    Disconnected { forced: bool },
    Connecting(ConnectingClient<P>),
    Connected(ConnectedClient<P>),
}

impl<P> Default for State<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    fn default() -> Self {
        Self::Disconnected { forced: false }
    }
}

// client states

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectingClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
    P::S2C: TryFromBytes,
{
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<P>>,
    #[derivative(Debug = "ignore")]
    send_event: bool,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectedClient<P>
where
    P: ChannelProtocol,
    P::C2S: TryAsBytes + OnChannel<Channel = P::Channel>,
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
