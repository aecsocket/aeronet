use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    FutureExt,
};
use tracing::debug;
use web_time::Duration;
use xwt_core::prelude::*;

use crate::{client::ToConnected, internal};

use super::{ClientConfig, ClientError};

pub async fn start(
    config: ClientConfig,
    target: String,
    send_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, ClientError> {
    let endpoint = {
        #[cfg(target_family = "wasm")]
        {
            Ok(xwt_web_sys::Endpoint {
                options: config.to_js(),
            })
        }

        #[cfg(not(target_family = "wasm"))]
        {
            let raw = wtransport::Endpoint::client(config).map_err(ClientError::CreateEndpoint)?;
            Ok(xwt_wtransport::Endpoint(raw))
        }
    }?;

    debug!("Created endpoint, connecting to {target:?}");
    let conn = endpoint
        .connect(&target)
        .await
        .map_err(|err| ClientError::Connect(err.into()))?
        .wait_connect()
        .await
        .map_err(|err| ClientError::AwaitConnect(err.into()))?;

    if !internal::supports_datagrams(&conn) {
        return Err(ClientError::DatagramsNotSupported);
    }

    let (send_rtt, recv_rtt) = mpsc::channel::<Duration>(1);
    let (send_c2s, recv_c2s) = mpsc::unbounded::<Bytes>();
    let (send_s2c, recv_s2c) = mpsc::channel::<Bytes>(internal::MSG_BUF_CAP);
    send_connected
        .send(ToConnected {
            #[cfg(not(target_family = "wasm"))]
            local_addr: endpoint.0.local_addr().map_err(ClientError::GetLocalAddr)?,
            #[cfg(not(target_family = "wasm"))]
            remote_addr: conn.0.remote_address(),
            initial_rtt: internal::rtt_of(&conn),
            recv_rtt,
            send_c2s,
            recv_s2c,
        })
        .map_err(|_| ClientError::FrontendClosed)?;

    debug!("Starting connection loop");
    // `receive_datagram` may not be cancel-safe, so we create multiple futures
    // which loop infinitely independently, and wait for the first one to fail
    let send_loop = internal::send_loop(&conn, recv_c2s);
    let recv_loop = internal::recv_loop(&conn, send_s2c);
    let update_rtt_loop = internal::update_rtt_loop(&conn, send_rtt);
    futures::select! {
        r = send_loop.fuse() => r,
        r = recv_loop.fuse() => r,
        r = update_rtt_loop.fuse() => r,
    }
    .map_err(|err| match err {
        internal::Error::FrontendClosed => ClientError::FrontendClosed,
        internal::Error::ConnectionLost(err) => ClientError::ConnectionLost(err.into()),
        internal::Error::SendDatagram(err) => ClientError::SendDatagram(err.into()),
    })
}
