mod backend;
mod frontend;

pub use frontend::*;
use wtransport::ServerConfig;

use std::{future::Future, io, net::SocketAddr, task::Poll};

use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};

use crate::{ClientKey, EndpointInfo};

/// An event which is raised by a [`WebTransportServer`].
#[derive(Debug)]
pub enum ServerEvent<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    /// The server backend has been set up and is ready to accept connections.
    Opened,
    /// A client has requested to connect.
    /// 
    /// No further data is known about the client yet.
    Incoming {
        /// The key of the client.
        client: ClientKey,
    },
    /// The server has accepted a client's request to connect.
    Accepted {
        /// The key of the client.
        client: ClientKey,
        /// See [`wtransport::endpoint::SessionRequest::authority`].
        authority: String,
        /// See [`wtransport::endpoint::SessionRequest::path`].
        path: String,
        /// See [`wtransport::endpoint::SessionRequest::origin`].
        origin: Option<String>,
        /// See [`wtransport::endpoint::SessionRequest::user_agent`].
        user_agent: Option<String>,
    },
    /// A client has fully established a connection to the server (including
    /// opening streams) and the connection is ready for messages.
    /// 
    /// This is equivalent to [`aeronet::ServerEvent::Connected`].
    Connected {
        /// The key of the client.
        client: ClientKey,
    },
    /// A client sent a message to the server.
    /// 
    /// This is equivalent to [`aeronet::ServerEvent::Recv`].
    Recv {
        /// The key of the client which sent the message.
        from: ClientKey,
        /// The message.
        msg: C2S,
    },
    /// A client has lost connection from this server, which cannot be recovered
    /// from.
    /// 
    /// This is equivalent to [`aeronet::ServerEvent::Disconnected`].
    Disconnected {
        /// The key of the client.
        client: ClientKey,
        /// The reason why the client lost connection.
        cause: WebTransportError<C2S, S2C, C>,
    },
    /// The server backend has been shut down, all client connections have been
    /// dropped, and the backend must be re-opened.
    Closed {
        /// The reason why the backend was closed.
        cause: WebTransportError<C2S, S2C, C>,
    }
}

impl<C2S, S2C, C> From<ServerEvent<C2S, S2C, C>>
    for Option<aeronet::ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    fn from(value: ServerEvent<C2S, S2C, C>) -> Self {
        match value {
            ServerEvent::Connected { client } => Some(aeronet::ServerEvent::Connected { client }),
            ServerEvent::Recv { from, msg } => Some(aeronet::ServerEvent::Recv { from, msg }),
            ServerEvent::Disconnected { client, cause } => {
                Some(aeronet::ServerEvent::Disconnected { client, cause })
            }
            _ => None,
        }
    }
}

// server states

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<S2C, C2S, C>;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct OpeningServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_open: oneshot::Receiver<OpenResult<C2S, S2C, C>>,
}

