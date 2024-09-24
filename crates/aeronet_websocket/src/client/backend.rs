use crate::session::{SessionError, SessionFrontend};
use aeronet_io::connection::DisconnectReason;
use futures::{channel::oneshot, never::Never};
use tracing::debug;

use super::{ClientConfig, ClientError, ConnectTarget};

pub async fn start(
    packet_buf_cap: usize,
    config: ClientConfig,
    target: ConnectTarget,
    send_connected: oneshot::Sender<SessionFrontend>,
) -> Result<Never, DisconnectReason<ClientError>> {
    let (frontend, backend) = {
        #[cfg(target_family = "wasm")]
        {
            // suppress `unused_variables`
            let _ = config;

            debug!("Spawning backend task to connect to {target:?}");

            let socket = web_sys::WebSocket::new(&target)
                .map_err(crate::JsError::from)
                .map_err(ClientError::CreateSocket)?;
            debug!("Created socket");

            crate::session::backend::wasm::split(socket, packet_buf_cap)
        }

        #[cfg(not(target_family = "wasm"))]
        {
            let target = target.map_err(ClientError::CreateTarget)?;

            debug!("Spawning backend task to connect to {:?}", target.uri());

            let (stream, _) = {
                let socket_config = Some(config.socket);
                let disable_nagle = !config.nagle;

                #[cfg(feature = "__tls")]
                {
                    tokio_tungstenite::connect_async_tls_with_config(
                        target,
                        socket_config,
                        disable_nagle,
                        Some(config.connector),
                    )
                }

                #[cfg(not(feature = "__tls"))]
                {
                    tokio_tungstenite::connect_async_with_config(
                        target,
                        socket_config,
                        disable_nagle,
                    )
                }
            }
            .await
            .map_err(ClientError::Connect)?;
            debug!("Created stream");

            crate::session::backend::native::split(stream, packet_buf_cap)
                .map_err(ClientError::Session)?
        }
    };

    send_connected
        .send(frontend)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ClientError::Session)?;

    debug!("Starting session loop");
    backend
        .start()
        .await
        .map_err(|reason| reason.map_err(ClientError::Session))
}
