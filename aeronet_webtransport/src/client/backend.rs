use futures::channel::oneshot;
use xwt_core::{Connecting, EndpointConnect};

use crate::{shared, BackendError, Endpoint, WebTransportClientConfig};

use super::ConnectedInner;

pub async fn connect(
    config: WebTransportClientConfig,
    send_conn: oneshot::Sender<Result<ConnectedInner, BackendError>>,
) {
    let endpoint = match create_endpoint(config.native).await {
        Ok(endpoint) => endpoint,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };

    let connecting = match endpoint
        .connect(&config.url)
        .await
        .map_err(BackendError::Connect)
    {
        Ok(connecting) => connecting,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };

    let conn = match connecting
        .wait_connect()
        .await
        .map_err(BackendError::Connecting)
    {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };

    let (chan_frontend, chan_backend) = match shared::connection_channel::<false>(
        &conn,
        config.version,
        config.max_packet_len,
        &config.lanes,
    )
    .await
    {
        Ok(chans) => chans,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };

    #[cfg(not(target_family = "wasm"))]
    let local_addr = match endpoint.0.local_addr().map_err(BackendError::GetLocalAddr) {
        Ok(local_addr) => local_addr,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };
    let _ = send_conn.send(Ok(ConnectedInner {
        conn: chan_frontend,
        #[cfg(not(target_family = "wasm"))]
        local_addr,
    }));
    chan_backend.handle(conn).await;
}

#[cfg(target_family = "wasm")]
async fn client_endpoint(config: web_sys::WebTransportOptions) -> Result<Endpoint, BackendError> {
    Ok(xwt::current::Endpoint { options: config })
}

#[cfg(not(target_family = "wasm"))]
async fn create_endpoint(config: wtransport::ClientConfig) -> Result<Endpoint, BackendError> {
    let endpoint = wtransport::Endpoint::client(config).map_err(BackendError::CreateEndpoint)?;
    Ok(xwt::current::Endpoint(endpoint))
}