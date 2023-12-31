use std::{future::Future, task::Poll};

use aeronet::{
    ClientState, ClientTransport, LaneProtocol, MessageState, MessageTicket, OnLane, Transport,
    TryAsBytes, TryFromBytes,
};
use futures::channel::oneshot;
use wtransport::{datagram::Datagram, endpoint::IntoConnectOptions, ClientConfig};

use crate::{ClientInfo, ClientWebTransport, ConnectedClient, ConnectingClient, WebTransportError};

use super::{ClientEvent, ConnectedResult, TransportConfig};

impl<P> ConnectingClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    pub fn new(
        wt_config: ClientConfig,
        transport_config: TransportConfig<P>,
        conn_opts: impl IntoConnectOptions,
    ) -> (Self, impl Future<Output = ()> + Send) {
        let conn_opts = conn_opts.into_options();
        let (send_connected, recv_connected) = oneshot::channel();
        let backend =
            super::backend::start::<P>(wt_config, transport_config, conn_opts, send_connected);
        (Self { recv_connected }, backend)
    }

    pub fn poll(&mut self) -> Poll<ConnectedResult<P>> {
        match self.recv_connected.try_recv() {
            Ok(Some(result)) => Poll::Ready(result),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::BackendClosed)),
        }
    }
}

impl<P> ConnectedClient<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    pub fn client_info(&self) -> ClientInfo {
        ClientInfo {
            conn: self.conn_info.clone(),
            local_addr: self.local_addr,
        }
    }

    pub fn message_state(&self, msg: MessageTicket) -> MessageState {
        todo!()
    }

    pub fn send(&self, msg: impl Into<P::Send>) -> Result<MessageTicket, WebTransportError<P>> {
        let msg = msg.into();
        self.send_s
            .unbounded_send(msg)
            .map_err(|_| WebTransportError::BackendClosed)?;
        todo!()
    }

    pub fn update(&mut self) -> Result<Vec<ClientEvent<P>>, WebTransportError<P>> {
        todo!()
    }
}

impl<P> Transport for ClientWebTransport<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    type SendData = P::Send;

    type RecvData = Datagram;
}

impl<P> ClientTransport<P> for ClientWebTransport<P>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    type Error = WebTransportError<P>;

    type ClientInfo = ClientInfo;

    fn client_state(&self) -> ClientState<Self::ClientInfo> {
        match self {
            Self::Disconnected => ClientState::Disconnected,
            Self::Connecting(_) => ClientState::Connecting,
            Self::Connected(client) => ClientState::Connected {
                info: client.client_info(),
            },
        }
    }

    fn message_state(&self, msg: MessageTicket) -> MessageState {
        match self {
            Self::Disconnected => MessageState::Unsent,
            Self::Connecting(_) => MessageState::Unsent,
            Self::Connected(client) => client.message_state(msg),
        }
    }

    fn send(&self, msg: impl Into<P::Send>) -> Result<MessageTicket, Self::Error> {
        match self {
            Self::Disconnected => Err(WebTransportError::NotConnected),
            Self::Connecting(_) => Err(WebTransportError::NotConnected),
            Self::Connected(client) => client.send(msg),
        }
    }

    fn update(&mut self) -> impl Iterator<Item = ClientEvent<P>> {
        match self {
            Self::Disconnected => vec![],
            _ => todo!(),
        }
        .into_iter()
    }
}
