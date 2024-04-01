mod wrapper;

use aeronet::{
    lane::LaneKind,
    message::{TryFromBytes, TryIntoBytes},
    protocol::{ProtocolVersion, TransportProtocol},
};
use aeronet_proto::seq::Seq;
use steamworks::{
    networking_sockets::NetworkingSockets,
    networking_types::{NetConnectionStatusChanged, NetworkingConnectionState, SendFlags},
};
pub use wrapper::*;

use derivative::Derivative;
use futures::channel::oneshot;
use std::{marker::PhantomData, net::SocketAddr, task::Poll};
use steamworks::{
    networking_sockets::NetConnection, networking_types::NetworkingIdentity, CallbackHandle,
    ClientManager, Manager, SteamId,
};

use crate::{internal::ConnectionFrontend, transport::ConnectionInfo};

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectingClient<P, M = ClientManager> {
    inner: Option<ClientInner<M>>,
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<Result<(), BackendError>>,
    #[derivative(Debug = "ignore")]
    status_changed_cb: CallbackHandle<M>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

#[derive(Debug, Clone, Copy)]
pub enum ConnectTarget {
    Ip { addr: SocketAddr },
    Peer { id: SteamId, virtual_port: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientMessageKey {
    msg_seq: Seq,
}

type SteamTransportError<P> = crate::transport::SteamTransportError<
    <P as TransportProtocol>::C2S,
    <P as TransportProtocol>::S2C,
>;

type ClientEvent<P> = aeronet::client::ClientEventFor<P, SteamTransportError<P>>;

#[derive(Debug)]
pub struct SteamClientTransportConfig {
    pub version: ProtocolVersion,
    pub max_packet_len: usize,
}

#[derive(Debug)]
enum BackendError {
    ConnectionRejected,
    ConnectionLost,
}

impl<S: TryIntoBytes, R: TryFromBytes> From<BackendError> for crate::SteamTransportError<S, R> {
    fn from(value: BackendError) -> Self {
        match value {
            BackendError::ConnectionRejected => crate::SteamTransportError::ConnectionRejected,
            BackendError::ConnectionLost => crate::SteamTransportError::ConnectionLost,
        }
    }
}

fn status_changed_to_result(
    event: &NetConnectionStatusChanged,
) -> Option<Result<(), BackendError>> {
    match event
        .connection_info
        .state()
        .unwrap_or(NetworkingConnectionState::None)
    {
        NetworkingConnectionState::Connecting | NetworkingConnectionState::FindingRoute => None,
        NetworkingConnectionState::Connected => Some(Ok(())),
        NetworkingConnectionState::ClosedByPeer => Some(Err(BackendError::ConnectionRejected)),
        NetworkingConnectionState::None | NetworkingConnectionState::ProblemDetectedLocally => {
            Some(Err(BackendError::ConnectionLost))
        }
    }
}

impl<P, M> Drop for ConnectingClient<P, M> {
    fn drop(&mut self) {
        self.status_changed_cb.disconnect();
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
struct ClientInner<M> {
    #[derivative(Debug = "ignore")]
    socks: NetworkingSockets<M>,
    #[derivative(Debug = "ignore")]
    steam: steamworks::Client<M>,
    #[derivative(Debug = "ignore")]
    conn: NetConnection<M>,
    config: SteamClientTransportConfig,
}

impl<P, M> ConnectingClient<P, M>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
    M: Manager + Send + Sync + 'static,
{
    pub fn connect(
        steam: steamworks::Client<M>,
        config: SteamClientTransportConfig,
    ) -> Result<Self, SteamTransportError<P>> {
        let socks = steam.networking_sockets();
        let conn = match config.target {
            ConnectTarget::Ip(target) => socks.connect_by_ip_address(target, []),
            ConnectTarget::Peer { id, virtual_port } => {
                socks.connect_p2p(NetworkingIdentity::new_steam_id(id), virtual_port, [])
            }
        }
        .map_err(|_| SteamTransportError::<P>::StartConnecting)?;

        let (send_connected, recv_connected) = oneshot::channel();
        let mut send_connected = Some(send_connected);
        let status_changed_cb = steam.register_callback(move |event| {
            Self::on_connection_status_changed(&mut send_connected, event);
        });

        Ok(Self {
            inner: Some(ClientInner {
                socks,
                steam,
                conn,
                config,
            }),
            recv_connected,
            status_changed_cb,
            _phantom: PhantomData,
        })
    }

    fn on_connection_status_changed(
        send_connected: &mut Option<oneshot::Sender<Result<(), BackendError>>>,
        event: NetConnectionStatusChanged,
    ) {
        if let Some(res) = status_changed_to_result(&event) {
            if let Some(send_connected) = send_connected.take() {
                let _ = send_connected.send(res);
            }
        }
    }

    pub fn poll(&mut self) -> Poll<Result<NegotiatingClient<P, M>, SteamTransportError<P>>> {
        match self.recv_connected.try_recv() {
            Ok(Some(Ok(()))) => {
                let inner = self
                    .inner
                    .take()
                    .expect("must not poll again after receiving next client state");
                let protocol_version = inner.config.version;
                Poll::Ready(Ok(NegotiatingClient {
                    inner: Some(inner),
                    negotiation: Negotiation::new(protocol_version),
                    state: NegotiationState::SendRequest,
                    _phantom: PhantomData,
                }))
            }
            Ok(Some(Err(err))) => Poll::Ready(Err(err.into())),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(SteamTransportError::<P>::InternalError)),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct NegotiatingClient<P, M = ClientManager> {
    inner: Option<ClientInner<M>>,
    negotiation: Negotiation,
    state: NegotiationState,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
enum NegotiationState {
    SendRequest,
    AwaitResponse,
}

impl<P, M> NegotiatingClient<P, M>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
    M: Manager + Send + Sync + 'static,
{
    pub fn poll(&mut self) -> Poll<Result<ConnectedClient<P, M>, SteamTransportError<P>>> {
        match self.state {
            NegotiationState::SendRequest => self.send_request(),
            NegotiationState::AwaitResponse => self.await_response(),
        }
    }

    fn send_request(&mut self) -> Poll<Result<ConnectedClient<P, M>, SteamTransportError<P>>> {
        let inner = self
            .inner
            .as_mut()
            .expect("must not poll again after receiving next client state");
        inner
            .conn
            .send_message(&self.negotiation.request(), SendFlags::RELIABLE_NO_NAGLE)
            .map_err(SteamTransportError::<P>::SendNegotiateRequest)?;
        self.state = NegotiationState::AwaitResponse;
        Poll::Pending
    }

    fn await_response(&mut self) -> Poll<Result<ConnectedClient<P, M>, SteamTransportError<P>>> {
        let inner = self
            .inner
            .as_mut()
            .expect("must not poll again after receiving next client state");
        let Some(msg) = inner
            .conn
            .receive_messages(1)
            .map_err(|_| SteamTransportError::<P>::Recv)?
            .pop()
        else {
            return Poll::Pending;
        };

        match self.negotiation.recv_response(msg.data()) {
            Ok(_) => {
                // next state
                let inner = self.inner.take().unwrap();
                let (send_err, recv_err) = oneshot::channel();
                let mut send_err = Some(send_err);
                let status_changed_cb = inner.steam.register_callback(move |event| {
                    ConnectedClient::<P, M>::on_connection_status_changed(&mut send_err, event);
                });

                Poll::Ready(Ok(ConnectedClient {
                    conn: ConnectionFrontend::new(
                        &inner.socks,
                        inner.conn,
                        inner.config.max_packet_len,
                        &inner.config.lanes,
                    ),
                    socks: inner.socks,
                    recv_err,
                    status_changed_cb,
                    _phantom: PhantomData,
                }))
            }
            Err(err) => Poll::Ready(Err(SteamTransportError::<P>::NegotiateResponse(err))),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct ConnectedClient<P, M = ClientManager> {
    conn: ConnectionFrontend<M>,
    #[derivative(Debug = "ignore")]
    socks: NetworkingSockets<M>,
    recv_err: oneshot::Receiver<BackendError>,
    #[derivative(Debug = "ignore")]
    status_changed_cb: CallbackHandle<M>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

impl<P, M> Drop for ConnectedClient<P, M> {
    fn drop(&mut self) {
        self.status_changed_cb.disconnect();
    }
}

impl<P, M> ConnectedClient<P, M>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
    M: Manager + Send + Sync + 'static,
{
    fn on_connection_status_changed(
        send_err: &mut Option<oneshot::Sender<BackendError>>,
        event: NetConnectionStatusChanged,
    ) {
        if let Some(Err(err)) = status_changed_to_result(&event) {
            if let Some(send_err) = send_err.take() {
                let _ = send_err.send(err);
            };
        }
    }

    #[must_use]
    pub fn info(&self) -> ConnectionInfo {
        self.conn.info.clone()
    }

    pub fn send(&mut self, msg: impl Into<P::C2S>) -> Result<(), SteamTransportError<P>> {
        let msg = msg.into();
        self.conn.send(msg)
    }

    pub fn poll(&mut self) -> (Vec<ClientEvent<P>>, Result<(), SteamTransportError<P>>) {
        let mut events = Vec::new();
        let result = self._poll(&mut events);
        (events, result)
    }

    fn _poll(&mut self, events: &mut Vec<ClientEvent<P>>) -> Result<(), SteamTransportError<P>> {
        self.conn.update(&self.socks);
        for msg in self.conn.recv::<P::C2S, P::S2C>() {
            let msg = msg?;
            events.push(ClientEvent::Recv { msg });
        }
        Ok(())
    }
}
