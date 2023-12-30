use std::net::SocketAddr;

use aeronet::{LaneProtocol, OnLane, TryAsBytes, TryFromBytes};
use futures::channel::{mpsc, oneshot};
use tracing::debug;
use wtransport::{endpoint::ConnectOptions, ClientConfig, Connection, Endpoint};

use crate::{
    shared::{self, MSG_CHAN_BUF},
    ConnectedClient, ConnectionInfo, WebTransportError,
};

use super::{ConnectedResult, TransportConfig};

pub(super) async fn start<P>(
    wt_config: ClientConfig,
    transport_config: TransportConfig<P>,
    conn_opts: ConnectOptions,
    send_connected: oneshot::Sender<ConnectedResult<P>>,
) where
    P: LaneProtocol,
    P::Send: TryAsBytes + OnLane<Lane = P::Lane>,
    P::Recv: TryFromBytes,
{
    debug!("Opened backend");
    _start(wt_config, transport_config, conn_opts, send_connected).await;
    debug!("Closed backend");
}

async fn _start<P>(
    wt_config: ClientConfig,
    transport_config: TransportConfig<P>,
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

    shared::open_lanes::<P>(&conn).await;

    let (send_s, recv_s) = mpsc::unbounded();
    let (send_r, recv_r) = mpsc::channel(MSG_CHAN_BUF);
    let (send_info, recv_info) = mpsc::channel(1);
    let (send_err, recv_err) = oneshot::channel();
    let _ = send_connected.send(Ok(ConnectedClient {
        conn_info: ConnectionInfo::from_connection(&conn),
        local_addr,
        send_s,
        recv_r,
        recv_info,
        recv_err,
    }));

    let TransportConfig {
        timeout,
        send_conditioner,
        recv_conditioner,
    } = transport_config;

    shared::handle_connection(
        conn,
        send_conditioner,
        recv_conditioner,
        recv_s,
        send_r,
        send_info,
        send_err,
    )
    .await;
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
