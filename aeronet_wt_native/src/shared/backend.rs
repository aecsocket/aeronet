use std::time::Duration;

use aeronet::Runtime;
use bytes::Bytes;
use futures::{channel::mpsc, FutureExt, SinkExt, StreamExt};
use tracing::debug;
use wtransport::Connection;

use crate::BackendError;

use super::ConnectionBackend;

const UPDATE_DURATION: Duration = Duration::from_secs(1);

pub async fn handle_connection(rt: &dyn Runtime, conn: Connection, chan: ConnectionBackend) {
    debug!("Connected backend");
    match try_handle_connection(rt, conn, chan.recv_c2s, chan.send_s2c, chan.send_rtt).await {
        Ok(()) => debug!("Closed backend"),
        Err(err) => {
            debug!("Closed backend: {:#}", aeronet::util::pretty_error(&err));
            let _ = chan.send_err.send(err);
        }
    }
}

async fn try_handle_connection(
    rt: &dyn Runtime,
    conn: Connection,
    mut recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    mut send_s2c: mpsc::Sender<Bytes>,
    mut send_rtt: mpsc::Sender<Duration>,
) -> Result<(), BackendError> {
    debug!("Starting connection loop");
    loop {
        // if we failed to send, then buffer's probably full
        // but we don't care, RTT is a lossy bit of info anyway
        let _ = send_rtt.try_send(conn.rtt());

        futures::select! {
            result = conn.receive_datagram().fuse() => {
                let datagram = result.map_err(BackendError::LostConnection)?;
                let _ = send_s2c.send(datagram.payload()).await;
            }
            msg = recv_c2s.next() => {
                let Some(msg) = msg else {
                    // frontend closed
                    return Ok(());
                };
                conn.send_datagram(msg).map_err(BackendError::SendDatagram)?;
            }
            _ = rt.timer(UPDATE_DURATION).fuse() => {
                // do another loop at least every second, so we run the stuff
                // before this `select!` fairly often (updating RTT)
            }
        }
    }
}
