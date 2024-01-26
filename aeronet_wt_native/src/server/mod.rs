use std::{future::Future, marker::PhantomData, net::SocketAddr, task::Poll};

use aeronet::{ClientKey, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;
use wtransport::ServerConfig;

use crate::{shared::SyncConnection, BackendError};

mod backend;

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpeningServer<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    recv_open: oneshot::Receiver<Result<OpenServerInner, BackendError>>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct OpenServerInner {
    local_addr: SocketAddr,
    recv_client: mpsc::Receiver<ClientIncoming>,
    _send_closed: oneshot::Sender<()>,
}

impl<P> OpeningServer<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    pub fn connect(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        let frontend = Self {
            recv_open,
            _phantom: PhantomData::default(),
        };
        let backend = backend::open(config, send_open);
        (frontend, backend)
    }

    pub fn poll(&mut self) -> Poll<Result<OpenServer<P>, WebTransportError<P>>> {
        match self.recv_open.try_recv() {
            Ok(Some(Ok(_))) => {
                todo!()
            }
            Ok(Some(Err(err))) => Poll::Ready(Err(WebTransportError::<P>::Backend(err))),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::Backend(BackendError::Closed))),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenServer<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    inner: OpenServerInner,
    clients: SlotMap<ClientKey, ClientState>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct ClientIncoming {
    send_key: oneshot::Sender<ClientKey>,
    recv_req: oneshot::Receiver<Result<ClientRequesting, BackendError>>,
}

#[derive(Debug)]
struct ClientRequesting {
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    send_resp: oneshot::Sender<SessionResponse>,
    recv_conn: oneshot::Receiver<Result<SyncConnection, BackendError>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionResponse {
    Accept,
    Forbidden,
}

#[derive(Debug)]
enum ClientState {}

impl<P> OpenServer<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
}
