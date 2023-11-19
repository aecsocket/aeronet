use std::{io, marker::PhantomData, net::SocketAddr};

use aeronet::{ChannelKey, ChannelKind, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;
use tokio::sync::{mpsc, oneshot};
use wtransport::{
    endpoint::{endpoint_side, IncomingSession},
    Connection, ServerConfig, datagram::Datagram,
};

use crate::{EndpointInfo, CHANNEL_CAP, ChannelId, ChannelRecvError, ChannelSendError, ChannelEstablishError};

// pub fn create_server<C2S, S2C, C>(config: ServerConfig) ->
// (WebTransportServer<C2S, S2C, C>, impl Future<Output = ()> + Send) where
//     C2S: Message + TryFromBytes,
//     S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
//     C: ChannelKey,
// {
//     let (send_result, recv_result) = oneshot::channel();
// }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientKey(usize);

impl ClientKey {
    pub fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> usize {
        self.0
    }
}

// state machine

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<S2C, C2S, C>;

type ServerEvent<C2S, S2C, C> =
    aeronet::ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>;

type Endpoint = wtransport::Endpoint<endpoint_side::Server>;

#[derive(Derivative)]
#[derivative(Debug)]
struct Closed<C2S, S2C, C> {
    _phantom_c2s: PhantomData<C2S>,
    _phantom_s2c: PhantomData<S2C>,
    _phantom_c: PhantomData<C>,
}

impl<C2S, S2C, C> Closed<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    pub fn new() -> Self {
        Self {
            _phantom_c2s: PhantomData::default(),
            _phantom_s2c: PhantomData::default(),
            _phantom_c: PhantomData::default(),
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct Opening<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_result: oneshot::Receiver<Result<Open<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>>,
}

/*
ok so basic architecture:
    frontend:
        clients: HashMap<ClientKey, ClientState>
        recv_client: Receiver<ClientState>
    
    ClientState:
        recv_sig, send_req

    backend main task:
        send_client: Sender<ClientState>

    backend task number N, where N is a client key:
        send_sig, recv_req
*/

#[derive(Debug)]
enum Signal<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Incoming {
        authority: String,
        path: String,
        origin: Option<String>,
        user_agent: Option<String>,
    },
    Connected {
        info: EndpointInfo,
    },
    UpdateInfo {
        info: EndpointInfo,
    },
    Recv {
        msg: C2S,
    },
    Disconnected {
        reason: WebTransportError<C2S, S2C, C>,
    },
}

#[derive(Debug, Clone)]
enum Request<S2C> {
    Send { msg: S2C },
    Disconnect,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct Open<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    local_addr: Result<SocketAddr, io::Error>,
    clients: FxHashMap<ClientKey, ClientState<C2S, S2C, C>>,
    #[derivative(Debug = "ignore")]
    recv_client: mpsc::UnboundedReceiver<ClientState<C2S, S2C, C>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct ClientState<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    info: Option<EndpointInfo>,
    #[derivative(Debug = "ignore")]
    recv_sig: mpsc::UnboundedReceiver<Signal<C2S, S2C, C>>,
    #[derivative(Debug = "ignore")]
    send_req: mpsc::UnboundedSender<Request<S2C>>,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("no client with key {client:?}")]
pub struct NoClientError {
    pub client: ClientKey,
}

impl<C2S, S2C, C> Open<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    pub fn local_addr(&self) -> Result<SocketAddr, &io::Error> {
        self.local_addr.as_ref().map(|addr| *addr)
    }

    pub fn connection_info(&self, client: ClientKey) -> Option<EndpointInfo> {
        self.clients.get(&client).and_then(|state| state.info)
    }

    // TODO this method sig sucks
    pub fn send<M: Into<S2C>>(self, to: ClientKey, msg: M) -> Result<Result<Self, Closed<C2S, S2C, C>>, NoClientError> {
        let Some(state) = self.clients.get(&to) else {
            return Err(NoClientError { client: to });
        };
        let msg = msg.into();
        match state.send_req.send(Request::Send { msg }) {
            Ok(_) => Ok(Ok(self)),
            Err(_) => Ok(Err(self.close())),
        }
    }

    pub fn recv(
        mut self,
    ) -> (
        impl Iterator<Item = ServerEvent<C2S, S2C, C>>,
        Result<Self, Closed<C2S, S2C, C>>,
    ) {
        let mut events = Vec::new();
        let result = loop {
            match self.recv_sig.try_recv() {
                Ok(signal) => match signal {
                    Signal::Incoming { client, .. } => {
                        debug_assert!(!self.clients.contains_key(&client));
                        self.clients.insert(client, ClientState::Incoming);
                    }
                    Signal::Connected { client, info } => {
                        debug_assert!(matches!(
                            self.clients.get(&client),
                            Some(ClientState::Incoming)
                        ));
                        self.clients.insert(client, ClientState::Connected(info));
                        events.push(ServerEvent::Connected { client });
                    }
                    Signal::UpdateInfo { client, info } => {
                        debug_assert!(matches!(
                            self.clients.get(&client),
                            Some(ClientState::Connected(_))
                        ));
                        self.clients.insert(client, ClientState::Connected(info));
                    }
                    Signal::Recv { from, msg } => {
                        events.push(ServerEvent::Recv { from, msg });
                    }
                    Signal::Disconnected { client, reason } => {
                        debug_assert!(self.clients.contains_key(&client));
                        self.clients.remove(&client);
                        events.push(ServerEvent::Disconnected { client, reason });
                    }
                },
                Err(mpsc::error::TryRecvError::Empty) => break Ok(self),
                Err(mpsc::error::TryRecvError::Disconnected) => break Err(self.close()),
            }
        };

        (events.into_iter(), result)
    }

    pub fn disconnect(self, target: ClientKey) -> Result<Result<Self, Closed<C2S, S2C, C>>, NoClientError> {
        let Some(state) = self.clients.get(&target) else {
            return Err(NoClientError { client: target });
        };
        match state.send_req.send(Request::Disconnect) {
            Ok(_) => Ok(Ok(self)),
            Err(_) => Ok(Err(self.close())),
        }
    }

    pub fn close(self) -> Closed<C2S, S2C, C> {
        Closed::new()
    }
}

