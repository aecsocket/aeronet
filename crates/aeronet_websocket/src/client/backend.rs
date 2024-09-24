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
        {}
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

// pub(crate) async fn start(
//     runtime: WebSocketRuntime,
// ) -> Result<Never, DisconnectReason<ClientError>> {
//     socket.set_binary_type(BinaryType::Arraybuffer);

//     let target = target.map_err(ClientError::CreateTarget)?;

//     let (handle, _) = {
//         let socket_config = Some(config.socket);
//         let disable_nagle = !config.nagle;

//         #[cfg(feature = "__tls")]
//         {
//             tokio_tungstenite::connect_async_tls_with_config(
//                 target,
//                 socket_config,
//                 disable_nagle,
//                 Some(config.connector),
//             )
//         }

//         #[cfg(not(feature = "__tls"))]
//         {
//             tokio_tungstenite::connect_async_with_config(target, socket_config, disable_nagle)
//         }
//     }
//     .await
//     .map_err(ClientError::Connect)?;
//     debug!("Created stream");

//     let next = ToConnected {
//         #[cfg(not(target_family = "wasm"))]
//         local_addr: tcp_stream_of(&handle)
//             .local_addr()
//             .map_err(SessionError::GetLocalAddr)
//             .map_err(ClientError::Session)?,
//         #[cfg(not(target_family = "wasm"))]
//         remote_addr: tcp_stream_of(&handle)
//             .peer_addr()
//             .map_err(SessionError::GetRemoteAddr)
//             .map_err(ClientError::Session)?,
//         recv_packet_b2f,
//         send_packet_f2b,
//         send_user_dc,
//     };
//     let backend = SessionBackend {
//         runtime,
//         handle,
//         send_packet_b2f,
//         recv_packet_f2b,
//         recv_user_dc,
//     };
//     send_next
//         .send(next)
//         .map_err(|_| SessionError::FrontendClosed)
//         .map_err(ClientError::Session)?;

//     debug!("Starting session loop");
//     backend
//         .start()
//         .await
//         .map_err(|reason| reason.map_err(ClientError::Session))
// }

#[cfg(not(target_family = "wasm"))]
fn tcp_stream_of(stream: &crate::session::WebSocketStream) -> &tokio::net::TcpStream {
    use tokio_tungstenite::MaybeTlsStream;

    match stream.get_ref() {
        MaybeTlsStream::Plain(stream) => stream,
        #[cfg(feature = "native-tls")]
        MaybeTlsStream::NativeTls(stream) => stream.get_ref().get_ref().get_ref(),
        #[cfg(feature = "__rustls-tls")]
        MaybeTlsStream::Rustls(stream) => stream.get_ref().0,
        _ => unreachable!("should only be one of these variants"),
    }
}
