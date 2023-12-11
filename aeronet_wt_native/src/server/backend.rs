use aeronet::{ChannelKey, Message, OnChannel, TryFromBytes, TryIntoBytes};
use slotmap::SlotMap;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;
use wtransport::{endpoint::IncomingSession, Endpoint, ServerConfig};

use crate::{shared, EndpointInfo};

use super::{
    AcceptedClient, AcceptedClientResult, ConnectedClient, IncomingClient, OpenResult, OpenServer,
    WebTransportError,
};

pub(super) async fn start<C2S, S2C, C>(
    config: ServerConfig,
    send_open: oneshot::Sender<OpenResult<C2S, S2C, C>>,
) where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    debug!("Starting backend");
    let endpoint = match Endpoint::server(config).map_err(WebTransportError::Endpoint) {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };
    debug!("Created endpoint");

    let (send_client, recv_client) = mpsc::unbounded_channel();
    let (send_closed, mut recv_closed) = mpsc::channel(1);
    let open = OpenServer {
        local_addr: endpoint.local_addr(),
        clients: SlotMap::default(),
        recv_client,
        send_closed,
    };
    if send_open.send(Ok(open)).is_err() {
        debug!("Frontend closed");
        return;
    }

    loop {
        debug!("Listening for incoming sessions");
        let session = tokio::select! {
            session = endpoint.accept() => session,
            _ = recv_closed.recv() => return,
        };
        debug!("Incoming session");

        let (send_accepted, recv_accepted) = oneshot::channel();
        let client_state = IncomingClient { recv_accepted };
        if send_client.send(client_state).is_err() {
            debug!("Frontend closed");
            return;
        };

        tokio::spawn(handle_session::<C2S, S2C, C>(session, send_accepted));
    }
}

async fn handle_session<C2S, S2C, C>(
    session: IncomingSession,
    send_accepted: oneshot::Sender<AcceptedClientResult<C2S, S2C, C>>,
) where
    C2S: Message + TryFromBytes,
    S2C: Message + TryIntoBytes + OnChannel<Channel = C>,
    C: ChannelKey,
{
    let session = match session.await.map_err(WebTransportError::IncomingSession) {
        Ok(session) => session,
        Err(err) => {
            let _ = send_accepted.send(Err(err));
            return;
        }
    };

    debug!("Session accepted on {}{}", session.authority(), session.path());

    let (send_connected, recv_connected) = oneshot::channel();
    let accepted = AcceptedClient {
        authority: session.authority().to_owned(),
        path: session.path().to_owned(),
        origin: session.origin().map(ToOwned::to_owned),
        user_agent: session.user_agent().map(ToOwned::to_owned),
        recv_connected,
    };
    if send_accepted.send(Ok(accepted)).is_err() {
        debug!("Frontend closed");
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

    debug!("Establishing channels");
    let channels_state = match shared::establish_channels::<S2C, C2S, C, true>(&conn).await {
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
    if send_connected.send(Ok(connected)).is_err() {
        debug!("Frontend closed");
        return;
    }

    debug!("Starting connection loop");
    if let Err(err) = shared::handle_connection::<S2C, C2S, C>(
        conn,
        channels_state,
        send_info,
        send_c2s,
        recv_s2c,
    )
    .await
    {
        debug!("Disconnected with error");
        let _ = send_err.send(err);
    } else {
        debug!("Disconnected without error");
    }
}