// backend

async fn backend<C2S, S2C, C>(
    config: ServerConfig,
    send_result: oneshot::Sender<Result<Open<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>>,
) where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    let endpoint = match create_endpoint(config).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_result.send(Err(err));
            return;
        }
    };

    let (send_client, recv_client) = mpsc::unbounded_channel();
    let next_state = Open::<C2S, S2C, C> {
        local_addr: endpoint.local_addr(),
        clients: FxHashMap::default(),
        recv_client,
    };
    let _ = send_result.send(Ok(next_state));

    for client in 0.. {
        let session = endpoint.accept().await;
        let client = ClientKey::from_raw(client);

        let (mut send_sig, recv_sig) = mpsc::unbounded_channel();
        let (send_req, recv_req) = mpsc::unbounded_channel();
        let client_state = ClientState {
            info: None,
            recv_sig,
            send_req,
        };
        if let Err(_) = send_client.send(client_state) {
            // frontend closed
            return;
        };

        tokio::spawn(async move {
            if let Err(reason) = handle_session::<C2S, S2C, C>(
                session,
                &mut send_sig,
                recv_req,
            ).await
            {
                let _ = send_sig.send(Signal::Disconnected { reason });
            }
        });
    }
}

async fn create_endpoint<C2S, S2C, C>(
    config: ServerConfig,
) -> Result<Endpoint, WebTransportError<C2S, S2C, C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Endpoint::server(config).map_err(WebTransportError::CreateEndpoint)
}

async fn handle_session<C2S, S2C, C>(
    session: IncomingSession,
    send_sig: &mut mpsc::UnboundedSender<Signal<C2S, S2C, C>>,
    mut recv_req: mpsc::UnboundedReceiver<Request<S2C>>,
) -> Result<(), WebTransportError<C2S, S2C, C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    let mut conn = accept_session(session, send_sig).await?;
    open_channels::<C2S, S2C, C>(&mut conn).await?;

    let _ = send_sig.send(Signal::Connected {
        info: EndpointInfo::from_connection(&conn),
    });
    loop {
        tokio::select! {
            result = recv_req.recv() => {
                let Some(req) = result else {
                    // frontend closed
                    return Ok(());
                };

                match req {
                    Request::Send { msg } => {
                        // TODO
                    }
                    Request::Disconnect => {
                        // TODO shut down tasks listening to streams as well
                        return Ok(());
                    }
                }
            }
            result = conn.receive_datagram() => {
                let datagram = match result {
                    Ok(datagram) => datagram,
                    Err(err) => return Err(WebTransportError::recv(ChannelId::Datagram, ChannelRecvError::RecvDatagram(err))),
                };

                let msg = deserialize::<C2S>(&datagram)
                    .map_err(|err| WebTransportError::recv(ChannelId::Datagram, err))?;
                let _ = send_sig.send(Signal::Recv { msg });
            }
        }
    }
}

fn deserialize<R>(buf: &[u8]) -> Result<R, ChannelRecvError<R>>
where
    R: Message + TryFromBytes,
{
    R::try_from_bytes(buf).map_err(ChannelRecvError::Deserialize)
}

async fn accept_session<C2S, S2C, C>(
    session: IncomingSession,
    send_sig: &mpsc::UnboundedSender<Signal<C2S, S2C, C>>,
) -> Result<Connection, WebTransportError<C2S, S2C, C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    let session = session.await.map_err(WebTransportError::IncomingSession)?;

    let _ = send_sig.send(Signal::Incoming {
        authority: session.authority().to_owned(),
        path: session.path().to_owned(),
        origin: session.origin().map(ToOwned::to_owned),
        user_agent: session.user_agent().map(ToOwned::to_owned),
    });

    let session = session
        .accept()
        .await
        .map_err(WebTransportError::AcceptSession)?;
    Ok(session)
}

async fn open_channels<C2S, S2C, C>(
    conn: &mut Connection,
) -> Result<Vec<ChannelId>, WebTransportError<C2S, S2C, C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
    C: ChannelKey,
{
    try_join_all(C::ALL.iter().enumerate().map(|(index, kind)| async move {}))
}

async fn open_stream<C2S, S2C>(conn: &mut Connection) -> Result<(), ChannelEstablishError>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + Clone,
{
    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(ChannelEstablishError::RequestOpenStream)?
        .await
        .map_err(ChannelEstablishError::OpenStream)?;

    tokio::spawn(async move {
        if let Err(err) = handle_stream::<C2S, S2C>().await {

        }
    });
    Ok(())
}

async fn handle_stream<S2C>() -> Result<(), ChannelSendError<S2C>> {
    
}
