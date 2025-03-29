use {
    super::{ServerConfig, ServerError, ToConnected, ToOpen},
    crate::{server::ToConnecting, session::SessionError},
    aeronet_io::{connection::Disconnected, server::Closed},
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
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig,
    tracing::{Instrument, debug, debug_span},
};

pub async fn start(
    config: ServerConfig,
    send_next: oneshot::Sender<ToOpen>,
) -> Result<Never, Closed> {
    let tls_acceptor = config.tls.map(TlsAcceptor::from);
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
        let (stream, peer_addr) = listener
            .accept()
            .await
            .map_err(ServerError::AcceptConnection)?;
        tokio::spawn({
            let send_connecting = send_connecting.clone();
            let tls_acceptor = tls_acceptor.clone();
            async move {
                if let Err(err) = accept_session(
                    stream,
                    peer_addr,
                    config.socket,
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
    peer_addr: SocketAddr,
    socket_config: WebSocketConfig,
    tls_acceptor: Option<TlsAcceptor>,
    mut send_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), Disconnected> {
    let (send_session_entity, recv_session_entity) = oneshot::channel::<Entity>();
    let (send_dc, recv_dc) = oneshot::channel::<Disconnected>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
    send_connecting
        .send(ToConnecting {
            peer_addr,
            send_session_entity,
            recv_dc,
            recv_next,
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    let session = recv_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)?;

    let Err(dc_reason) = handle_session(stream, peer_addr, socket_config, tls_acceptor, send_next)
        .instrument(debug_span!("session", %session))
        .await;
    _ = send_dc.send(dc_reason);
    Ok(())
}

async fn handle_session(
    stream: TcpStream,
    peer_addr: SocketAddr,
    socket_config: WebSocketConfig,
    tls_acceptor: Option<TlsAcceptor>,
    send_next: oneshot::Sender<ToConnected>,
) -> Result<Never, Disconnected> {
    debug!("Accepting session");

    let stream = if let Some(tls_acceptor) = tls_acceptor {
        tls_acceptor
            .accept(stream)
            .await
            .map(MaybeTlsStream::Rustls)
            .map_err(ServerError::TlsHandshake)?
    } else {
        MaybeTlsStream::Plain(stream)
    };
    // TODO accept hdr: find some way to pass control of headers over to user
    let stream = tokio_tungstenite::accept_async_with_config(stream, Some(socket_config))
        .await
        .map_err(ServerError::AcceptClient)?;

    let (frontend, backend) = crate::session::backend::native::split(stream);
    let connected = ToConnected {
        peer_addr,
        frontend,
    };
    debug!("Connected");

    send_next
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
