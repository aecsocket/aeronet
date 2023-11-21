mod backend;

use std::{mem, future::Future, io, net::SocketAddr};

use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use replace_with::replace_with_or_abort_and_return;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};
use wtransport::{endpoint::IncomingSession, Endpoint, ServerConfig};

use crate::{common, EndpointInfo};

slotmap::new_key_type! {
    /// Key type used to uniquely identify a client connected to a
    /// [`WebTransportServer`].
    pub struct ClientKey;
}

/// The given client is not connected to the server.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("no client with key {client:?}")]
pub struct NoClientError {
    /// The key of the client.
    pub client: ClientKey,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ExpectedStateError {
    #[error("expected state Closed")]
    Closed,
    #[error("expected state Open")]
    Open,
}

pub struct WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    state: State<C2S, S2C, C>,
}

impl<C2S, S2C, C> WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    pub fn new() -> Self {
        Self { state: State::Closed }
    }

    pub fn open(&mut self, config: ServerConfig) -> Result<impl Future<Output = ()> + Send, ExpectedStateError> {
        let State::Closed = self.state else {
            return Err(ExpectedStateError::Closed);
        };

        let (send_open, recv_open) = oneshot::channel();
        self.state = State::Opening(Opening { recv_open });
        Ok(backend::listen::<C2S, S2C, C>(config, send_open))
    }

    pub fn poll(&mut self) {
        match &mut self.state {
            State::Closed => {},
            State::Opening(state) => match state.recv_open.try_recv() {
                Ok(open) => {
                    mem::replace(&mut self.state, State::Open(open));
                },
                Err(oneshot::error::TryRecvError::Empty) => {},
                Err(oneshot::error::TryRecvError::Closed) => {
                    mem::replace(&mut self.state, State::Closed);
                }
            }
            State::Open(state) => {},
        }
    }
}

#[derive(Debug)]
enum State<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Closed,
    Opening(Opening<C2S, S2C, C>),
    Open(Open<C2S, S2C, C>),
}


// state machine

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<S2C, C2S, C>;

type ServerEvent<C2S, S2C, C> =
    aeronet::ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct Opening<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_open: oneshot::Receiver<OpenResult<C2S, S2C, C>>,
}

type OpenResult<C2S, S2C, C> = Result<Open<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct Open<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    local_addr: Result<SocketAddr, io::Error>,
    clients: SlotMap<ClientKey, ClientState<C2S, S2C, C>>,
    #[derivative(Debug = "ignore")]
    recv_client: mpsc::UnboundedReceiver<PendingClient<C2S, S2C, C>>,
    // TODO this needs to be improved
    #[derivative(Debug = "ignore")]
    send_closed: mpsc::Sender<()>,
}

#[derive(Derivative)]
#[derivative(Debug)]
struct PendingClient<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_incoming: oneshot::Receiver<IncomingClientResult<C2S, S2C, C>>,
}

type IncomingClientResult<C2S, S2C, C> =
    Result<IncomingClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct IncomingClient<C2S, S2C, C>
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

type ConnectedClientResult<C2S, S2C, C> =
    Result<ConnectedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

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

#[derive(Debug)]
enum ClientState<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Pending(PendingClient<C2S, S2C, C>),
    Incoming(IncomingClient<C2S, S2C, C>),
    Connected(ConnectedClient<C2S, S2C, C>),
}
