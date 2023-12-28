use std::{net::SocketAddr, sync::Arc};

use aeronet::{ChannelProtocol, OnChannel, TryAsBytes, TryFromBytes};
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot, Notify};
use tracing::{debug, debug_span, Instrument};
use wtransport::{
    endpoint::{endpoint_side, IncomingSession},
    Endpoint, ServerConfig,
};

use crate::{server::UntrackedClient, shared::{self, MSG_BUF_CAP, INFO_BUF_CAP}, ClientKey, EndpointInfo};

use super::{
    AcceptedClient, AcceptedClientResult, ConnectedClient, IncomingClient, OpenServer,
    OpenServerResult, WebTransportError,
};

pub(super) async fn open<P>(config: ServerConfig, send_open: oneshot::Sender<OpenServerResult<P>>)
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    debug!("Opened backend");
    start::<P>(config, send_open).await;
    debug!("Closed backend");
}

async fn start<P>(config: ServerConfig, send_open: oneshot::Sender<OpenServerResult<P>>)
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    let (endpoint, local_addr) = match create_endpoint::<P>(config) {
        Ok(t) => t,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };
    debug!("Created endpoint");

    let (send_client, recv_client) = mpsc::unbounded_channel();
    let closed = Arc::new(Notify::new());
    let open = OpenServer {
        local_addr,
        clients: SlotMap::default(),
        recv_client,
        closed: closed.clone(),
    };
    let _ = send_open.send(Ok(open));

    debug!("Listening for incoming sessions");
    loop {
        let session = tokio::select! {
            () = closed.notified() => return,
            session = endpoint.accept() => session,
        };
        debug!("Incoming session");

        // the backend doesn't know the ClientKey,
        // so the frontend has to send it over
        // so that we can instrument this session
        let (send_key, recv_key) = oneshot::channel();
        let (send_incoming, recv_incoming) = oneshot::channel();
        let _ = send_client.send(UntrackedClient {
            send_key: Some(send_key),
            recv_incoming,
        });

        // make sure to start a new task ASAP
        // so that we can keep accepting sessions
        tokio::spawn(start_session::<P>(session, recv_key, send_incoming));
    }
}

fn create_endpoint<P>(
    config: ServerConfig,
) -> Result<(Endpoint<endpoint_side::Server>, SocketAddr), WebTransportError<P>>
where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    let endpoint = Endpoint::server(config).map_err(WebTransportError::Endpoint)?;
    let local_addr = endpoint
        .local_addr()
        .map_err(WebTransportError::GetLocalAddr)?;
    Ok((endpoint, local_addr))
}

async fn start_session<P>(
    session: IncomingSession,
    recv_key: oneshot::Receiver<ClientKey>,
    send_incoming: oneshot::Sender<IncomingClient<P>>,
) where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    let key = match recv_key.await {
        Ok(key) => key,
        Err(_) => return,
    };

    let (send_accepted, recv_accepted) = oneshot::channel();
    let _ = send_incoming.send(IncomingClient { recv_accepted });

    async move {
        handle_session::<P>(session, send_accepted).await;
        debug!("Finished session");
    }
    .instrument(debug_span!("Client", key = tracing::field::display(key)))
    .await;
}

async fn handle_session<P>(
    session: IncomingSession,
    send_accepted: oneshot::Sender<AcceptedClientResult<P>>,
) where
    P: ChannelProtocol,
    P::C2S: TryFromBytes,
    P::S2C: TryAsBytes + OnChannel<Channel = P::Channel>,
{
    let session = match session.await.map_err(WebTransportError::IncomingSession) {
        Ok(session) => session,
        Err(err) => {
            let _ = send_accepted.send(Err(err));
            return;
        }
    };

    let authority = session.authority();
    let path = session.path();
    debug!("Session accepted on {authority}{path}");

    let (send_connected, recv_connected) = oneshot::channel();
    let accepted = AcceptedClient {
        authority: authority.to_owned(),
        path: path.to_owned(),
        origin: session.origin().map(ToOwned::to_owned),
        user_agent: session.user_agent().map(ToOwned::to_owned),
        recv_connected,
    };
    if send_accepted.send(Ok(accepted)).is_err() {
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

    let setup = match shared::setup_connection::<P, P::S2C, P::C2S, true>(&conn).await {
        Ok(t) => t,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };

    let (send_c2s, recv_c2s) = mpsc::channel(MSG_BUF_CAP);
    let (send_s2c, recv_s2c) = mpsc::unbounded_channel();
    let (send_info, recv_info) = mpsc::channel(INFO_BUF_CAP);
    let (send_err, recv_err) = oneshot::channel();
    let connected = ConnectedClient {
        info: EndpointInfo::from_connection(&conn),
        recv_info,
        recv_c2s,
        send_s2c,
        recv_err,
    };
    if send_connected.send(Ok(connected)).is_err() {
        return;
    }

    shared::handle_connection::<P, P::S2C, P::C2S>(
        conn,
        setup,
        send_info,
        send_c2s,
        send_err,
        recv_s2c,
    )
    .await;
}
