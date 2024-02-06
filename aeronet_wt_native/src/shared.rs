use std::time::Duration;

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

use crate::{BackendError, ConnectionInfo};

const MSG_BUF_CAP: usize = 64;
const UPDATE_DURATION: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub struct ConnectionFrontend {
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_rtt: mpsc::Receiver<Duration>,
    recv_err: oneshot::Receiver<BackendError>,
    pub info: ConnectionInfo,
}

#[derive(Debug)]
pub struct ConnectionBackend {
    recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    send_s2c: mpsc::Sender<Bytes>,
    send_rtt: mpsc::Sender<Duration>,
    send_err: oneshot::Sender<BackendError>,
}

pub fn connection_channel(conn: &Connection) -> (ConnectionFrontend, ConnectionBackend) {
    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::channel(MSG_BUF_CAP);
    let (send_rtt, recv_rtt) = mpsc::channel(1);
    let (send_err, recv_err) = oneshot::channel();
    (
        ConnectionFrontend {
            send_c2s,
            recv_s2c,
            recv_rtt,
            recv_err,
            info: ConnectionInfo::new(conn.remote_address(), conn.rtt()),
        },
        ConnectionBackend {
            recv_c2s,
            send_s2c,
            send_rtt,
            send_err,
        },
    )
}

impl ConnectionFrontend {
    pub fn update(&mut self) {
        while let Ok(Some(rtt)) = self.recv_rtt.try_next() {
            self.info.rtt = rtt;
        }
    }

    pub fn send(&self, msg: Bytes) -> Result<(), BackendError> {
        self.send_c2s
            .unbounded_send(msg)
            .map_err(|_| BackendError::Closed)
    }

    pub fn recv(&mut self) -> Option<Bytes> {
        match self.recv_s2c.try_next() {
            Ok(None) | Err(_) => None,
            Ok(Some(msg)) => Some(msg),
        }
    }

    pub fn recv_err(&mut self) -> Result<(), BackendError> {
        match self.recv_err.try_recv() {
            Ok(None) => Ok(()),
            Ok(Some(err)) => Err(err),
            Err(_) => Err(BackendError::Closed),
        }
    }
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
