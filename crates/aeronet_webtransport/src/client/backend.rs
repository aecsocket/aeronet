use {
    super::{ClientConfig, ClientError, ConnectTarget, ToConnected},
    crate::session::{SessionBackend, SessionError, SessionMeta},
    aeronet_io::connection::DisconnectReason,
    bytes::Bytes,
    futures::{
        channel::{mpsc, oneshot},
        never::Never,
    },
    tracing::debug,
    xwt_core::prelude::*,
};

pub async fn start(
    packet_buf_cap: usize,
    config: ClientConfig,
    target: ConnectTarget,
    send_next: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason<ClientError>> {
    // TODO: On native, debug log the target after this is merged:
    // https://github.com/BiagioFesta/wtransport/pull/226
    #[cfg(target_family = "wasm")]
    debug!("Spawning backend task to connect to {target:?}");

    let endpoint = {
        #[cfg(target_family = "wasm")]
        {
            xwt_web_sys::Endpoint {
                options: config.to_js(),
            }
        }

        #[cfg(not(target_family = "wasm"))]
        {
            wtransport::Endpoint::client(config)
                .map(xwt_wtransport::Endpoint)
                .map_err(SessionError::CreateEndpoint)
                .map_err(ClientError::Session)?
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

    let (send_meta, recv_meta) = mpsc::channel::<SessionMeta>(1);
    let (send_packet_b2f, recv_packet_b2f) = mpsc::channel::<Bytes>(packet_buf_cap);
    let (send_packet_f2b, recv_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (send_user_dc, recv_user_dc) = oneshot::channel::<String>();
    let next = ToConnected {
        #[cfg(not(target_family = "wasm"))]
        local_addr: endpoint
            .local_addr()
            .map_err(SessionError::GetLocalAddr)
            .map_err(ClientError::Session)?,
        #[cfg(not(target_family = "wasm"))]
        initial_remote_addr: conn.0.remote_address(),
        #[cfg(not(target_family = "wasm"))]
        initial_rtt: conn.0.rtt(),
        initial_mtu: conn
            .max_datagram_size()
            .ok_or(SessionError::DatagramsNotSupported)
            .map_err(ClientError::Session)?,
        recv_meta,
        recv_packet_b2f,
        send_packet_f2b,
        send_user_dc,
    };
    let backend = SessionBackend {
        conn,
        send_meta,
        send_packet_b2f,
        recv_packet_f2b,
        recv_user_dc,
    };
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ClientError::Session)?;

    debug!("Starting session loop");
    Err(backend.start().await.map_err(ClientError::Session))
}
