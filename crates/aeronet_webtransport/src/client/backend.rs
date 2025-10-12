use {
    super::{ClientConfig, ClientError, ConnectTarget, ToConnected},
    crate::session::{SessionBackend, SessionError, SessionMeta},
    aeronet_io::{connection::DisconnectReason, packet::RecvPacket},
    bytes::Bytes,
    futures::{
        channel::{mpsc, oneshot},
        never::Never,
    },
    tracing::debug,
    xwt_core::prelude::*,
};

pub async fn start(
    config: ClientConfig,
    target: ConnectTarget,
    tx_next: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason> {
    let endpoint = {
        #[cfg(target_family = "wasm")]
        {
            debug!("Spawning backend task to connect to {target:?}");
            xwt_web::Endpoint {
                options: config.to_js(),
            }
        }

        #[cfg(not(target_family = "wasm"))]
        {
            debug!("Spawning backend task to connect to {:?}", target.url());
            wtransport::Endpoint::client(config)
                .map(xwt_wtransport::Endpoint)
                .map_err(SessionError::CreateEndpoint)?
        }
    };
    debug!("Created endpoint");

    let conn = {
        #[cfg(target_family = "wasm")]
        {
            endpoint
                .connect(&target)
                .await
                .map_err(|err| ClientError::Connect(err.into()))?
                .wait_connect()
                .await
                .map_err(|err| ClientError::AwaitConnect(err.into()))?
        }

        #[cfg(not(target_family = "wasm"))]
        {
            endpoint
                .0
                .connect(target)
                .await
                .map(xwt_wtransport::Connection)
                .map_err(ClientError::Connect)?
        }
    };
    debug!("Connected");

    let (tx_meta, rx_meta) = mpsc::channel::<SessionMeta>(1);
    let (tx_packet_b2f, rx_packet_b2f) = mpsc::unbounded::<RecvPacket>();
    let (tx_packet_f2b, rx_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (tx_user_dc, rx_user_dc) = oneshot::channel::<String>();
    let next = ToConnected {
        #[cfg(not(target_family = "wasm"))]
        local_addr: endpoint.local_addr().map_err(SessionError::GetLocalAddr)?,
        #[cfg(not(target_family = "wasm"))]
        initial_peer_addr: conn.0.remote_address(),
        #[cfg(not(target_family = "wasm"))]
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
    tx_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting session loop");
    Err(backend.start().await)
}
