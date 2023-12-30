mod backend;
mod frontend;

use std::net::SocketAddr;

use derivative::Derivative;

use aeronet::{LaneProtocol, LocalAddr, OnLane, TryAsBytes, TryFromBytes};
use futures::channel::{mpsc, oneshot};

use crate::{ConnectionInfo, WebTransportError};

type TransportConfig<P> = aeronet::TransportConfig<P, ClientWebTransport<P>>;
type ClientEvent<P> = aeronet::ClientEvent<P, ClientWebTransport<P>>;

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub conn: ConnectionInfo,
    pub local_addr: SocketAddr,
}

impl LocalAddr for ClientInfo {
    fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    recv_connected: oneshot::Receiver<ConnectedResult<P>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    conn_info: ConnectionInfo,
    local_addr: SocketAddr,
    send_s: mpsc::UnboundedSender<P::Send>,
    recv_r: mpsc::Receiver<P::Recv>,
    recv_info: mpsc::Receiver<ConnectionInfo>,
    recv_err: oneshot::Receiver<WebTransportError<P>>,
}

type ConnectedResult<P> = Result<ConnectedClient<P>, WebTransportError<P>>;

#[derive(Derivative, Default)]
#[derivative(Debug(bound = ""))]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub enum ClientWebTransport<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    #[default]
    Disconnected,
    Connecting(ConnectingClient<P>),
    Connected(ConnectedClient<P>),
}
