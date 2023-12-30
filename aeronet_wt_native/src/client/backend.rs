use std::net::SocketAddr;

use aeronet::{LaneKey, LaneProtocol, OnLane, TryAsBytes, TryFromBytes};
use tokio::sync::oneshot;
use wtransport::{endpoint::ConnectOptions, ClientConfig, Connection, Endpoint};

use crate::WebTransportError;

use super::ConnectedResult;

pub(super) async fn start<P>(
    wt_config: ClientConfig,
    conn_opts: ConnectOptions,
    send_connected: oneshot::Sender<ConnectedResult<P>>,
) where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    let (conn, local_addr) = match connect::<P>(wt_config, conn_opts).await {
        Ok(t) => t,
        Err(err) => {
            let _ = send_connected.send(Err(err));
            return;
        }
    };
}

async fn connect<P>(
    wt_config: ClientConfig,
    conn_opts: ConnectOptions,
) -> Result<(Connection, SocketAddr), WebTransportError<P>>
where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    let endpoint = Endpoint::client(wt_config).map_err(WebTransportError::<P>::CreateEndpoint)?;

    let local_addr = endpoint
        .local_addr()
        .map_err(WebTransportError::<P>::GetLocalAddr)?;

    let conn = endpoint
        .connect(conn_opts)
        .await
        .map_err(WebTransportError::<P>::Connect)?;

    Ok((conn, local_addr))
}
