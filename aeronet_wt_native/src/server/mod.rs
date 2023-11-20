use std::{io, future::Future, net::SocketAddr};

use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};
use wtransport::{endpoint::IncomingSession, Endpoint, ServerConfig};

use crate::{common, EndpointInfo};

slotmap::new_key_type! {
    pub struct ClientKey;
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("no client with key {client:?}")]
pub struct NoClientError {
    pub client: ClientKey,
}

pub fn create_server<C2S, S2C, C>(config: ServerConfig) -> impl Future<Output = ()> + Send
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    let (send_open, recv_open) = oneshot::channel();
    backend::<C2S, S2C, C>(config, send_open)
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

// backend

async fn backend<C2S, S2C, C>(
    config: ServerConfig,
    send_open: oneshot::Sender<OpenResult<C2S, S2C, C>>,
) where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    let endpoint = match Endpoint::server(config).map_err(WebTransportError::CreateEndpoint) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };

    let (send_client, recv_client) = mpsc::unbounded_channel();
    let (send_closed, mut recv_closed) = mpsc::channel(1);
    let open = Open::<C2S, S2C, C> {
        local_addr: endpoint.local_addr(),
        clients: SlotMap::default(),
        recv_client,
        send_closed,
    };
    if let Err(_) = send_open.send(Ok(open)) {
        // frontend closed
        return;
    }

    loop {
        let session = tokio::select! {
            session = endpoint.accept() => session,
            _ = recv_closed.recv() => return,
        };
        let (send_incoming, recv_incoming) = oneshot::channel();
        let client_state = PendingClient { recv_incoming };
        if let Err(_) = send_client.send(client_state) {
            // frontend closed
            return;
        };

        tokio::spawn(handle_session::<C2S, S2C, C>(session, send_incoming));
    }
}

async fn handle_session<C2S, S2C, C>(
    session: IncomingSession,
    send_incoming: oneshot::Sender<IncomingClientResult<C2S, S2C, C>>,
) where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    let session = match session.await.map_err(WebTransportError::IncomingSession) {
        Ok(session) => session,
        Err(err) => {
            let _ = send_incoming.send(Err(err));
            return;
        }
    };

    let (send_connected, recv_connected) = oneshot::channel();
    let incoming = IncomingClient {
        authority: session.authority().to_owned(),
        path: session.path().to_owned(),
        origin: session.origin().map(|s| s.to_owned()),
        user_agent: session.user_agent().map(|s| s.to_owned()),
        recv_connected,
    };
    if let Err(_) = send_incoming.send(Ok(incoming)) {
        // frontend closed
        return;
    }

    let conn = match session
        .accept()
        .await
        .map_err(WebTransportError::AcceptSession)
    {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };

    let channels_state = match common::establish_channels::<S2C, C2S, C, true>(&conn).await {
        Ok(state) => state,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };

    let (send_c2s, recv_c2s) = mpsc::unbounded_channel();
    let (send_s2c, recv_s2c) = mpsc::unbounded_channel();
    let (send_info, recv_info) = mpsc::unbounded_channel();
    let (send_err, recv_err) = oneshot::channel();
    let connected = ConnectedClient {
        info: EndpointInfo::from_connection(&conn),
        recv_info,
        recv_c2s,
        send_s2c,
        recv_err,
    };
    if let Err(_) = send_connected.send(Ok(connected)) {
        // frontend closed
        return;
    }

    if let Err(err) = common::handle_connection::<S2C, C2S, C>(conn, channels_state, send_info, send_c2s, recv_s2c).await {
        let _ = send_err.send(err);
    }
}
