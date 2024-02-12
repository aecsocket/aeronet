mod backend;
mod wrapper;

use tracing::debug;
pub use wrapper::*;

use std::{collections::HashMap, future::Future, marker::PhantomData, net::SocketAddr, task::Poll};

use aeronet::{
    ClientKey, ClientState, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes,
    VersionedProtocol,
};
use bytes::Bytes;
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;
use wtransport::ServerConfig;

use crate::{shared::ConnectionFrontend, BackendError, ConnectionInfo};

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

type ServerEvent<P> = aeronet::ServerEvent<P, WebTransportError<P>>;

/// [`ServerState::Opening`] variant of [`WebTransportServer`].
///
/// [`ServerState::Opening`]: aeronet::ServerState::Opening
#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpeningServer<P> {
    recv_open: oneshot::Receiver<Result<OpenServerInner, BackendError>>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct OpenServerInner {
    local_addr: SocketAddr,
    recv_client: mpsc::Receiver<ClientRequestingKey>,
    _send_closed: oneshot::Sender<()>,
}

impl<P> OpeningServer<P>
where
    P: LaneProtocol + VersionedProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    pub fn open(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        let frontend = Self {
            recv_open,
            _phantom: PhantomData,
        };
        let backend = backend::open::<P>(config, send_open);
        (frontend, backend)
    }

    pub fn poll(&mut self) -> Poll<Result<OpenServer<P>, WebTransportError<P>>> {
        match self.recv_open.try_recv() {
            Ok(None) => Poll::Pending,
            Ok(Some(Ok(inner))) => Poll::Ready(Ok(OpenServer {
                local_addr: inner.local_addr,
                recv_client: inner.recv_client,
                clients: SlotMap::default(),
                _send_closed: inner._send_closed,
                _phantom: PhantomData,
            })),
            Ok(Some(Err(err))) => Poll::Ready(Err(err.into())),
            Err(_) => Poll::Ready(Err(WebTransportError::<P>::backend_closed())),
        }
    }
}

/// [`ServerState::Open`] variant of [`WebTransportServer`].
///
/// [`ServerState::Open`]: aeronet::ServerState::Open
#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct OpenServer<P> {
    local_addr: SocketAddr,
    recv_client: mpsc::Receiver<ClientRequestingKey>,
    clients: SlotMap<ClientKey, Client>,
    _send_closed: oneshot::Sender<()>,
    _phantom: PhantomData<P>,
}

#[derive(Debug)]
struct ClientRequestingKey {
    send_key: oneshot::Sender<ClientKey>,
    recv_req: oneshot::Receiver<Result<ClientRequesting, BackendError>>,
}

#[derive(Debug)]
struct ClientIncoming {
    recv_req: oneshot::Receiver<Result<ClientRequesting, BackendError>>,
}

#[derive(Debug)]
struct ClientRequesting {
    info: ClientRequestingInfo,
    send_resp: Option<oneshot::Sender<ConnectionResponse>>,
    recv_conn: oneshot::Receiver<Result<ConnectionFrontend, BackendError>>,
}

#[derive(Debug)]
struct ClientConnected {
    conn: ConnectionFrontend,
    // TODO lane state
}

#[derive(Debug)]
enum Client {
    Incoming(ClientIncoming),
    Requesting(ClientRequesting),
    Connected(ClientConnected),
}

#[derive(Debug, Clone)]
pub struct ClientRequestingInfo {
    pub authority: String,
    pub path: String,
    pub origin: Option<String>,
    pub user_agent: Option<String>,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ConnectionResponse {
    Accepted,
    Forbidden,
}

impl<P> OpenServer<P>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn client_state(
        &self,
        client: ClientKey,
    ) -> ClientState<ClientRequestingInfo, ConnectionInfo> {
        match self.clients.get(client) {
            None | Some(Client::Incoming(_)) => ClientState::Disconnected,
            Some(Client::Requesting(client)) => ClientState::Connecting(client.info.clone()),
            Some(Client::Connected(client)) => ClientState::Connected(client.conn.info.clone()),
        }
    }

