use std::{net::SocketAddr, time::Duration};

use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt, StreamExt,
};
use tracing::debug;
use wtransport::{endpoint::ConnectOptions, ClientConfig, Connection, Endpoint};

use crate::BackendError;

use super::OpenState;

const MSG_BUF_CAP: usize = 64;

pub(super) async fn open(
    config: ClientConfig,
    options: ConnectOptions,
    send_open: oneshot::Sender<Result<OpenState, BackendError>>,
) {
    debug!("Opened backend");
    let (conn, local_addr) = match connect(config, options).await {
        Ok(conn) => conn,
        Err(err) => {
            let _ = send_open.send(Err(err));
            return;
        }
    };

    let remote_addr = conn.remote_address();
    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::channel(MSG_BUF_CAP);
    let (send_rtt, recv_rtt) = mpsc::channel(1);
    let (send_err, recv_err) = oneshot::channel();
    let _ = send_open.send(Ok(OpenState {
        local_addr,
        remote_addr,
        send_c2s,
        recv_s2c,
        recv_rtt,
        recv_err,
    }));

    match handle_connection(conn, recv_c2s, send_s2c, send_rtt).await {
        Ok(()) => debug!("Closed backend"),
        Err(err) => {
            debug!("Closed backend: {:#}", aeronet::util::as_pretty(&err));
            let _ = send_err.send(err);
        }
    }
}

async fn connect(
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

async fn handle_connection(
    conn: Connection,
    mut recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    mut send_s2c: mpsc::Sender<Bytes>,
    mut send_rtt: mpsc::Sender<Duration>,
) -> Result<(), BackendError> {
    debug!("Starting connection loop");
    loop {
        let _ = send_rtt.send(conn.rtt());

        futures::select! {
            result = conn.receive_datagram().fuse() => {
                let datagram = result.map_err(BackendError::LostConnection)?;
                let _ = send_s2c.send(datagram.payload());
            }
            msg = recv_c2s.next() => {
                let Some(msg) = msg else { return Ok(()) };
                conn.send_datagram(msg).map_err(BackendError::SendDatagram)?;
            }
        }
    }
}
