mod backend;
mod wrapper;

use tracing::debug;
pub use wrapper::*;

use std::{collections::HashMap, future::Future, marker::PhantomData, net::SocketAddr, task::Poll};

use aeronet::{
    client::ClientState, LaneConfig, MessageState, OnLane, ProtocolVersion, TransportProtocol,
    TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;

use crate::{shared::ConnectionFrontend, BackendError, ConnectionInfo, MessageKey};

slotmap::new_key_type! {
    /// Key identifying a unique client connected to a [`WebTransportServer`].
    pub struct ClientKey;
}

impl std::fmt::Display for ClientKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerMessageKey {
    client: ClientKey,
    msg: MessageKey,
}

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

type ServerEvent<P> =
    aeronet::server::ServerEvent<P, WebTransportError<P>, ClientKey, ServerMessageKey>;

pub struct WebTransportServerConfig {
    pub native: wtransport::ServerConfig,
    pub version: ProtocolVersion,
    pub max_packet_len: usize,
    pub lanes: Vec<LaneConfig>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpeningServer<P> {
    recv_open: oneshot::Receiver<Result<OpenInner, BackendError>>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct OpenInner {
    local_addr: SocketAddr,
    recv_client: mpsc::Receiver<ClientRequestingKey>,
    send_closed: oneshot::Sender<()>,
}

#[derive(Debug)]
struct ClientRequestingKey {
    send_key: oneshot::Sender<ClientKey>,
    recv_req: oneshot::Receiver<Result<ClientRequesting, BackendError>>,
}

#[derive(Debug)]
struct ClientRequesting {
    info: RemoteRequestingInfo,
    send_resp: oneshot::Sender<ConnectionResponse>,
    recv_conn: oneshot::Receiver<Result<ConnectionFrontend, BackendError>>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum Client {
    Incoming {
        recv_req: oneshot::Receiver<Result<ClientRequesting, BackendError>>,
    },
    Requesting {
        info: RemoteRequestingInfo,
        send_resp: Option<oneshot::Sender<ConnectionResponse>>,
        recv_conn: oneshot::Receiver<Result<ConnectionFrontend, BackendError>>,
    },
    Connected {
        conn: ConnectionFrontend,
    },
}

#[derive(Debug, Clone)]
pub struct RemoteRequestingInfo {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ConnectionResponse {
    Accepted,
    Rejected,
}

impl<P> OpeningServer<P>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
{
    pub fn open(config: WebTransportServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        let frontend = Self {
            recv_open,
            _phantom: PhantomData,
        };
        let backend = backend::open(config, send_open);
        (frontend, backend)
    }

    pub fn poll(&mut self) -> Poll<Result<OpenServer<P>, WebTransportError<P>>> {
        match self.recv_open.try_recv() {
            Ok(None) => Poll::Pending,
            Ok(Some(Ok(inner))) => Poll::Ready(Ok(OpenServer {
                local_addr: inner.local_addr,
                recv_client: inner.recv_client,
                clients: SlotMap::default(),
                _send_closed: inner.send_closed,
                _phantom: PhantomData,
            })),
            Ok(Some(Err(err))) => Poll::Ready(Err(err.into())),
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::Backend(BackendError::Closed))),
        }
    }
}

/// [`ServerState::Open`] variant of [`WebTransportServer`].
///
/// [`ServerState::Open`]: aeronet::server::ServerState::Open
#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenServer<P> {
    local_addr: SocketAddr,
    recv_client: mpsc::Receiver<ClientRequestingKey>,
    clients: SlotMap<ClientKey, Client>,
    _send_closed: oneshot::Sender<()>,
    _phantom: PhantomData<P>,
}

impl<P> OpenServer<P>
where
    P: TransportProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane,
    P::S2C: TryAsBytes + TryFromBytes + OnLane,
{
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    #[must_use]
    pub fn client_state(
        &self,
        client_key: ClientKey,
    ) -> ClientState<RemoteRequestingInfo, ConnectionInfo> {
        match self.clients.get(client_key) {
            None | Some(Client::Incoming { .. }) => ClientState::Disconnected,
            Some(Client::Requesting { info, .. }) => ClientState::Connecting(info.clone()),
            Some(Client::Connected { conn, .. }) => ClientState::Connected(conn.info.clone()),
        }
    }

    #[must_use]
    pub fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    #[must_use]
    pub fn message_state(&self, msg_key: ServerMessageKey) -> Option<MessageState> {
        None
    }

    pub fn accept_request(&mut self, client_key: ClientKey) -> Result<(), WebTransportError<P>> {
        self.respond_to_request(client_key, ConnectionResponse::Accepted)
    }

    pub fn reject_request(&mut self, client_key: ClientKey) -> Result<(), WebTransportError<P>> {
        self.respond_to_request(client_key, ConnectionResponse::Rejected)
    }

    fn respond_to_request(
        &mut self,
        client_key: ClientKey,
        resp: ConnectionResponse,
    ) -> Result<(), WebTransportError<P>> {
        match self.clients.get_mut(client_key) {
            None | Some(Client::Incoming { .. }) => {
                Err(WebTransportError::<P>::NoClient { client_key })
            }
            Some(Client::Requesting { send_resp, .. }) => match send_resp.take() {
                Some(send_resp) => {
                    let _ = send_resp.send(resp);
                    Ok(())
                }
                None => Err(WebTransportError::<P>::AlreadyRespondedToRequest),
            },
            Some(Client::Connected { .. }) => Err(WebTransportError::<P>::AlreadyConnected),
        }
    }

    pub fn send(
        &mut self,
        client_key: ClientKey,
        msg: impl Into<P::S2C>,
    ) -> Result<ServerMessageKey, WebTransportError<P>> {
        let Some(Client::Connected { conn }) = self.clients.get_mut(client_key) else {
            return Err(WebTransportError::<P>::NoClient { client_key });
        };

        let msg = msg.into();
        conn.buffer_send(&msg).map(|msg| ServerMessageKey {
            client: client_key,
            msg,
        })
    }

    pub fn disconnect(&mut self, client_key: ClientKey) -> Result<(), WebTransportError<P>> {
        match self.clients.remove(client_key) {
            None => Err(WebTransportError::<P>::NoClient { client_key }),
            Some(_) => Ok(()),
        }
    }

    pub fn poll(&mut self) -> Vec<ServerEvent<P>> {
        let mut events = Vec::new();

        while let Ok(Some(client)) = self.recv_client.try_next() {
            let client_key = self.clients.insert(Client::Incoming {
                recv_req: client.recv_req,
            });
            let _ = client.send_key.send(client_key);
            debug!("Assigned new client {client_key}");
            // don't send a connecting event yet;
            // send it once the user has the opportunity to accept/reject it
        }

        let mut clients_to_remove = Vec::new();
        for (client_key, client) in &mut self.clients {
            if let Err(reason) = Self::poll_client(client_key, client, &mut events) {
                clients_to_remove.push(client_key);
                if let Some(reason) = reason {
                    events.push(ServerEvent::Disconnected { client_key, reason });
                }
            }
        }
        for client_key in clients_to_remove {
            debug!("Removed client {client_key}");
            self.clients.remove(client_key);
        }

        events
    }

    fn poll_client(
        client_key: ClientKey,
        state: &mut Client,
        events: &mut Vec<ServerEvent<P>>,
    ) -> Result<(), Option<WebTransportError<P>>> {
        match state {
            Client::Incoming { recv_req } => match recv_req.try_recv() {
                Ok(None) => Ok(()),
                Ok(Some(Ok(requesting))) => {
                    *state = Client::Requesting {
                        info: requesting.info,
                        send_resp: Some(requesting.send_resp),
                        recv_conn: requesting.recv_conn,
                    };
                    events.push(ServerEvent::Connecting { client_key });
                    Ok(())
                }
                // silently remove, because we haven't actually emitted a
                // `Connecting` event for this client yet, so we can't send a
                // `Disconnected`
                Ok(Some(Err(_))) | Err(_) => Err(None),
            },
            Client::Requesting { recv_conn, .. } => match recv_conn.try_recv() {
                Ok(None) => Ok(()),
                Ok(Some(Ok(conn))) => {
                    *state = Client::Connected { conn };
                    events.push(ServerEvent::Connected { client_key });
                    Ok(())
                }
                Ok(Some(Err(err))) => Err(Some(err.into())),
                Err(_) => Err(Some(WebTransportError::<P>::Backend(BackendError::Closed))),
            },
            Client::Connected { conn } => {
                conn.update();
                while let Some(msg) = conn.recv()? {
                    events.push(ServerEvent::Recv { client_key, msg });
                }
                conn.recv_err()
                    .map_err(|err| Some(WebTransportError::<P>::Backend(err)))
            }
        }
    }
}
