use {
    super::{ClientConfig, ClientError, ToConnected},
    crate::{
        WebTransportRuntime,
        session::{SessionBackend, SessionError, SessionMeta},
    },
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
    runtime: WebTransportRuntime,
    packet_buf_cap: usize,
    config: ClientConfig,
    target: String,
    send_next: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason<ClientError>> {
    debug!("Spawning backend task to connect to {target:?}");

    let endpoint = {
        #[cfg(target_family = "wasm")]
        {
            todo!()
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

    #[cfg_attr(
        not(target_family = "wasm"),
        expect(clippy::useless_conversion, reason = "conversion required for WASM")
    )]
    let conn = endpoint
        .connect(&target)
        .await
        .map_err(|err| ClientError::Connect(err.into()))?
        .wait_connect()
        .await
        .map_err(|err| ClientError::AwaitConnect(err.into()))?;
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
        #[cfg_attr(
            not(target_family = "wasm"),
            expect(clippy::useless_conversion, reason = "conversion required for WASM")
        )]
        initial_mtu: conn
            .max_datagram_size()
            .ok_or(SessionError::DatagramsNotSupported.into())
            .map_err(ClientError::Session)?,
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
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ClientError::Session)?;

    debug!("Starting session loop");
    Err(backend.start().await.map_err(ClientError::Session))
}
