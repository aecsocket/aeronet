use futures::{channel::oneshot, never::Never};
use tracing::debug;
use xwt_core::prelude::*;

use crate::internal;

use super::{ClientConfig, ClientError, Connected, Endpoint};

pub async fn start(
    config: ClientConfig,
    target: String,
    send_connected: oneshot::Sender<Connected>,
) -> Result<Never, ClientError> {
    #[cfg(target_family = "wasm")]
    let endpoint = todo!();

    #[cfg(not(target_family = "wasm"))]
    let endpoint = {
        let raw = wtransport::Endpoint::client(config).map_err(ClientError::CreateEndpoint)?;
        Ok(xwt::current::Endpoint(raw))
    }?;

    debug!("Created endpoint, connecting to {target:?}");
    let conn = endpoint
        .connect(&target)
        .await
        .map_err(ClientError::Connect)?
        .wait_connect()
        .await
        .map_err(ClientError::AwaitConnect)?;

    if !internal::check_datagram_support(&conn) {
        return Err(ClientError::DatagramsNotSupported);
    }

    send_connected
        .send(Connected {
            #[cfg(not(target_family = "wasm"))]
            local_addr: endpoint.0.local_addr().map_err(ClientError::GetLocalAddr)?,
            #[cfg(not(target_family = "wasm"))]
            remote_addr: conn.0.remote_address(),
            stats: (),
            recv_s2c: (),
            send_c2s: (),
            recv_stats: (),
        })
        .map_err(|_| ClientError::FrontendClosed)?;

    loop {}
}
