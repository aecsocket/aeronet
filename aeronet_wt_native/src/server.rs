use std::{convert::Infallible, future::Future, io, marker::PhantomData, net::SocketAddr};

use aeronet::Message;
use aeronet_wt_core::{ChannelId, Channels, OnChannel};
use futures::{stream::FuturesUnordered, StreamExt};
use tokio::sync::{broadcast, mpsc, oneshot};
use wtransport::{
    endpoint::IncomingSession,
    error::{ConnectionError, StreamOpeningError},
    Connection, RecvStream, SendStream, ServerConfig,
};

#[derive(Debug, thiserror::Error)]
pub enum WebTransportError {
    #[error("backend closed")]
    BackendClosed,
    #[error("failed to create endpoint")]
    CreateEndpoint(#[source] io::Error),
    #[error("failed to receive incoming session")]
    IncomingSession(#[source] ConnectionError),
    #[error("failed to accept incoming session")]
    AcceptSession(#[source] ConnectionError),
    #[error("on {on:?}")]
    OnChannel {
        on: ChannelId,
        #[source]
        source: ChannelError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("failed to request to open stream")]
    RequestOpenStream(#[source] ConnectionError),
    #[error("failed to open stream")]
    OpenStream(#[source] StreamOpeningError),
}

impl WebTransportError {
    pub fn on(on: ChannelId, source: ChannelError) -> Self {
        Self::OnChannel { on, source }
    }
}

pub type Result<T, E = WebTransportError> = std::result::Result<T, E>;

type Endpoint = wtransport::endpoint::Endpoint<wtransport::endpoint::endpoint_side::Server>;

pub enum Poll<S, R> {
    Pending(S),
    Ready(R),
}

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

const INTERNAL_CAP: usize = 128;
const DATA_CAP: usize = 65536;

//

pub struct Closed<C2S, S2C, C> {
    _phantom_c2s: PhantomData<C2S>,
    _phantom_s2c: PhantomData<S2C>,
    _phantom_c: PhantomData<C>,
}

impl<C2S, S2C, C> Closed<C2S, S2C, C>
where
    C2S: Message,
    S2C: Message + Clone,
    C: Channels,
{
    pub fn new() -> Self {
        Self {
            _phantom_c2s: PhantomData::default(),
            _phantom_s2c: PhantomData::default(),
            _phantom_c: PhantomData::default(),
        }
    }

    pub fn create(self, config: ServerConfig) -> (Creating<C2S, S2C, C>, impl Future) {
        let (send_next, recv_next) = oneshot::channel::<Result<Open<C2S, S2C, C>>>();
        let front = Creating { recv_next };
        let back = backend::<C2S, S2C, C>(config, send_next);
        (front, back)
    }
}

async fn backend<C2S, S2C, C>(
    config: ServerConfig,
    send_next: oneshot::Sender<Result<Open<C2S, S2C, C>>>,
) where
    C2S: Message,
    S2C: Message + Clone,
    C: Channels,
{
    let endpoint = match create_endpoint(config).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_next.send(Err(err));
            return;
        }
    };

    let local_addr = endpoint.local_addr();
    let (send_sig, recv_sig) = mpsc::unbounded_channel::<Signal<C2S>>();
    let (send_req, _) = broadcast::channel::<Request<S2C>>(INTERNAL_CAP);
    let next = Open {
        local_addr,
        recv_sig,
        send_req: send_req.clone(),
        _phantom_c: PhantomData::default(),
    };
    let _ = send_next.send(Ok(next));

    for client in 0.. {
        let client = ClientKey::from_raw(client);
        let session = endpoint.accept().await;
        let _ = send_sig.send(Signal::Incoming { client });

        let mut send_sig = send_sig.clone();
        let recv_req = send_req.subscribe();
        tokio::spawn(async move {
            let reason = handle_session::<C2S, S2C, C>(session, client, &mut send_sig, recv_req)
                .await
                .unwrap_err();
            let _ = send_sig.send(Signal::Disconnected { client, reason });
        });
    }
}

async fn create_endpoint(config: ServerConfig) -> Result<Endpoint> {
    Endpoint::server(config).map_err(WebTransportError::CreateEndpoint)
}

async fn handle_session<C2S, S2C, C>(
    session: IncomingSession,
    client: ClientKey,
    send_sig: &mut mpsc::UnboundedSender<Signal<C2S>>,
    recv_req: broadcast::Receiver<Request<S2C>>,
) -> Result<Infallible>
where
    C: Channels,
{
    let mut conn = accept_session::<C2S>(session, client, send_sig).await?;
    open_streams::<C>(&conn).await;

    Err(WebTransportError::BackendClosed)
}

async fn accept_session<C2S>(
    session: IncomingSession,
    client: ClientKey,
    send_sig: &mut mpsc::UnboundedSender<Signal<C2S>>,
) -> Result<Connection> {
    let req = session.await.map_err(WebTransportError::IncomingSession)?;

    let _ = send_sig.send(Signal::Accepted {
        client,
        authority: req.authority().to_owned(),
        path: req.path().to_owned(),
        origin: req.origin().map(ToOwned::to_owned),
        user_agent: req.user_agent().map(ToOwned::to_owned),
    });

    let conn = req
        .accept()
        .await
        .map_err(WebTransportError::AcceptSession)?;

    Ok(conn)
}

//
// IMPORTANT:
// The SERVER will OPEN the streams!!!
// The CLIENT will ACCEPT them!!!
//

async fn open_streams<C2S, S2C, C>(
    client: ClientKey,
    conn: &Connection,
    send_sig: mpsc::Sender<Signal<C2S>>,
    recv_req: broadcast::Receiver<Request<S2C>>,
) -> Result<Vec<oneshot::Receiver<ChannelError>>>
where
    C2S: Message,
    S2C: Message + Clone + OnChannel<Channel = C>,
    C: Channels,
{
    (0..C::NUM_STREAMS).map(|stream_id| {
        let channel = ChannelId::Stream(stream_id);
        let send_sig = send_sig.clone();
        let recv_req = recv_req.resubscribe();
        async move {
            let recv_error = open_stream::<C2S, S2C>(client, conn, channel, send_sig, recv_req)
                .await
                .map_err(|err| WebTransportError::on(channel, err))?;
            Ok::<_, WebTransportError>(recv_error)
        }
    })
    .collect::<FuturesUnordered<_>>()
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .collect::<Result<_>>()
}

async fn open_stream<C2S, S2C>(
    client: ClientKey,
    conn: &Connection,
    channel: ChannelId,
    send_sig: mpsc::Sender<Signal<C2S>>,
    recv_req: broadcast::Receiver<Request<S2C>>,
) -> Result<oneshot::Receiver<ChannelError>, ChannelError>
where
    C2S: Message,
    S2C: Message + Clone + OnChannel,
{
    let (send, recv) = conn
        .open_bi()
        .await
        .map_err(ChannelError::RequestOpenStream)?
        .await
        .map_err(ChannelError::OpenStream)?;

    let (send_err, recv_err) = oneshot::channel::<ChannelError>();
    tokio::spawn(async move {
        if let Err(err) = handle_stream(client, channel, send, recv, send_sig, recv_req).await {
            let _ = send_err.send(err);
        }
    });

    Ok(recv_err)
}

async fn handle_stream<C2S, S2C>(
    client: ClientKey,
    channel: ChannelId,
    mut send: SendStream,
    mut recv: RecvStream,
    send_sig: mpsc::Sender<Signal<C2S>>,
    mut recv_req: broadcast::Receiver<Request<S2C>>,
) -> Result<(), ChannelError>
where
    C2S: Message,
    S2C: Message + Clone + OnChannel,
{
    let mut buf = [0u8; DATA_CAP];
    loop {
        tokio::select! {
            req = recv_req.recv() => {
                let Ok(req) = req else {
                    // frontend closed
                    return Ok(());
                };
                match req {
                    Request::Send { to, msg } if to == client && msg.channel().channel_id() == channel => {
                        // write
                    }
                    _ => {}
                }
            }
            read = recv.read(&mut buf) => {

            }
        }
    }
}

//

#[derive(Debug)]
pub enum Signal<C2S> {
    Incoming {
        client: ClientKey,
    },
    Accepted {
        client: ClientKey,
        authority: String,
        path: String,
        origin: Option<String>,
        user_agent: Option<String>,
    },
    Connected {
        client: ClientKey,
    },
    Recv {
        from: ClientKey,
        msg: C2S,
    },
    Disconnected {
        client: ClientKey,
        reason: WebTransportError,
    },
}

#[derive(Debug, Clone)]
enum Request<S2C> {
    Send { to: ClientKey, msg: S2C },
    Disconnect { client: ClientKey },
}

pub struct Creating<C2S, S2C, C> {
    recv_next: oneshot::Receiver<Result<Open<C2S, S2C, C>>>,
}

impl<C2S, S2C, C> Creating<C2S, S2C, C> {
    pub fn poll(mut self) -> Poll<Self, Result<Open<C2S, S2C, C>>> {
        match self.recv_next.try_recv() {
            Ok(next) => Poll::Ready(next),
            Err(oneshot::error::TryRecvError::Empty) => Poll::Pending(self),
            Err(oneshot::error::TryRecvError::Closed) => {
                Poll::Ready(Err(WebTransportError::BackendClosed))
            }
        }
    }
}

pub struct Open<C2S, S2C, C> {
    local_addr: io::Result<SocketAddr>,
    recv_sig: mpsc::UnboundedReceiver<Signal<C2S>>,
    send_req: broadcast::Sender<Request<S2C>>,
    _phantom_c: PhantomData<C>,
}

impl<C2S, S2C, C> Open<C2S, S2C, C>
where
    C2S: Message,
    S2C: Message + Clone,
    C: Channels,
{
    pub fn local_addr(&self) -> Result<SocketAddr, &io::Error> {
        self.local_addr.as_ref().map(|addr| *addr)
    }

    pub fn close(self) -> Closed<C2S, S2C, C> {
        // implicitly drops mpsc channels, causing the backend to close
        Closed::new()
    }

    pub fn poll(mut self) -> (impl Iterator<Item = Signal<C2S>>, Result<Self>) {
        let mut signals = Vec::new();
        let result = loop {
            match self.recv_sig.try_recv() {
                Ok(sig) => signals.push(sig),
                Err(mpsc::error::TryRecvError::Empty) => break Ok(self),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    break Err(WebTransportError::BackendClosed)
                }
            }
        };
        (signals.into_iter(), result)
    }

    pub fn send<M: Into<S2C>>(self, to: ClientKey, msg: M) -> Result<Self> {
        let msg = msg.into();
        self.send_req
            .send(Request::Send { to, msg })
            .map_err(|_| WebTransportError::BackendClosed)?;
        Ok(self)
    }

    pub fn disconnect(self, client: ClientKey) -> Result<Self> {
        self.send_req
            .send(Request::Disconnect { client })
            .map_err(|_| WebTransportError::BackendClosed)?;
        Ok(self)
    }
}
