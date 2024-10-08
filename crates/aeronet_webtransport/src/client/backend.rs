use aeronet::client::DisconnectReason;
use aeronet_proto::session::{Session, SessionConfig};
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
};
use tracing::debug;
use web_time::Instant;
use xwt_core::prelude::*;

use crate::{
    client::ToConnected,
    internal::{self, ConnectionMeta, MIN_MTU},
    runtime::WebTransportRuntime,
};

use super::{ClientConfig, ClientError};

pub async fn start(
    runtime: WebTransportRuntime,
    net_config: ClientConfig,
    session_config: SessionConfig,
    target: String,
    send_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason<ClientError>> {
    let endpoint = internal::create_client_endpoint(net_config)?;

    debug!("Created endpoint, connecting to {target:?}");
    #[allow(clippy::useless_conversion)] // multi-target support
    let conn = endpoint
        .connect(&target)
        .await
        .map_err(|err| ClientError::Connect(err.into()))?
        .wait_connect()
        .await
        .map_err(|err| ClientError::AwaitConnect(err.into()))?;

    let Some(mtu) = conn.max_datagram_size() else {
        return Err(ClientError::DatagramsNotSupported.into());
    };
    let session = Session::client(Instant::now(), session_config, MIN_MTU, mtu)
        .map_err(ClientError::MtuTooSmall)?;

    #[cfg(target_family = "wasm")]
    {
        // I don't know how the high water mark works exactly,
        // but you need it so that WASM transport works.
        // Don't believe me? Change this to 1.0 and see what happens
        let datagrams = conn.transport.datagrams();
        datagrams.set_incoming_high_water_mark(1_000_000.0);
        datagrams.set_outgoing_high_water_mark(1_000_000.0);
    }

    let (send_meta, recv_meta) = mpsc::channel::<ConnectionMeta>(1);
    let (send_c2s, recv_c2s) = mpsc::unbounded::<Bytes>();
    let (send_s2c, recv_s2c) = mpsc::channel::<Bytes>(internal::MSG_BUF_CAP);
    let (send_local_dc, recv_local_dc) = oneshot::channel::<String>();
    send_connected
        .send(ToConnected {
            #[cfg(not(target_family = "wasm"))]
            local_addr: endpoint.0.local_addr().map_err(ClientError::GetLocalAddr)?,
            #[cfg(not(target_family = "wasm"))]
            initial_remote_addr: conn.0.remote_address(),
            #[cfg(not(target_family = "wasm"))]
            initial_rtt: conn.0.rtt(),
            recv_meta,
            send_c2s,
            recv_s2c,
            send_local_dc,
            session,
        })
        .map_err(|_| ClientError::FrontendClosed)?;

    debug!("Starting connection loop");
    internal::handle_connection(runtime, conn, recv_c2s, send_s2c, send_meta, recv_local_dc)
        .await
        .map_err(|reason| reason.map_err(From::from))
}
