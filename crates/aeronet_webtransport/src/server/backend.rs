use {
    super::{ServerError, SessionResponse, ToConnected, ToConnecting, ToOpen},
    crate::{
        WebTransportRuntime,
        session::{SessionBackend, SessionError, SessionMeta},
    },
    aeronet_io::{connection::DisconnectReason, packet::RecvPacket, server::CloseReason},
    bevy_ecs::prelude::*,
    bytes::Bytes,
    futures::{
        FutureExt, SinkExt, StreamExt,
        channel::{mpsc, oneshot},
        never::Never,
    },
    tracing::{Instrument, debug, debug_span},
    wtransport::{
        Endpoint, ServerConfig,
        endpoint::{IncomingSession, SessionRequest},
    },
    xwt_core::prelude::*,
};

pub async fn start(
    config: ServerConfig,
    tx_next: oneshot::Sender<ToOpen>,
) -> Result<Never, CloseReason> {
    debug!("Spawning backend task to open server");

    let endpoint = Endpoint::server(config).map_err(SessionError::CreateEndpoint)?;
    debug!("Created endpoint");

    let (tx_connecting, rx_connecting) = mpsc::channel(1);
    let (tx_dropped, mut rx_dropped) = mpsc::channel::<()>(0);

    let local_addr = endpoint.local_addr().map_err(SessionError::GetLocalAddr)?;
    let next = ToOpen {
        local_addr,
        rx_connecting,
        _tx_dropped: tx_dropped,
    };
    tx_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting server loop");
    loop {
        let session = futures::select! {
            x = endpoint.accept().fuse() => x,
            _ = rx_dropped.next() => {
                return Err(CloseReason::ByError(SessionError::FrontendClosed.into()));
            }
        };

        WebTransportRuntime::spawn({
            let tx_connecting = tx_connecting.clone();
            async move {
                if let Err(err) = accept_session(session, tx_connecting).await {
                    debug!("Failed to accept session: {err:?}");
                }
            }
        });
    }
}

async fn accept_session(
    session: IncomingSession,
    mut tx_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), CloseReason> {
    let request = session.await.map_err(ServerError::AwaitSessionRequest)?;

    let (tx_session_entity, rx_session_entity) = oneshot::channel::<Entity>();
    let (tx_session_response, rx_session_response) = oneshot::channel::<SessionResponse>();
    let (tx_dc_reason, rx_dc_reason) = oneshot::channel::<DisconnectReason>();
    let (tx_next, rx_next) = oneshot::channel::<ToConnected>();
    tx_connecting
        .send(ToConnecting {
            authority: request.authority().to_owned(),
            path: request.path().to_owned(),
            origin: request.origin().map(ToOwned::to_owned),
            user_agent: request.user_agent().map(ToOwned::to_owned),
            headers: request.headers().clone(),
            tx_session_entity,
            tx_session_response,
            rx_dc_reason,
            rx_next,
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    let session = rx_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)?;

    let Err(dc_reason) = handle_session(request, rx_session_response, tx_next)
        .instrument(debug_span!("session", %session))
        .await;
    _ = tx_dc_reason.send(dc_reason);
    Ok(())
}

async fn handle_session(
    request: SessionRequest,
    rx_session_response: oneshot::Receiver<SessionResponse>,
    tx_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason> {
    debug!(
        "New session request from {}{}",
        request.authority(),
        request.path()
    );

    let session_response = rx_session_response
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    debug!("Frontend responded to this session request with {session_response:?}");

    let conn = match session_response {
        SessionResponse::Accepted => request.accept(),
        SessionResponse::Forbidden => {
            request.forbidden().await;
            return Err(ServerError::Rejected.into());
        }
        SessionResponse::NotFound => {
            request.not_found().await;
            return Err(ServerError::Rejected.into());
        }
    }
    .await
    .map(xwt_wtransport::Connection)
    .map_err(ServerError::AcceptSessionRequest)?;
    debug!("Connected");

    let (tx_meta, rx_meta) = mpsc::channel::<SessionMeta>(1);
    let (tx_packet_b2f, rx_packet_b2f) = mpsc::unbounded::<RecvPacket>();
    let (tx_packet_f2b, rx_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (tx_user_dc, rx_user_dc) = oneshot::channel::<String>();
    let next = ToConnected {
        initial_peer_addr: conn.0.remote_address(),
        initial_rtt: conn.0.rtt(),
        initial_mtu: conn
            .max_datagram_size()
            .ok_or(SessionError::DatagramsNotSupported)?,
        rx_meta,
        rx_packet_b2f,
        tx_packet_f2b,
        tx_user_dc,
    };
    let backend = SessionBackend {
        conn,
        tx_meta,
        tx_packet_b2f,
        rx_packet_f2b,
        rx_user_dc,
    };
    tx_connected
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting session loop");
    Err(backend.start().await)
}
