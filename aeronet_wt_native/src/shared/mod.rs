mod backend;

pub use backend::*;
use tracing::debug;

use std::time::Duration;

use aeronet::{
    protocol::{Fragmentation, Negotiation, NegotiationError, Sequenced, Unsequenced},
    LaneKind, VersionedProtocol,
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

pub async fn connection_channel<P: VersionedProtocol, const SERVER: bool>(
    conn: &Connection,
) -> Result<(ConnectionFrontend, ConnectionBackend), BackendError> {
    if conn.max_datagram_size().is_none() {
        return Err(BackendError::DatagramsNotSupported);
    }

    debug!(
        "Starting negotiation - our protocol version: {}",
        P::VERSION
    );
    let negotiation = Negotiation::from_protocol::<P>();
    let (send_managed, recv_managed) = if SERVER {
        let (mut send_managed, mut recv_managed) = conn
            .open_bi()
            .await
            .map_err(BackendError::OpeningManaged)?
            .await
            .map_err(BackendError::OpenManaged)?;

        // recv response
        debug!("Opened managed stream, waiting for response");
        let mut resp_buf = [0; Negotiation::HEADER_LEN];
        let bytes_read = recv_managed
            .read(&mut resp_buf)
            .await
            .map_err(BackendError::RecvNegotiateResponse)?
            .ok_or(BackendError::ManagedStreamClosed)?;
        if bytes_read != Negotiation::HEADER_LEN {
            return Err(BackendError::Negotiate(NegotiationError::InvalidHeader));
        }
        // check response
        negotiation
            .check_response(&resp_buf[..bytes_read])
            .map_err(BackendError::Negotiate)?;
        // send ok
        debug!("Negotiation success, sending ok");
        send_managed
            .write_all(Negotiation::OK)
            .await
            .map_err(BackendError::SendManaged)?;

        (send_managed, recv_managed)
    } else {
        let (mut send_managed, mut recv_managed) = conn
            .accept_bi()
            .await
            .map_err(BackendError::AcceptManaged)?;

        // send request
        debug!("Opened managed stream, sending request");
        send_managed
            .write_all(&negotiation.create_request())
            .await
            .map_err(BackendError::SendManaged)?;
        // wait for ok
        debug!("Waiting for ok");
        let mut resp_buf = [0; Negotiation::OK.len()];
        let bytes_read = recv_managed
            .read(&mut resp_buf)
            .await
            // if there's a read error, it's probably because server closed connection
            // likely because we have the wrong protocol version
            .ok()
            .flatten()
            .ok_or(BackendError::Negotiate(NegotiationError::OurWrongVersion {
                ours: P::VERSION,
            }))?;
        if bytes_read != Negotiation::OK.len() || &resp_buf[..bytes_read] != Negotiation::OK {
            return Err(BackendError::RecvNegotiateOk);
        }
        debug!("Negotiation success");

        (send_managed, recv_managed)
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

impl ConnectionFrontend {
    pub fn update(&mut self) {
        while let Ok(Some(rtt)) = self.recv_rtt.try_next() {
            self.info.rtt = rtt;
        }
    }

    pub fn send(&mut self, msg: Bytes) -> Result<(), BackendError> {
        self.info.total_bytes_sent += msg.len();
        self.send_c2s
            .unbounded_send(msg)
            .map_err(|_| BackendError::Closed)
    }

    pub fn recv(&mut self) -> Option<Bytes> {
        match self.recv_s2c.try_next() {
            Ok(None) | Err(_) => None,
            Ok(Some(msg)) => {
                self.info.total_bytes_recv += msg.len();
                Some(msg)
            }
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
