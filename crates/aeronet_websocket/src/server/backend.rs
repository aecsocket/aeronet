use {
    super::{ServerConfig, ServerError, ToConnected, ToOpen},
    crate::{server::ToConnecting, session::SessionError},
    aeronet_io::{connection::DisconnectReason, server::CloseReason},
    bevy_ecs::prelude::*,
    core::{
        net::SocketAddr,
        pin::Pin,
        task::{Context, Poll},
    },
    futures::{
        SinkExt,
        channel::{mpsc, oneshot},
        never::Never,
    },
    tokio::{
        io::{AsyncRead, AsyncWrite, ReadBuf},
        net::{TcpListener, TcpStream},
    },
    tokio_rustls::TlsAcceptor,
    tokio_tungstenite::tungstenite::{
        handshake::server::{Request, Response},
        protocol::WebSocketConfig,
    },
    tracing::{Instrument, debug, debug_span},
};

pub async fn start(
    config: ServerConfig,
    tx_next: oneshot::Sender<ToOpen>,
) -> Result<Never, CloseReason> {
    let tls_acceptor = config.tls.map(TlsAcceptor::from);
    let listener = TcpListener::bind(config.bind_address)
        .await
        .map_err(ServerError::BindSocket)?;
    debug!("Listening on {}", config.bind_address);

    let (tx_connecting, rx_connecting) = mpsc::channel::<ToConnecting>(1);

    let local_addr = listener.local_addr().map_err(SessionError::GetLocalAddr)?;
    let next = ToOpen {
        local_addr,
        rx_connecting,
    };
    tx_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting server loop");
    loop {
        let (stream, peer_addr) = listener
            .accept()
            .await
            .map_err(ServerError::AcceptConnection)?;
        tokio::spawn({
            let tx_connecting = tx_connecting.clone();
            let tls_acceptor = tls_acceptor.clone();
            let handshake_handler = config.handshake_handler.clone();
            async move {
                if let Err(err) = accept_session(
                    stream,
                    peer_addr,
                    config.socket,
                    tls_acceptor,
                    tx_connecting,
                    handshake_handler,
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
    peer_addr: SocketAddr,
    socket_config: WebSocketConfig,
    tls_acceptor: Option<TlsAcceptor>,
    mut tx_connecting: mpsc::Sender<ToConnecting>,
    handshake_handler: Option<HandshakeHandler>,
) -> Result<(), Disconnected> {
    let (tx_session_entity, rx_session_entity) = oneshot::channel::<Entity>();
    let (tx_dc_reason, rx_dc_reason) = oneshot::channel::<DisconnectReason>();
    let (tx_next, rx_next) = oneshot::channel::<ToConnected>();
    tx_connecting
        .send(ToConnecting {
            peer_addr,
            tx_session_entity,
            rx_dc_reason,
            rx_next,
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    let session = rx_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)?;

    let Err(dc_reason) = handle_session(
        stream,
        peer_addr,
        socket_config,
        tls_acceptor,
        tx_next,
        handshake_handler,
    )
    .instrument(debug_span!("session", %session))
    .await;
    _ = tx_dc_reason.send(dc_reason);
    Ok(())
}

async fn handle_session(
    stream: TcpStream,
    peer_addr: SocketAddr,
    socket_config: WebSocketConfig,
    tls_acceptor: Option<TlsAcceptor>,
    tx_next: oneshot::Sender<ToConnected>,
    handshake_handler: Option<HandshakeHandler>,
) -> Result<Never, Disconnected> {
    debug!("Performing Session handshake");

    let stream = if let Some(tls_acceptor) = tls_acceptor {
        tls_acceptor
            .accept(stream)
            .await
            .map(MaybeTlsStream::Rustls)
            .map_err(ServerError::TlsHandshake)?
    } else {
        MaybeTlsStream::Plain(stream)
    };
    let stream = tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        |req: &Request, resp: Response| match &handshake_handler {
            Some(h) => h.handle(req, resp),
            None => Ok(resp),
        },
        Some(socket_config),
    )
    .await
    .map_err(ServerError::AcceptClient)?;

    let (frontend, backend) = crate::session::backend::native::split(stream);
    let connected = ToConnected {
        peer_addr,
        frontend,
    };
    debug!("Connected");

    tx_next
        .send(connected)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting session loop");
    backend.start().await
}

#[derive(Debug)]
#[expect(clippy::large_enum_variant, reason = "most users will use `Rustls`")]
enum MaybeTlsStream<S> {
    Plain(S),
    Rustls(tokio_rustls::server::TlsStream<S>),
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for MaybeTlsStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Rustls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for MaybeTlsStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Rustls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Rustls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Self::Rustls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
