use std::net::SocketAddr;

use futures::channel::oneshot;
use tracing::debug;
use wtransport::{endpoint::ConnectOptions, ClientConfig, Connection, Endpoint};

use crate::{
    shared::{self, SyncConnection},
    BackendError,
};

pub(super) async fn connect(
    config: ClientConfig,
    options: ConnectOptions,
    send_conn: oneshot::Sender<Result<SyncConnection, BackendError>>,
) {
    debug!("Connecting backend");
    let (conn, local_addr) = match create(config, options).await {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };

    shared::start_connection(conn, send_conn).await
}

async fn create(
    config: ClientConfig,
    options: ConnectOptions,
) -> Result<(Connection, SocketAddr), BackendError> {
    let endpoint = Endpoint::client(config).map_err(BackendError::CreateEndpoint)?;
    let conn = endpoint
        .connect(options)
        .await
        .map_err(BackendError::Connect)?;
    if conn.max_datagram_size().is_none() {
        return Err(BackendError::DatagramsNotSupported);
    }
    let local_addr = endpoint.local_addr().map_err(BackendError::GetLocalAddr)?;

    Ok((conn, local_addr))
}