impl<C2S, S2C, C> OpeningServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    pub fn new(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        (
            Self { recv_open },
            backend::start::<C2S, S2C, C>(config, send_open),
        )
    }

    pub fn poll(&mut self) -> Poll<OpenResult<C2S, S2C, C>> {
        match self.recv_open.try_recv() {
            Ok(result) => Poll::Ready(result),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => Poll::Ready(Err(WebTransportError::BackendClosed)),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct OpenServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    local_addr: Result<SocketAddr, io::Error>,
    clients: SlotMap<ClientKey, Client<C2S, S2C, C>>,
    #[derivative(Debug = "ignore")]
    recv_client: mpsc::UnboundedReceiver<IncomingClient<C2S, S2C, C>>,
    #[derivative(Debug = "ignore")]
    send_closed: mpsc::Sender<()>,
}

type OpenResult<C2S, S2C, C> = Result<OpenServer<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

impl<C2S, S2C, C> OpenServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    pub fn local_addr(&self) -> Result<SocketAddr, &io::Error> {
        self.local_addr.as_ref().map(|addr| *addr)
    }

    pub fn clients(&self) -> impl Iterator<Item = ClientKey> + '_ {
        self.clients.keys()
    }

    pub fn connection_info(&self, client: ClientKey) -> Option<EndpointInfo> {
        self.clients
            .get(client)
            .and_then(|client| match client {
                Client::Connected(client) => Some(client.info.clone()),
                _ => None,
            })
    }

    pub fn send<M: Into<S2C>>(&self, to: ClientKey, msg: M) -> Result<(), WebTransportError<C2S, S2C, C>> {
        let Some(client) = self.clients.get(to) else {
            return Err(WebTransportError::NoClient(to));
        };
        let Client::Connected(client) = client else {
            return Err(WebTransportError::NotConnected(to));
        };

        let msg = msg.into();
        client.send_s2c.send(msg).map_err(|_| WebTransportError::NotConnected(to))
    }

    pub fn disconnect(&mut self, target: ClientKey) -> Result<(), WebTransportError<C2S, S2C, C>> {
        match self.clients.remove(target) {
            Some(_) => Ok(()),
            None => Err(WebTransportError::NoClient(target)),
        }
    }

    pub fn recv(&mut self) -> Result<std::vec::IntoIter<ServerEvent<C2S, S2C, C>>, WebTransportError<C2S, S2C, C>> {
        let mut events = Vec::new();
        loop {
            match self.recv_client.try_recv() {
                Ok(client) => {
                    let client = self.clients.insert(Client::Incoming(client));
                    events.push(ServerEvent::Incoming { client });
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err(WebTransportError::BackendClosed);
                }
            }
        }

        let mut to_remove = Vec::new();
        for (client, state) in self.clients.iter_mut() {
            match state {
                Client::Incoming(incoming) => match incoming.recv_accepted.try_recv() {
                    Ok(Ok(accepted)) => {
                        events.push(ServerEvent::Accepted {
                            client,
                            authority: accepted.authority.clone(),
                            path: accepted.path.clone(),
                            origin: accepted.origin.clone(),
                            user_agent: accepted.user_agent.clone(),
                        });
                        *state = Client::Accepted(accepted);
                    }
                    Ok(Err(cause)) => {
                        events.push(ServerEvent::Disconnected { client, cause });
                        to_remove.push(client);
                    },
                    Err(oneshot::error::TryRecvError::Empty) => {},
                    Err(oneshot::error::TryRecvError::Closed) => {
                        events.push(ServerEvent::Disconnected { client, cause: WebTransportError::BackendClosed });
                        to_remove.push(client);
                    },
                }
                Client::Accepted(accepted) => match accepted.recv_connected.try_recv() {
                    Ok(Ok(connected)) => {
                        events.push(ServerEvent::Connected { client });
                        *state = Client::Connected(connected);
                    }
                    Ok(Err(cause)) => {
                        events.push(ServerEvent::Disconnected { client, cause });
                        to_remove.push(client);
                    }
                    Err(oneshot::error::TryRecvError::Empty) => {},
                    Err(oneshot::error::TryRecvError::Closed) => {
                        events.push(ServerEvent::Disconnected { client, cause: WebTransportError::BackendClosed });
                        to_remove.push(client);
                    }
                }
                Client::Connected(connected) => {
                    loop {
                        match connected.recv_info.try_recv() {
                            Ok(info) => connected.info = info,
                            Err(_) => break,
                        }
                    }

                    loop {
                        match connected.recv_c2s.try_recv() {
                            Ok(msg) => events.push(ServerEvent::Recv { from: client, msg }),
                            Err(_) => break,
                        }
                    }

                    match connected.recv_err.try_recv() {
                        Ok(cause) => {
                            println!("A!");
                            events.push(ServerEvent::Disconnected { client, cause });
                            to_remove.push(client);
                        }
                        Err(oneshot::error::TryRecvError::Empty) => {},
                        Err(oneshot::error::TryRecvError::Closed) => {
                            println!("B! {}", match connected.recv_c2s.try_recv() {
                                Err(x) => format!("{:?}", x),
                                Ok(_) => "ok".into(),
                            });
                            events.push(ServerEvent::Disconnected { client, cause: WebTransportError::BackendClosed });
                            to_remove.push(client);
                        }
                    }
                }
            }
        }

        for client in to_remove {
            self.clients.remove(client);
        }

        Ok(events.into_iter())
    }
}

// client states

#[derive(Debug)]
enum Client<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Incoming(IncomingClient<C2S, S2C, C>),
    Accepted(AcceptedClient<C2S, S2C, C>),
    Connected(ConnectedClient<C2S, S2C, C>),
}

#[derive(Derivative)]
#[derivative(Debug)]
struct IncomingClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_accepted: oneshot::Receiver<AcceptedClientResult<C2S, S2C, C>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct AcceptedClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    #[derivative(Debug = "ignore")]
    recv_connected: oneshot::Receiver<ConnectedClientResult<C2S, S2C, C>>,
}

type AcceptedClientResult<C2S, S2C, C> =
    Result<AcceptedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct ConnectedClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    info: EndpointInfo,
    #[derivative(Debug = "ignore")]
    recv_info: mpsc::UnboundedReceiver<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_c2s: mpsc::UnboundedReceiver<C2S>,
    #[derivative(Debug = "ignore")]
    send_s2c: mpsc::UnboundedSender<S2C>,
    #[derivative(Debug = "ignore")]
    recv_err: oneshot::Receiver<WebTransportError<C2S, S2C, C>>,
}

type ConnectedClientResult<C2S, S2C, C> =
    Result<ConnectedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;
