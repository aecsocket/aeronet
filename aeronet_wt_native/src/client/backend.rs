use futures::channel::oneshot;
use tracing::debug;
use wtransport::{endpoint::ConnectOptions, ClientConfig, Endpoint};

use crate::{shared, BackendError};

use super::ConnectedClientInner;

pub(super) async fn connect(
    config: ClientConfig,
    options: ConnectOptions,
    send_conn: oneshot::Sender<Result<ConnectedClientInner, BackendError>>,
) {
    debug!("Connecting backend");
    let endpoint = match Endpoint::client(config).map_err(BackendError::CreateEndpoint) {
        Ok(t) => t,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };
    let local_addr = match endpoint.local_addr().map_err(BackendError::GetLocalAddr) {
        Ok(t) => t,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };
    let conn = match endpoint
        .connect(options)
        .await
        .map_err(BackendError::Connect)
    {
        Ok(t) => t,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };

    let (chan_frontend, chan_backend) = match shared::connection_channel(&conn) {
        Ok(t) => t,
        Err(err) => {
            let _ = send_conn.send(Err(err));
            return;
        }
    };
    let _ = send_conn.send(Ok(ConnectedClientInner {
        conn: chan_frontend,
        local_addr,
    }));
    shared::handle_connection(conn, chan_backend).await
}
