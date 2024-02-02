mod backend;
mod wrapper;

pub use wrapper::*;

use std::{collections::HashMap, future::Future, marker::PhantomData, net::SocketAddr, task::Poll};

use aeronet::{
    ClientKey, ClientState, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;
use wtransport::ServerConfig;

use crate::{shared::ConnectionFrontend, BackendError, ConnectionInfo};

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

type ServerEvent<P> = aeronet::ServerEvent<P, WebTransportError<P>>;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpeningServer<P: TransportProtocol> {
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
            Ok(Some(Ok(inner))) => Poll::Ready(Ok(OpenServer {
                local_addr: inner.local_addr,
                recv_client: inner.recv_client,
                clients: SlotMap::default(),
                _send_closed: inner._send_closed,
                _phantom: PhantomData::default(),
            })),
            Ok(Some(Err(err))) => Poll::Ready(Err(WebTransportError::<P>::Backend(err))),
            Ok(None) => Poll::Pending,
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::Backend(BackendError::Closed))),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenServer<P: TransportProtocol> {
    local_addr: SocketAddr,
    recv_client: mpsc::Receiver<ClientIncoming>,
    clients: SlotMap<ClientKey, Client>,
    _send_closed: oneshot::Sender<()>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct ClientIncoming {
    send_key: Option<oneshot::Sender<ClientKey>>,
    recv_req: oneshot::Receiver<Result<ClientRequesting, BackendError>>,
}

#[derive(Debug)]
struct ClientRequesting {
    info: RemoteConnectingClientInfo,
    send_resp: Option<oneshot::Sender<ConnectionResponse>>,
    recv_conn: oneshot::Receiver<Result<ConnectionFrontend, BackendError>>,
}

#[derive(Debug, Clone)]
pub struct RemoteConnectingClientInfo {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
}

#[derive(Debug)]
enum Client {
    Incoming(ClientIncoming),
    Requesting(ClientRequesting),
    Connected(ConnectionFrontend),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    Accepted,
    Forbidden,
}

impl<P> OpenServer<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn client_state(
        &self,
        client_key: ClientKey,
    ) -> ClientState<RemoteConnectingClientInfo, ConnectionInfo> {
        match self.clients.get(client_key) {
            None | Some(Client::Incoming(_)) => ClientState::Disconnected,
            Some(Client::Requesting(client)) => ClientState::Connecting(client.info.clone()),
            Some(Client::Connected(client)) => ClientState::Connected(todo!()),
        }
    }

    pub fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    pub fn accept_request(&mut self, client_key: ClientKey) -> Result<(), WebTransportError<P>> {
        self.respond_to_request(client_key, ConnectionResponse::Accepted)
    }

    pub fn reject_request(&mut self, client_key: ClientKey) -> Result<(), WebTransportError<P>> {
        self.respond_to_request(client_key, ConnectionResponse::Forbidden)
    }

    fn respond_to_request(
        &mut self,
        client_key: ClientKey,
        resp: ConnectionResponse,
    ) -> Result<(), WebTransportError<P>> {
        let Some(Client::Requesting(client)) = self.clients.get_mut(client_key) else {
            return Err(WebTransportError::<P>::NoClient(client_key));
        };

        match client.send_resp.take() {
            Some(send_resp) => {
                let _ = send_resp.send(resp);
                Ok(())
            }
            None => Err(WebTransportError::<P>::AlreadyRespondedToRequest),
        }
    }

    pub fn send(
        &mut self,
        client_key: ClientKey,
        msg: impl Into<P::S2C>,
    ) -> Result<(), WebTransportError<P>> {
        todo!()
    }

    pub fn disconnect(&mut self, client_key: ClientKey) -> Result<(), WebTransportError<P>> {
        todo!()
    }

    pub fn update(&mut self) -> (Vec<ServerEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();

        while let Ok(Some(mut client)) = self.recv_client.try_next() {
            let send_key = client
                .send_key
                .take()
                .expect("should have a sender after receiving client from backend");
            let client_key = self.clients.insert(Client::Incoming(client));
            let _ = send_key.send(client_key);
            events.push(ServerEvent::Connecting { client: client_key });
        }

        let mut clients_to_remove = Vec::new();
        for (client_key, client) in self.clients.iter_mut() {
            update_client(client_key, client, &mut clients_to_remove, &mut events);
        }

        for client_key in clients_to_remove {
            self.clients.remove(client_key);
        }

        (events, Ok(()))
    }
}

fn update_client<P>(
    client_key: ClientKey,
    client: &mut Client,
    clients_to_remove: &mut Vec<ClientKey>,
    events: &mut Vec<ServerEvent<P>>,
) where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    match client {
        Client::Incoming(client) => match client.recv_req.try_recv() {
            Ok(Some(Ok(requesting))) => todo!(),
            Ok(Some(Err(x))) => todo!(),
            Ok(None) => {}
            Err(_) => {
                events.push(ServerEvent::Disconnected {
                    client: client_key,
                    reason: WebTransportError::<P>::Backend(BackendError::Closed),
                });
                clients_to_remove.push(client_key);
            }
        },
        Client::Requesting(client) => {}
        Client::Incoming(client) => {}
        Client::Connected(client) => {}
    }
}
