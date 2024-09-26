use {
    super::{ServerConfig, ServerError, ToConnected, ToOpen},
    crate::{server::ToConnecting, session::SessionError},
    aeronet_io::connection::DisconnectReason,
    bevy_ecs::prelude::*,
    futures::{
        SinkExt,
        channel::{mpsc, oneshot},
        never::Never,
    },
    std::net::SocketAddr,
    tokio::net::{TcpListener, TcpStream},
    tokio_rustls::TlsAcceptor,
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig,
    tracing::{Instrument, debug, debug_span},
};

pub async fn start(
    config: ServerConfig,
    packet_buf_cap: usize,
    send_next: oneshot::Sender<ToOpen>,
) -> Result<Never, ServerError> {
    let tls_acceptor = TlsAcceptor::from(config.crypto);
    let listener = TcpListener::bind(config.bind_address)
        .await
        .map_err(ServerError::BindSocket)?;
    debug!("Listening on {}", config.bind_address);

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
            let tls_acceptor = tls_acceptor.clone();
            async move {
                if let Err(err) = accept_session(
                    stream,
                    remote_addr,
                    config.socket,
                    packet_buf_cap,
                    tls_acceptor,
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
    tls_acceptor: TlsAcceptor,
    mut send_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), DisconnectReason<ServerError>> {
    let (send_session_entity, recv_session_entity) = oneshot::channel::<Entity>();
    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ServerError>>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
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

    let Err(dc_reason) = handle_session(
        stream,
        remote_addr,
        socket_config,
        packet_buf_cap,
        tls_acceptor,
        send_next,
    )
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
    remote_addr: SocketAddr,
    socket_config: WebSocketConfig,
    packet_buf_cap: usize,
    tls_acceptor: TlsAcceptor,
    send_next: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason<ServerError>> {
    let stream = tls_acceptor
        .accept(stream)
        .await
        .map_err(ServerError::TlsHandshake)?;
    let stream = tokio_tungstenite::accept_async_with_config(stream, Some(socket_config))
        .await
        .map_err(ServerError::AcceptClient)?;

    // TODO accept hdr: find some way to pass control of headers over to user
    let (frontend, backend) = crate::session::backend::native::split(stream, packet_buf_cap);
    let connected = ToConnected {
        remote_addr,
        frontend,
    };
    debug!("Connected");

    send_next
        .send(connected)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ServerError::Session)?;

    debug!("Starting session loop");
    backend
        .start()
        .await
        .map_err(|reason| reason.map_err(ServerError::Session))
}
