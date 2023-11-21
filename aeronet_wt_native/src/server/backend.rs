use aeronet::{Message, TryFromBytes, TryIntoBytes, OnChannel, ChannelKey};
use slotmap::SlotMap;
use tokio::sync::{oneshot, mpsc};
use wtransport::{ServerConfig, Endpoint, endpoint::IncomingSession};

use crate::{common, EndpointInfo};

use super::{OpenResult, WebTransportError, OpenServer, PendingClient, IncomingClientResult, IncomingClient, ConnectedClient};

pub(super) async fn listen<C2S, S2C, C>(
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
    let open = OpenServer::<C2S, S2C, C> {
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

    if let Err(err) = common::handle_connection::<S2C, C2S, C>(
        conn,
        channels_state,
        send_info,
        send_c2s,
        recv_s2c,
    )
    .await
    {
        let _ = send_err.send(err);
    }
}
