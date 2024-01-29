use std::{future::Future, marker::PhantomData, net::SocketAddr, task::Poll};

use aeronet::{ClientKey, LaneProtocol, OnLane, TransportProtocol, TryAsBytes, TryFromBytes};
use derivative::Derivative;
use futures::channel::{mpsc, oneshot};
use slotmap::SlotMap;
use wtransport::ServerConfig;

use crate::{shared::ConnectionFrontend, BackendError};

mod backend;

type WebTransportError<P> =
    crate::WebTransportError<<P as TransportProtocol>::S2C, <P as TransportProtocol>::C2S>;

type ServerEvent<P> = aeronet::ServerEvent<P, (), (), WebTransportError<P>>;

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
pub struct OpenServer<P>
where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
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
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    send_resp: Option<oneshot::Sender<ConnectionResponse>>,
    recv_conn: oneshot::Receiver<Result<ConnectionFrontend, BackendError>>,
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
        let Some(Client::Requesting(client_state)) = self.clients.get_mut(client) else {
            return Err(WebTransportError::<P>::NoClient(client));
        };

        match client_state.send_resp.take() {
            Some(send_resp) => {
                let _ = send_resp.send(resp);
                Ok(())
            }
            None => Err(WebTransportError::<P>::AlreadyRespondedToRequest),
        }
    }

    pub fn update(&mut self) -> (Vec<ServerEvent<P>>, Result<(), WebTransportError<P>>) {
        let mut events = Vec::new();

        while let Ok(Some(mut incoming)) = self.recv_client.try_next() {
            let send_key = incoming
                .send_key
                .take()
                .expect("should have a sender after receiving client from backend");
            let client_key = self.clients.insert(Client::Incoming(incoming));
            let _ = send_key.send(client_key);
            events.push(ServerEvent::Connecting {
                client: client_key,
                info: (),
            });
        }

        let mut clients_to_remove = Vec::new();
        for (client_key, client_state) in self.clients.iter_mut() {
            update_client(
                client_key,
                client_state,
                &mut clients_to_remove,
                &mut events,
            );
        }

        for client_key in clients_to_remove {
            self.clients.remove(client_key);
        }

        (events, Ok(()))
    }
}

fn update_client<P>(
    client_key: ClientKey,
    client_state: &mut Client,
    clients_to_remove: &mut Vec<ClientKey>,
    events: &mut Vec<ServerEvent<P>>,
) where
    P: LaneProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnLane<Lane = P::Lane>,
{
    match client_state {
        Client::Incoming(incoming) => match incoming.recv_req.try_recv() {
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
        Client::Requesting(requesting) => {}
        Client::Incoming(incoming) => {}
        Client::Connected(connected) => {}
    }
}
