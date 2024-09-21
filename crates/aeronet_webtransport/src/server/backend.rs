use aeronet_io::connection::DisconnectReason;
use bevy_ecs::prelude::*;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    SinkExt,
};
use tracing::{debug, debug_span, Instrument};
use wtransport::{
    endpoint::{IncomingSession, SessionRequest},
    Endpoint, ServerConfig,
};
use xwt_core::prelude::*;

use crate::{
    session::{SessionBackend, SessionError, SessionMeta},
    WebTransportRuntime,
};

use super::{ConnectionResponse, ServerError, ToConnected, ToConnecting, ToOpen};

pub async fn start(
    runtime: WebTransportRuntime,
    packet_buf_cap: usize,
    config: ServerConfig,
    send_next: oneshot::Sender<ToOpen>,
) -> Result<Never, ServerError> {
    debug!("Spawning backend task to open server");

    let endpoint = Endpoint::server(config).map_err(SessionError::CreateEndpoint)?;
    debug!("Created endpoint");

    let (send_connecting, recv_connecting) = mpsc::channel(1);

    let next = ToOpen {
        local_addr: endpoint.local_addr().map_err(SessionError::GetLocalAddr)?,
        recv_connecting,
    };
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting server loop");
    loop {
        let session = endpoint.accept().await;

        runtime.spawn({
            let runtime = runtime.clone();
            let send_connecting = send_connecting.clone();
            async move {
                if let Err(err) =
                    accept_session(runtime, packet_buf_cap, session, send_connecting).await
                {
                    debug!("Failed to accept session: {err:?}");
                };
            }
        });
    }
}

async fn accept_session(
    runtime: WebTransportRuntime,
    packet_buf_cap: usize,
    session: IncomingSession,
    mut send_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), ServerError> {
    let request = session.await.map_err(ServerError::AwaitSessionRequest)?;

    let (send_session_entity, recv_session_entity) = oneshot::channel::<Entity>();
    let (send_conn_response, recv_conn_response) = oneshot::channel::<ConnectionResponse>();
    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ServerError>>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
    send_connecting
        .send(ToConnecting {
            authority: request.authority().to_owned(),
            path: request.path().to_owned(),
            origin: request.origin().map(ToOwned::to_owned),
            user_agent: request.user_agent().map(ToOwned::to_owned),
            headers: request.headers().clone(),
            send_session_entity,
            send_conn_response,
            recv_dc,
            recv_next,
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    let session = recv_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)?;

    let Err(dc_reason) = handle_session(
        runtime,
        packet_buf_cap,
        request,
        recv_conn_response,
        send_next,
    )
    .instrument(debug_span!("session", session = %session))
    .await
    else {
        unreachable!()
    };
    let _ = send_dc.send(dc_reason);
    Ok(())
}

async fn handle_session(
    runtime: WebTransportRuntime,
    packet_buf_cap: usize,
    request: SessionRequest,
    recv_conn_response: oneshot::Receiver<ConnectionResponse>,
    send_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason<ServerError>> {
    debug!(
        "New session request from {}{}",
        request.authority(),
        request.path()
    );

    let conn_response = recv_conn_response
        .await
        .map_err(|_| SessionError::FrontendClosed.into())
        .map_err(ServerError::Session)?;
    debug!("Frontend responded to this request with {conn_response:?}");

    let conn = match conn_response {
        ConnectionResponse::Accepted => request.accept(),
        ConnectionResponse::Forbidden => {
            request.forbidden().await;
            return Err(ServerError::Rejected.into());
        }
        ConnectionResponse::NotFound => {
            request.not_found().await;
            return Err(ServerError::Rejected.into());
        }
    }
    .await
    .map(xwt_wtransport::Connection)
    .map_err(ServerError::AcceptSessionRequest)?;
    debug!("Connected");

    let (send_meta, recv_meta) = mpsc::channel::<SessionMeta>(1);
    let (send_packet_b2f, recv_packet_b2f) = mpsc::channel::<Bytes>(packet_buf_cap);
    let (send_packet_f2b, recv_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (send_user_dc, recv_user_dc) = oneshot::channel::<String>();
    let next = ToConnected {
        initial_remote_addr: conn.0.remote_address(),
        initial_rtt: conn.0.rtt(),
        initial_mtu: conn
            .max_datagram_size()
            .ok_or(SessionError::DatagramsNotSupported.into())
            .map_err(ServerError::Session)?,
        recv_meta,
        recv_packet_b2f,
        send_packet_f2b,
        send_user_dc,
    };
    let backend = SessionBackend {
        runtime,
        conn,
        send_meta,
        send_packet_b2f,
        recv_packet_f2b,
        recv_user_dc,
    };
    send_connected
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;

    debug!("Starting session loop");
    Err(backend.start().await.map_err(ServerError::Session))
}
