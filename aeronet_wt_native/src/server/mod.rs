mod backend;
mod frontend;

pub use frontend::{WebTransportServer, ServerEvent};

use std::{io, net::SocketAddr};

use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};

use crate::{ClientKey, EndpointInfo};

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
    recv_client: mpsc::UnboundedReceiver<IncomingClient<C2S, S2C, C>>,
    // TODO this needs to be improved
    #[derivative(Debug = "ignore")]
    send_closed: mpsc::Sender<()>,
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