    pub fn client_keys(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    pub fn accept_request(&mut self, client: ClientKey) -> Result<(), WebTransportError<P>> {
        self.respond_to_request(client, ConnectionResponse::Accepted)
    }

    pub fn reject_request(&mut self, client: ClientKey) -> Result<(), WebTransportError<P>> {
        self.respond_to_request(client, ConnectionResponse::Forbidden)
    }

    fn respond_to_request(
        &mut self,
        client: ClientKey,
        resp: ConnectionResponse,
    ) -> Result<(), WebTransportError<P>> {
        let Some(Client::Requesting(client)) = self.clients.get_mut(client) else {
            return Err(WebTransportError::<P>::NoClient(client));
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
        client: ClientKey,
        msg: impl Into<P::S2C>,
    ) -> Result<(), WebTransportError<P>> {
        let Some(Client::Connected(client)) = self.clients.get_mut(client) else {
            return Err(WebTransportError::<P>::NoClient(client));
        };

        // TODO not actually how it works cause we have to do frag and stuff
        let msg: P::S2C = msg.into();
        let buf = msg.try_as_bytes().map_err(WebTransportError::<P>::Encode)?;
        let buf = Bytes::from(buf.as_ref().to_vec());
        client
            .conn
            .send(buf)
            .map_err(|_| WebTransportError::<P>::backend_closed())
    }

    pub fn disconnect(&mut self, client: ClientKey) -> Result<(), WebTransportError<P>> {
        match self.clients.remove(client) {
            None => Err(WebTransportError::<P>::NoClient(client)),
            Some(_) => Ok(()),
        }
    }

    pub fn update(&mut self) -> (Vec<ServerEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();

        while let Ok(Some(client)) = self.recv_client.try_next() {
            let client_key = self.clients.insert(Client::Incoming(ClientIncoming {
                recv_req: client.recv_req,
            }));
            let _ = client.send_key.send(client_key);
            // don't send a connecting event yet;
            // send it once the user has the opportunity to accept/reject it
        }

        let mut clients_to_remove = Vec::new();
        for (client_key, client) in self.clients.iter_mut() {
            if let Err(reason) = update_client(client_key, client, &mut events) {
                clients_to_remove.push(client_key);
                if let Some(reason) = reason {
                    events.push(ServerEvent::Disconnected {
                        client: client_key,
                        reason,
                    });
                }
            }
        }
        for client_key in clients_to_remove {
            self.clients.remove(client_key);
        }

        (events, Ok(()))
    }
}

fn update_client<P>(
    client_key: ClientKey,
    state: &mut Client,
    events: &mut Vec<ServerEvent<P>>,
) -> Result<(), Option<WebTransportError<P>>>
where
    P: LaneProtocol,
    P::C2S: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
    P::S2C: TryAsBytes + TryFromBytes + OnLane<Lane = P::Lane>,
{
    match state {
        Client::Incoming(client) => match client.recv_req.try_recv() {
            Ok(None) => Ok(()),
            Ok(Some(Ok(requesting))) => {
                *state = Client::Requesting(requesting);
                events.push(ServerEvent::Connecting { client: client_key });
                Ok(())
            }
            // silently remove, because we haven't actually emitted a
            // `Connecting` event for this client yet, so we can't send a
            // `Disconnected`
            Ok(Some(Err(_))) => Err(None),
            Err(_) => Err(None),
        },
        Client::Requesting(client) => match client.recv_conn.try_recv() {
            Ok(None) => Ok(()),
            Ok(Some(Ok(conn))) => {
                *state = Client::Connected(ClientConnected { conn });
                events.push(ServerEvent::Connected { client: client_key });
                Ok(())
            }
            Ok(Some(Err(err))) => Err(Some(err.into())),
            Err(_) => Err(Some(WebTransportError::<P>::backend_closed())),
        },
        Client::Connected(client) => {
            client.conn.update();

            while let Some(packet) = client.conn.recv() {
                // TODO this isnt how it actually works but like
                let msg = P::C2S::try_from_bytes(&packet)
                    .map_err(|err| Some(WebTransportError::<P>::Decode(err)))?;
                events.push(ServerEvent::Recv {
                    client: client_key,
                    msg,
                });
            }

            client.conn.recv_err().map_err(|err| {
                debug!(
                    "{client_key} disconnected: {:#}",
                    aeronet::util::pretty_error(&err)
                );
                Some(WebTransportError::<P>::Backend(err))
            })
        }
    }
}
