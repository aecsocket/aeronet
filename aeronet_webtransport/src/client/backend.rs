use futures::{channel::oneshot, never::Never};
use tracing::debug;

use crate::internal;

use super::{ClientConfig, ClientError, Connected};

pub async fn start(
    config: ClientConfig,
    target: String,
    send_connected: oneshot::Sender<Connected>,
) -> Result<Never, ClientError> {
    let endpoint = create_endpoint(config)?;
    debug!("Created endpoint, connecting to {target:?}");
    let conn = endpoint
        .connect(target)
        .await
        .map_err(ClientError::Connect)?
        .await
        .map_err(ClientError::AwaitConnect)?;

    if !internal::check_datagram_support(&conn) {
        return Err(ClientError::DatagramsNotSupported);
    }

    loop {
        todo!()
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
    } else {
        type ClientEndpoint = xwt::current::Endpoint<wtransport::endpoint::endpoint_side::Client>;

        fn create_endpoint(config: ClientConfig) -> Result<ClientEndpoint, ClientError> {
            let raw = wtransport::Endpoint::client(config).map_err(ClientError::CreateEndpoint)?;
            Ok(xwt::current::Endpoint(raw))
        }
    }
}
