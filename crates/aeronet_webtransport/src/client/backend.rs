use aeronet_proto::session::{Session, SessionConfig};
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt,
};
use tracing::debug;
use web_time::Instant;
use xwt_core::prelude::*;

use crate::{
    client::ToConnected,
    internal::{self, ClientEndpoint, ConnectionMeta, MIN_MTU},
};

use super::{ClientConfig, ClientError};

pub async fn start(
    net_config: ClientConfig,
    session_config: SessionConfig,
    target: String,
    send_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, ClientError> {
    let endpoint: Result<ClientEndpoint, ClientError> = {
        #[cfg(target_family = "wasm")]
        {
            Ok(xwt_web_sys::Endpoint {
                options: net_config.to_js(),
            })
        }

        #[cfg(not(target_family = "wasm"))]
        {
            let raw =
                wtransport::Endpoint::client(net_config).map_err(ClientError::CreateEndpoint)?;
            Ok(xwt_wtransport::Endpoint(raw))
        }
    };
    let endpoint = endpoint?;

    debug!("Created endpoint, connecting to {target:?}");
    #[allow(clippy::useless_conversion)] // multi-target support
    let conn = endpoint
        .connect(&target)
        .await
        .map_err(|err| ClientError::Connect(err.into()))?
        .wait_connect()
        .await
        .map_err(|err| ClientError::AwaitConnect(err.into()))?;

    let Some(mtu) = internal::get_mtu(&conn) else {
        return Err(ClientError::DatagramsNotSupported);
    };
    let session = Session::new(Instant::now(), session_config, MIN_MTU, mtu)
        .map_err(ClientError::MtuTooSmall)?;

    let (send_meta, recv_meta) = mpsc::channel::<ConnectionMeta>(1);
    let (send_c2s, recv_c2s) = mpsc::unbounded::<Bytes>();
    let (send_s2c, recv_s2c) = mpsc::channel::<Bytes>(internal::MSG_BUF_CAP);
    send_connected
        .send(ToConnected {
            #[cfg(not(target_family = "wasm"))]
            local_addr: endpoint.0.local_addr().map_err(ClientError::GetLocalAddr)?,
            #[cfg(not(target_family = "wasm"))]
            remote_addr: conn.0.remote_address(),
            #[cfg(not(target_family = "wasm"))]
            initial_rtt: conn.0.rtt(),
            recv_meta,
            send_c2s,
            recv_s2c,
            session,
        })
        .map_err(|_| ClientError::FrontendClosed)?;

    debug!("Starting connection loop");
    /*
    TODO: BROKEN ON WASM:
      We can't actually poll for sending and receiving at the same time in the same future.
      These 3 loops should be split into separately spawned tasks.
      But, with xwt, we can't split this single Connection into its sending and receiving halves,
      making it really annoying to split this into separate tasks.

      So for now, WASM clients will be practically useless.

    BLOCKING ISSUE!!
    */
    let send_loop = internal::send_loop(&conn, recv_c2s);
    let recv_loop = internal::recv_loop(&conn, send_s2c);
    let update_meta_loop = internal::update_meta(&conn, send_meta);
    #[allow(clippy::useless_conversion)] // multi-target support
    futures::select! {
        r = send_loop.fuse() => r,
        r = recv_loop.fuse() => r,
        r = update_meta_loop.fuse() => r,
    }
    .map_err(From::from)
}
