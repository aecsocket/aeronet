mod backend;
mod frontend;
mod negotiate;

pub use backend::*;
use bitcode::{Decode, Encode};
use derivative::Derivative;
use integer_encoding::VarInt;

use std::{marker::PhantomData, time::Duration};

use aeronet::{
    protocol::{Fragmentation, Sequenced, Unsequenced, FRAG_HEADER_SIZE, MTU},
    LaneKey, LaneKind, LaneProtocol, ProtocolVersion, TryAsBytes, TryFromBytes,
};
use bytes::Bytes;
use futures::channel::{mpsc, oneshot};
use wtransport::{Connection, RecvStream, SendStream};

use crate::{BackendError, ConnectionInfo, WebTransportError};

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
        negotiate::server(conn, version).await?
    } else {
        negotiate::client(conn, version).await?
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

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Lanes<P> {
    lanes: Vec<LaneState>,
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<P>,
}

#[derive(Debug, Clone, Encode, Decode)]
struct LaneHeader {
    lane: u64,
}

#[derive(Debug)]
enum LaneState {
    UnreliableUnsequenced { frag: Fragmentation<Unsequenced> },
    UnreliableSequenced { frag: Fragmentation<Sequenced> },
    ReliableUnordered {},
    ReliableOrdered {},
}

impl<P: LaneProtocol> Lanes<P> {
    pub fn new() -> Self {
        u64::try_from(P::Lane::VARIANTS.len()).expect("must be no more than `u64::MAX` lanes");
        let mut lanes = Vec::new();
        lanes.reserve_exact(P::Lane::VARIANTS.len());
        lanes.extend(P::Lane::VARIANTS.iter().map(|lane| match lane.kind() {
            LaneKind::UnreliableUnsequenced => LaneState::UnreliableUnsequenced {
                frag: Fragmentation::unsequenced(),
            },
            LaneKind::UnreliableSequenced => LaneState::UnreliableSequenced {
                frag: Fragmentation::sequenced(),
            },
            LaneKind::ReliableUnordered => LaneState::ReliableUnordered {},
            LaneKind::ReliableOrdered => LaneState::ReliableOrdered {},
        }));

        Self {
            lanes,
            _phantom: PhantomData,
        }
    }

    pub fn update(&mut self) {
        for lane in &mut self.lanes {
            match lane {
                LaneState::UnreliableUnsequenced { frag } => frag.clean_up(),
                LaneState::UnreliableSequenced { frag } => frag.clean_up(),
                LaneState::ReliableUnordered {} | LaneState::ReliableOrdered {} => {}
            }
        }
    }

    pub fn send<'a>(
        &'a mut self,
        msg: &'a [u8],
        lane: P::Lane,
        conn: &mut ConnectionFrontend,
    ) -> Result<(), BackendError> {
        tracing::info!("msg len = {}", msg.len());
        self.create_outgoing(msg, lane, |packet| {
            tracing::info!("pkt len = {}", packet.len());
            conn.info.total_bytes_sent += packet.len();
            let _ = conn.send(packet);
        })?;
        conn.info.msg_bytes_sent += msg.len();
        conn.info.msgs_sent += 1;
        Ok(())
    }

    fn create_outgoing(
        &mut self,
        msg: &[u8],
        lane: P::Lane,
        f: impl FnMut(Bytes),
    ) -> Result<(), BackendError> {
        fn unreliable<S>(
            msg: &[u8],
            lane_header: &[u8],
            frag: &mut Fragmentation<S>,
            f: impl FnMut(Bytes),
        ) -> Result<(), BackendError> {
            let payload_size = MTU - lane_header.len() - FRAG_HEADER_SIZE;
            frag.fragment(msg, payload_size)
                .map_err(BackendError::Fragment)?
                .map(|frag_packet| {
                    let frag_start = lane_header.len();
                    let payload_start = frag_start + FRAG_HEADER_SIZE;
                    let mut packet =
                        vec![0; payload_start + frag_packet.payload.len()].into_boxed_slice();
                    packet[..frag_start].copy_from_slice(lane_header);
                    packet[frag_start..payload_start].copy_from_slice(&frag_packet.header);
                    packet[payload_start..].copy_from_slice(frag_packet.payload);
                    Bytes::from(packet.into_vec())
                })
                .for_each(f);
            Ok(())
        }

        let lane_state = self
            .lanes
            .get_mut(lane.variant())
            .expect("`P::Lane::variant` should be a valid index into `P::Lane::VARIANTS`");
        let lane = u64::try_from(lane.variant()).expect("should be validated on construction");
        let lane_header = lane.encode_var_vec();

        match lane_state {
            LaneState::UnreliableUnsequenced { frag } => unreliable(msg, &lane_header, frag, f),
            LaneState::UnreliableSequenced { frag } => unreliable(msg, &lane_header, frag, f),
            _ => todo!(),
        }
    }

    pub fn recv<S: TryAsBytes, R: TryFromBytes>(
        &mut self,
        conn: &mut ConnectionFrontend,
    ) -> Result<Option<R>, WebTransportError<S, R>> {
        while let Some(packet) = conn.recv() {
            conn.info.total_bytes_recv += packet.len();
            if let Some(msg_bytes) = self
                .recv_incoming(&packet)
                .map_err(WebTransportError::<S, R>::Backend)?
            {
                let msg =
                    R::try_from_bytes(&msg_bytes).map_err(WebTransportError::<S, R>::Decode)?;
                conn.info.msg_bytes_recv += msg_bytes.len();
                conn.info.msgs_recv += 1;
                return Ok(Some(msg));
            }
        }
        Ok(None)
    }

    fn recv_incoming(&mut self, packet: &[u8]) -> Result<Option<Bytes>, BackendError> {
        let (lane_index, bytes_read) = u64::decode_var(packet).ok_or(BackendError::ReadLane)?;
        let lane_index = usize::try_from(lane_index).map_err(|_| BackendError::ReadLane)?;
        let lane_state = self
            .lanes
            .get_mut(lane_index)
            .ok_or(BackendError::RecvOnInvalidLane { lane_index })?;

        let packet = &packet[bytes_read..];
        match lane_state {
            LaneState::UnreliableUnsequenced { frag } => {
                frag.reassemble(packet).map_err(BackendError::Reassemble)
            }
            LaneState::UnreliableSequenced { frag } => {
                frag.reassemble(packet).map_err(BackendError::Reassemble)
            }
            LaneState::ReliableUnordered {} => todo!(),
            LaneState::ReliableOrdered {} => todo!(),
        }
    }
}
