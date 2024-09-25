use std::net::SocketAddr;

use aeronet_io::connection::DisconnectReason;
use bevy_ecs::prelude::*;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    SinkExt,
};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{tungstenite::protocol::WebSocketConfig, MaybeTlsStream};
use tracing::{debug, debug_span, Instrument};

use crate::{
    server::ToConnecting,
    session::{SessionError, SessionFrontend},
};

use super::{ServerConfig, ServerError, ToOpen};

pub async fn start(
    config: ServerConfig,
    packet_buf_cap: usize,
    send_next: oneshot::Sender<ToOpen>,
) -> Result<Never, ServerError> {
    let listener = TcpListener::bind(config.addr)
        .await
        .map_err(ServerError::BindSocket)?;
    debug!("Listening on {}", config.addr);

    let (send_connecting, recv_connecting) = mpsc::channel::<ToConnecting>(1);

    let local_addr = listener.local_addr().map_err(SessionError::GetLocalAddr)?;
    let next = ToOpen {
        local_addr,
        recv_connecting,
    };
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting server loop");
    loop {
        let (stream, remote_addr) = listener
            .accept()
            .await
            .map_err(ServerError::AcceptConnection)?;
        tokio::spawn({
            let send_connecting = send_connecting.clone();
            async move {
                if let Err(err) = accept_session(
                    stream,
                    remote_addr,
                    config.socket,
                    packet_buf_cap,
                    send_connecting,
                )
                .await
                {
                    debug!("Failed to accept session: {err:?}");
                }
            }
        });
    }
}

async fn accept_session(
    stream: TcpStream,
    remote_addr: SocketAddr,
    socket_config: WebSocketConfig,
    packet_buf_cap: usize,
    mut send_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), DisconnectReason<ServerError>> {
    let (send_session_entity, recv_session_entity) = oneshot::channel::<Entity>();
    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ServerError>>();
    let (send_next, recv_next) = oneshot::channel::<SessionFrontend>();
    send_connecting
        .send(ToConnecting {
            remote_addr,
            send_session_entity,
            recv_dc,
            recv_next,
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;
    let session = recv_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;

    let Err(dc_reason) = handle_session(stream, socket_config, packet_buf_cap, send_next)
        .instrument(debug_span!("session", %session))
        .await
    else {
        unreachable!();
    };
    let _ = send_dc.send(dc_reason);
    Ok(())
}

async fn handle_session(
    stream: TcpStream,
    socket_config: WebSocketConfig,
    packet_buf_cap: usize,
    send_next: oneshot::Sender<SessionFrontend>,
) -> Result<Never, DisconnectReason<ServerError>> {
    // TODO TLS
    // TODO accept hdr: find some way to pass control of headers over to user
    let stream = MaybeTlsStream::Plain(stream);
    let stream = tokio_tungstenite::accept_async_with_config(stream, Some(socket_config))
        .await
        .map_err(ServerError::AcceptClient)?;
    let (frontend, backend) = crate::session::backend::native::split(stream, packet_buf_cap)
        .map_err(ServerError::Session)?;
    debug!("Connected");

    send_next
        .send(frontend)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;

    debug!("Starting session loop");
    backend
        .start()
        .await
        .map_err(|reason| reason.map_err(ServerError::Session))
}
