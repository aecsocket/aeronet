mod backend;

use std::{future::Future, io, net::SocketAddr};

use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};
use wtransport::ServerConfig;

use crate::EndpointInfo;

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

/// The server was expected to be in a specific state for this action, but was
/// actually in a different state.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ExpectedStateError {
    /// Expected to be closed.
    #[error("expected state Closed")]
    Closed,
    /// Expected to be open.
    #[error("expected state Open")]
    Open,
}

#[derive(Debug)]
#[cfg_attr(feature = "bevy", derive(bevy::prelude::Resource))]
pub struct WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    state: ServerState<C2S, S2C, C>,
}

#[derive(Debug)]
enum ServerState<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Closed,
    Opening(OpeningServer<C2S, S2C, C>),
    Open(OpenServer<C2S, S2C, C>),
}

impl<C2S, S2C, C> WebTransportServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    pub fn open(config: ServerConfig) -> (Self, impl Future<Output = ()> + Send) {
        let (send_open, recv_open) = oneshot::channel();
        let opening = OpeningServer { recv_open };
        (Self { state: ServerState::Opening(opening) }, backend::listen::<C2S, S2C, C>(config, send_open))
    }

    pub fn poll(&mut self) -> Result<(), WebTransportError<C2S, S2C, C>> {
        
    }
}

// state machine

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<S2C, C2S, C>;

#[derive(Derivative)]
#[derivative(Debug)]
struct OpeningServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_open: oneshot::Receiver<OpenResult<C2S, S2C, C>>,
}

type OpenResult<C2S, S2C, C> = Result<OpenServer<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>;

#[derive(Derivative)]
#[derivative(Debug)]
struct OpenServer<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    local_addr: Result<SocketAddr, io::Error>,
    clients: SlotMap<ClientKey, Client<C2S, S2C, C>>,
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
enum Client<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    Pending(PendingClient<C2S, S2C, C>),
    Incoming(IncomingClient<C2S, S2C, C>),
    Connected(ConnectedClient<C2S, S2C, C>),
}
