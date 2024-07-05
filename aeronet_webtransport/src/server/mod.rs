mod backend;

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    future::Future,
    io,
    net::SocketAddr,
};

use aeronet::{
    client::ClientState,
    lane::LaneIndex,
    server::{ServerState, ServerTransport},
};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use octs::Bytes;
use slotmap::SlotMap;
use wtransport::error::{ConnectionError, SendDatagramError};

use crate::shared::{ConnectionStats, MessageKey};

#[derive(Derivative)]
#[derivative(Debug = "transparent")]
pub struct WebTransportServer {
    state: State,
}

type State = ServerState<Opening, Open>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    Accept,
    Forbidden,
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    // frontend
    #[error("frontend closed")]
    FrontendClosed,
    #[error("already opening or open")]
    AlreadyOpen,
    #[error("already closed")]
    AlreadyClosed,

    // backend
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to get endpoint local address")]
    GetLocalAddr(#[source] io::Error),
    #[error("failed to await session request")]
    AwaitSessionRequest(#[source] ConnectionError),
    #[error("failed to accept session request")]
    AcceptSessionRequest(#[source] ConnectionError),
    #[error("server forced disconnect")]
    ForceDisconnect,
    #[error("failed to send datagram")]
    SendDatagram(#[source] SendDatagramError),
}

#[derive(Debug)]
pub struct Opening {
    recv_open: oneshot::Receiver<Open>,
    recv_err: oneshot::Receiver<ServerError>,
}

#[derive(Debug)]
pub struct Open {
    pub local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<Connecting>,
    clients: SlotMap<ClientKey, Client>,
    _send_closed: oneshot::Sender<()>,
}

type Client = ClientState<Connecting, Connected>;

#[derive(Debug)]
pub struct Connecting {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
    send_key: oneshot::Sender<ClientKey>,
    send_conn_resp: oneshot::Sender<ConnectionResponse>,
    recv_err: oneshot::Receiver<ServerError>,
    recv_connected: oneshot::Receiver<Connected>,
}

#[derive(Debug)]
pub struct Connected {
    pub remote_addr: SocketAddr,
    pub stats: ConnectionStats,
    recv_c2s: mpsc::Receiver<Bytes>,
    send_s2c: mpsc::UnboundedSender<Bytes>,
    recv_stats: mpsc::Receiver<ConnectionStats>,
}

slotmap::new_key_type! {
    pub struct ClientKey;
}

impl Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl WebTransportServer {
    pub fn closed() -> Self {
        Self {
            state: State::Closed,
        }
    }

    pub fn open_new(config: wtransport::ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel::<Open>();
        let (send_err, recv_err) = oneshot::channel::<ServerError>();

        let frontend = Self {
            state: State::Opening(Opening {
                recv_open,
                recv_err,
            }),
        };
        let backend = async move {
            if let Err(err) = backend::start(config, send_open).await {
                let _ = send_err.send(err);
            }
        };
        (frontend, backend)
    }

    pub fn open(
        &mut self,
        config: wtransport::ServerConfig,
    ) -> Result<impl Future<Output = ()> + Send, ServerError> {
        match self.state {
            State::Closed => {
                let (frontend, backend) = Self::open_new(config);
                *self = frontend;
                Ok(backend)
            }
            State::Opening(_) | State::Open(_) => Err(ServerError::AlreadyOpen),
        }
    }

    pub fn close(&mut self) -> Result<(), ServerError> {
        match self.state {
            State::Closed => Err(ServerError::AlreadyClosed),
            State::Opening(_) | State::Open(_) => {
                self.state = State::Closed;
                Ok(())
            }
        }
    }
}

impl ServerTransport for WebTransportServer {
    type Error = ServerError;

    type Opening<'this> = &'this Opening;

    type Open<'this> = &'this Open;

    type Connecting<'this> = &'this Connecting;

    type Connected<'this> = &'this Connected;

    type ClientKey = ClientKey;

    type MessageKey = MessageKey;

    fn state(&self) -> ServerState<Self::Opening<'_>, Self::Open<'_>> {
        self.state.as_ref()
    }

    fn client_state(
        &self,
        client_key: Self::ClientKey,
    ) -> ClientState<Self::Connecting<'_>, Self::Connected<'_>> {
        let State::Open(server) = &self.state else {
            return ClientState::Disconnected;
        };
        server
            .clients
            .get(client_key)
            .map(ClientState::as_ref)
            .unwrap_or(ClientState::Disconnected)
    }

    fn client_keys(&self) -> impl Iterator<Item = Self::ClientKey> + '_ {
        match &self.state {
            State::Closed | State::Opening(_) => None,
            State::Open(server) => Some(server.clients.keys()),
        }
        .into_iter()
        .flatten()
    }
}
