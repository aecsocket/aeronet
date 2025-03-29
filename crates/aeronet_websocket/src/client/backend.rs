use {
    super::{ClientConfig, ClientError, ConnectTarget},
    crate::{client::ToConnected, session::SessionError},
    aeronet_io::connection::Disconnected,
    futures::{channel::oneshot, never::Never},
    tracing::debug,
};

pub async fn start(
    config: ClientConfig,
    target: ConnectTarget,
    send_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, Disconnected> {
    let (connected, backend) = {
        #[cfg(target_family = "wasm")]
        {
            // suppress `unused_variables`
            _ = config;

            debug!("Spawning backend task to connect to {target:?}");

            let socket = web_sys::WebSocket::new(&target)
                .map_err(crate::JsError::from)
                .map_err(ClientError::CreateSocket)?;
            debug!("Created socket");

            let (frontend, backend) = crate::session::backend::wasm::split(socket);
            (ToConnected { frontend }, backend)
        }

        #[cfg(not(target_family = "wasm"))]
        {
            use tokio_tungstenite::MaybeTlsStream;

            let target = target.map_err(ClientError::CreateTarget)?;

            debug!("Spawning backend task to connect to {:?}", target.uri());

            let (stream, _) = {
                let socket_config = Some(config.socket);
                let disable_nagle = !config.nagle;

                tokio_tungstenite::connect_async_tls_with_config(
                    target,
                    socket_config,
                    disable_nagle,
                    Some(config.connector),
                )
            }
            .await
            .map_err(ClientError::Connect)?;

            let socket = match stream.get_ref() {
                MaybeTlsStream::Plain(socket) => socket,
                MaybeTlsStream::Rustls(stream) => stream.get_ref().0,
                _ => panic!("should not be using this kind of stream - {stream:?}"),
            };
            let local_addr = socket.local_addr().map_err(SessionError::GetLocalAddr)?;
            let peer_addr = socket.peer_addr().map_err(SessionError::GetPeerAddr)?;
            debug!("Created stream");

            let (frontend, backend) = crate::session::backend::native::split(stream);
            (
                ToConnected {
                    local_addr,
                    peer_addr,
                    frontend,
                },
                backend,
            )
        }
    };

    send_connected
        .send(connected)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting session loop");
    backend.start().await
}
