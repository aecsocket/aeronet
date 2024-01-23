use std::{net::SocketAddr, time::Duration};

use aeronet::protocol::Fragmentation;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    FutureExt, SinkExt, StreamExt,
};
use tracing::debug;
use wtransport::Connection;

use crate::BackendError;

const MSG_BUF_CAP: usize = 64;
const UPDATE_DURATION: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub struct BackendConnection {
    pub local_addr: SocketAddr,
    pub remote_addr: SocketAddr,
    pub send_c2s: mpsc::UnboundedSender<Bytes>,
    pub recv_s2c: mpsc::Receiver<Bytes>,
    pub recv_rtt: mpsc::Receiver<Duration>,
    pub recv_err: oneshot::Receiver<BackendError>,
}

pub async fn open_connection(
    conn: Connection,
    local_addr: SocketAddr,
    send_open: oneshot::Sender<Result<BackendConnection, BackendError>>,
) {
    let remote_addr = conn.remote_address();
    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::channel(MSG_BUF_CAP);
    let (send_rtt, recv_rtt) = mpsc::channel(1);
    let (send_err, recv_err) = oneshot::channel();
    let _ = send_open.send(Ok(BackendConnection {
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
            _ = tokio::time::sleep(UPDATE_DURATION).fuse() => {
                // do another loop at least every second, so we run the stuff
                // before this `select!` fairly often
            }
        }
    }
}

#[derive(Debug)]
pub enum LaneState {
    /// ```text
    /// 0      1               5                                                     MAX  byte index
    /// +------+---------------+-------------------------------------------------------+
    /// | lane | fragmentation | payload                                               |  data
    /// ```
    UnreliableUnsequenced { frag: Fragmentation },
    /// ```text
    /// 0      1               5                                                     MAX  byte index
    /// +------+---------------+-------------------------------------------------------+
    /// | lane | fragmentation | payload                                               |  data
    /// ```
    UnreliableSequenced { frag: Fragmentation },
}
