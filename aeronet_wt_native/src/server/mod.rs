use std::{io, marker::PhantomData, net::SocketAddr};

use aeronet::{ChannelKey, ChannelKind, Message, OnChannel, TryFromBytes, TryIntoBytes};
use derivative::Derivative;
use futures::future::{try_join_all, Pending};
use rustc_hash::FxHashMap;
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};
use wtransport::{
    endpoint::{endpoint_side, IncomingSession},
    Connection, ServerConfig,
};

use crate::{
    ChannelError, EndpointInfo,
};

// pub fn create_server<C2S, S2C, C>(config: ServerConfig) ->
// (WebTransportServer<C2S, S2C, C>, impl Future<Output = ()> + Send) where
//     C2S: Message + TryFromBytes,
//     S2C: Message + TryIntoBytes + Clone + OnChannel<Channel = C>,
//     C: ChannelKey,
// {
//     let (send_result, recv_result) = oneshot::channel();
// }

slotmap::new_key_type! {
    pub struct ClientKey;
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("no client with key {client:?}")]
pub struct NoClientError {
    pub client: ClientKey,
}

// state machine

type WebTransportError<C2S, S2C, C> = crate::WebTransportError<S2C, C2S, C>;

type ServerEvent<C2S, S2C, C> =
    aeronet::ServerEvent<C2S, ClientKey, WebTransportError<C2S, S2C, C>>;

type Endpoint = wtransport::Endpoint<endpoint_side::Server>;

#[derive(Derivative)]
#[derivative(Debug)]
struct Opening<C2S, S2C, C>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    #[derivative(Debug = "ignore")]
    recv_result: oneshot::Receiver<Result<Open<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>>,
}

// open

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
    recv_incoming: oneshot::Receiver<Result<IncomingClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>>,
}

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
    recv_connected: oneshot::Receiver<Result<ConnectedClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>>,
}

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
    recv_c2s: mpsc::UnboundedReceiver<Result<C2S, WebTransportError<C2S, S2C, C>>>,
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
    send_result: oneshot::Sender<Result<Open<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>>,
)
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    let endpoint = match Endpoint::server(config).map_err(WebTransportError::CreateEndpoint) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_result.send(Err(err));
            return;
        }
    };

    let (send_client, recv_client) = mpsc::unbounded_channel();
    let open = Open::<C2S, S2C, C> {
        local_addr: endpoint.local_addr(),
        clients: FxHashMap::default(),
        recv_client,
    };
    let _ = send_result.send(Ok(open));

    loop {
        let session = endpoint.accept().await;
        let (send_incoming, recv_incoming) = oneshot::channel();
        let client_state = PendingClient {
            recv_incoming,
        };
        if let Err(_) = send_client.send(client_state) {
            // frontend closed
            return;
        };

        tokio::spawn(handle_session::<C2S, S2C, C>(session, send_incoming));
    }
}

async fn handle_session<C2S, S2C, C>(
    session: IncomingSession,
    send_incoming: oneshot::Sender<Result<IncomingClient<C2S, S2C, C>, WebTransportError<C2S, S2C, C>>>,
)
where
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

    let conn = match session.accept().await.map_err(WebTransportError::AcceptSession) {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };

    let (send_c2s, recv_c2s) = ;
    let connected = ConnectedClient {
        info: EndpointInfo::from_connection(&conn),
        recv_c2s,
    }

    let conn = match accept_session::<C2S, S2C, C>(session).await {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_incoming.send(Err(err));
            return;
        }
    };


}

async fn accept_session<C2S, S2C, C>(
    session: IncomingSession,
) -> Result<Connection, WebTransportError<C2S, S2C, C>>
where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
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
