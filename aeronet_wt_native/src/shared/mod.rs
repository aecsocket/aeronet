mod backend;
mod frontend;
mod negotiate;

pub(crate) use backend::*;

use std::time::Duration;

use aeronet::{
    protocol::{Fragmentation, Sequenced, Unsequenced},
    LaneKind, ProtocolVersion,
};
use bytes::Bytes;
use futures::channel::{mpsc, oneshot};
use wtransport::{Connection, RecvStream, SendStream};

use crate::{BackendError, ConnectionInfo};

const MSG_BUF_CAP: usize = 64;

#[derive(Debug)]
pub struct ConnectionFrontend {
    send_c2s: mpsc::UnboundedSender<Bytes>,
    recv_s2c: mpsc::Receiver<Bytes>,
    recv_rtt: mpsc::Receiver<Duration>,
    recv_err: oneshot::Receiver<BackendError>,
    /// Connection statistics.
    ///
    /// `remote_addr`, `rtt`, and `total_bytes_(sent|recv)` are managed by this
    /// struct itself. All other fields are managed by the user of the
    /// connection.
    pub info: ConnectionInfo,
}

#[derive(Debug)]
pub struct ConnectionBackend {
    recv_c2s: mpsc::UnboundedReceiver<Bytes>,
    send_s2c: mpsc::Sender<Bytes>,
    send_rtt: mpsc::Sender<Duration>,
    send_err: oneshot::Sender<BackendError>,
    _send_managed: SendStream,
    _recv_managed: RecvStream,
}

pub async fn connection_channel<const SERVER: bool>(
    conn: &Connection,
    version: ProtocolVersion,
) -> Result<(ConnectionFrontend, ConnectionBackend), BackendError> {
    if conn.max_datagram_size().is_none() {
        return Err(BackendError::DatagramsNotSupported);
    }

    let (send_managed, recv_managed) = if SERVER {
        negotiate::server(&conn, version).await?
    } else {
        negotiate::client(&conn, version).await?
    };

    let (send_c2s, recv_c2s) = mpsc::unbounded();
    let (send_s2c, recv_s2c) = mpsc::channel(MSG_BUF_CAP);
    let (send_rtt, recv_rtt) = mpsc::channel(1);
    let (send_err, recv_err) = oneshot::channel();
    Ok((
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
            // we have to keep the managed streams alive
            // so we'll just pass them to the backend
            // this also lets us expand the functionality of managed streams
            // in the future
            _send_managed: send_managed,
            _recv_managed: recv_managed,
        },
    ))
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
            LaneKind::ReliableUnordered => Self::ReliableUnordered {},
            LaneKind::ReliableOrdered => Self::ReliableOrdered {},
        }
    }

    pub fn update(&mut self) {
        match self {
            Self::UnreliableUnsequenced { frag } => {
                frag.clean_up();
            }
            Self::UnreliableSequenced { frag } => {
                frag.clean_up();
            }
            Self::ReliableUnordered {} => {}
            Self::ReliableOrdered {} => {}
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
