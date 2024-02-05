use std::{net::SocketAddr, time::Duration};

use aeronet::{
    protocol::{Fragmentation, Sequenced, Unsequenced},
    LaneKind,
};
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
pub struct ConnectionFrontend {
    pub remote_addr: SocketAddr,
    pub send_c2s: mpsc::UnboundedSender<Bytes>,
    pub recv_s2c: mpsc::Receiver<Bytes>,
    pub recv_rtt: mpsc::Receiver<Duration>,
    pub recv_err: oneshot::Receiver<BackendError>,
}

#[derive(Debug)]
pub struct ConnectionBackend {
    recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    send_s2c: mpsc::Sender<Bytes>,
    send_rtt: mpsc::Sender<Duration>,
    send_err: oneshot::Sender<BackendError>,
}

pub fn connection_channel(conn: &Connection) -> (ConnectionFrontend, ConnectionBackend) {
    let remote_addr = conn.remote_address();
    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::channel(MSG_BUF_CAP);
    let (send_rtt, recv_rtt) = mpsc::channel(1);
    let (send_err, recv_err) = oneshot::channel();
    (
        ConnectionFrontend {
            remote_addr,
            send_c2s,
            recv_s2c,
            recv_rtt,
            recv_err,
        },
        ConnectionBackend {
            recv_c2s,
            send_s2c,
            send_rtt,
            send_err,
        },
    )
}

pub async fn handle_connection(conn: Connection, chan: ConnectionBackend) {
    debug!("Connected backend");
    match try_handle_connection(conn, chan.recv_c2s, chan.send_s2c, chan.send_rtt).await {
        Ok(()) => debug!("Closed backend"),
        Err(err) => {
            debug!("Closed backend: {:#}", aeronet::util::pretty_error(&err));
            let _ = chan.send_err.send(err);
        }
    }
}

async fn try_handle_connection(
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
                let _ = send_s2c.send(datagram.payload()).await;
            }
            msg = recv_c2s.next() => {
                let Some(msg) = msg else {
                    // frontend closed
                    return Ok(());
                };
                conn.send_datagram(msg).map_err(BackendError::SendDatagram)?;
            }
            _ = tokio::time::sleep(UPDATE_DURATION).fuse() => {
                // do another loop at least every second, so we run the stuff
                // before this `select!` fairly often (updating RTT)
            }
        }
    }
}

/// # Packet layout
///
/// ## Unreliable
///
/// ```text
/// 0      1               5             byte index
/// +------+---------------+-----------
///   lane   fragmentation   payload...  data
/// ```
#[derive(Debug)]
pub enum LaneState {
    UnreliableUnsequenced { frag: Fragmentation<Unsequenced> },
    UnreliableSequenced { frag: Fragmentation<Sequenced> },
    ReliableUnordered {},
    ReliableOrdered {},
}

impl LaneState {
    pub fn new(kind: LaneKind) -> Self {
        match kind {
            LaneKind::UnreliableUnsequenced => Self::UnreliableUnsequenced {
                frag: Fragmentation::unsequenced(),
            },
            LaneKind::UnreliableSequenced => Self::UnreliableSequenced {
                frag: Fragmentation::sequenced(),
            },
            LaneKind::ReliableUnordered => todo!(),
            LaneKind::ReliableOrdered => todo!(),
        }
    }

    pub fn update(&mut self) {
        match self {
            Self::UnreliableUnsequenced { frag } => {
                frag.update();
            }
            Self::UnreliableSequenced { frag } => {
                frag.update();
            }
            Self::ReliableUnordered {} => todo!(),
            Self::ReliableOrdered {} => todo!(),
        }
    }

    pub fn outgoing_packets(
        &mut self,
        bytes: &[u8],
    ) -> Result<impl Iterator<Item = Bytes>, BackendError> {
        Ok(std::iter::empty()) // todo
    }

    pub fn recv(&mut self, packet: &[u8]) -> Result<(), BackendError> {
        todo!()
    }
}
