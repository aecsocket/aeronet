pub mod ord;
pub mod reliable;
pub mod unreliable;

pub use ord::{Ordered, Sequenced, Unordered};
pub use reliable::Reliable;
pub use unreliable::Unreliable;

use aeronet::{LaneConfig, LaneKind};
use bytes::Bytes;
use enum_dispatch::enum_dispatch;
use octets::{Octets, OctetsMut};

use crate::{FragmentError, ReassembleError, Seq};

const VARINT_MAX_SIZE: usize = 10;

// impl details: I considered adding a `message_state` here to get the current
// state of a message by its Seq, but imo this is too unreliable and we can't
// provide strong enough guarantees to get the *current* state of a message
#[enum_dispatch]
pub trait LaneState {
    fn buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError>;

    fn recv(&mut self, packet: Bytes) -> Result<LaneRecv<'_>, LaneError>;

    fn send_buffered(&mut self) -> Result<LaneSend<'_>, LaneError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneError {
    #[error("failed to receive a message in time")]
    RecvTimeout,
    #[error("failed to send a message in time")]
    SendTimeout,

    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),

    #[error("input too short to contain lane index")]
    NoLaneIndex,
    #[error("invalid lane index {lane_index}")]
    InvalidLane { lane_index: usize },
    #[error("fragment reported {len} bytes, but buffer only had {cap} more")]
    TooLong { len: usize, cap: usize },
    #[error("input too short to contain sequence number")]
    NoSeq,
    #[error("input too short to contain ack header data")]
    NoAckHeader,
    #[error("input too short to contain frag header data")]
    NoFragHeader,
    #[error("frag header is invalid")]
    InvalidFragHeader,
    #[error("failed to reassemble payload")]
    Reassemble(#[source] ReassembleError),
}

#[derive(Debug)]
#[enum_dispatch(LaneState)]
pub enum Lane {
    UnreliableUnordered(Unreliable<Unordered>),
    UnreliableSequenced(Unreliable<Sequenced>),
    ReliableUnordered(Reliable<Unordered>),
    ReliableSequenced(Reliable<Sequenced>),
    ReliableOrdered(Reliable<Ordered>),
}

#[derive(Debug)]
pub enum LaneSend<'l> {
    Unreliable(unreliable::Send<'l>),
    ReliableUnordered(reliable::Send<'l, Unordered>),
    ReliableSequenced(reliable::Send<'l, Sequenced>),
    ReliableOrdered(reliable::Send<'l, Ordered>),
}

impl Iterator for LaneSend<'_> {
    type Item = Bytes;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Unreliable(iter) => todo!(),
            Self::ReliableUnordered(iter) => iter.next(),
            Self::ReliableSequenced(iter) => iter.next(),
            Self::ReliableOrdered(iter) => iter.next(),
        }
    }
}

pub enum LaneRecv<'l> {
    UnreliableUnordered(unreliable::Recv<'l, Unordered>),
    UnreliableSequenced(unreliable::Recv<'l, Sequenced>),
    ReliableUnordered(reliable::Recv<'l, Unordered>),
    ReliableSequenced(reliable::Recv<'l, Sequenced>),
    ReliableOrdered(reliable::Recv<'l, Ordered>),
}

impl Iterator for LaneRecv<'_> {
    type Item = Result<Bytes, LaneError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::UnreliableUnordered(iter) => iter.next(),
            Self::UnreliableSequenced(iter) => iter.next(),
            Self::ReliableUnordered(iter) => iter.next(),
            Self::ReliableSequenced(iter) => iter.next(),
            Self::ReliableOrdered(iter) => iter.next(),
        }
    }
}

#[derive(Debug)]
pub struct Lanes {
    lanes: Box<[Lane]>,
}

/*
pseudocode of packet layout:

struct Packet {
    lane_index: Varint,
    payload: [..]
}

struct unreliable::Payload {
    frags: [Fragment]
}

struct reliable::Payload {
    ack_header: AckHeader,
    frags: [Fragment]
}

struct Fragment {
    len: Varint,
    seq: Seq,
    frag_header: FragHeader
}
 */

impl Lanes {
    pub fn new(max_packet_len: usize, lanes: &[LaneConfig]) -> Self {
        let lanes = lanes
            .iter()
            .map(|config| match config.kind {
                LaneKind::UnreliableUnsequenced => {
                    Unreliable::unordered(max_packet_len, config).into()
                }
                LaneKind::UnreliableSequenced => {
                    Unreliable::sequenced(max_packet_len, config).into()
                }
                LaneKind::ReliableUnordered => Reliable::unordered(max_packet_len, config).into(),
                LaneKind::ReliableSequenced => Reliable::sequenced(max_packet_len, config).into(),
                LaneKind::ReliableOrdered => Reliable::ordered(max_packet_len, config).into(),
            })
            .collect();
        Self { lanes }
    }

    // todo docs
    /// # Panics
    ///
    /// If `lane_index` is outside the range of the original slice of lanes
    /// passed into [`Lanes::new`], this will panic.
    pub fn buffer_send(&mut self, lane_index: usize, msg: Bytes) -> Result<Seq, LaneError> {
        self.lanes[lane_index].buffer_send(msg)
    }

    pub fn recv(&mut self, packet: Bytes) -> Result<LaneRecv<'_>, LaneError> {
        let mut octs = Octets::with_slice(&packet);
        let lane_index = octs.get_varint().map_err(|_| LaneError::NoLaneIndex)?;
        let lane_index = lane_index as usize;
        let lane = self
            .lanes
            .get_mut(lane_index)
            .ok_or(LaneError::InvalidLane { lane_index })?;

        let packet = packet.slice(octs.off()..);
        Ok(lane.recv(packet)?)
    }

    pub fn send_buffered(&mut self) -> Result<impl Iterator<Item = Bytes> + '_, LaneError> {
        // TODO remove this Vec allocation somehow
        let iters = self
            .lanes
            .iter_mut()
            .enumerate()
            .map(|(i, lane)| lane.send_buffered().map(|iter| (i, iter)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(iters
            .into_iter()
            .map(|(lane_index, iter)| {
                let lane_index = lane_index as u64;
                let lane_index_len = octets::varint_len(lane_index);
                iter.map(move |payload| {
                    // reallocate here
                    // TODO we really need to reduce reallocs
                    let mut buf = vec![0; lane_index_len + payload.len()].into_boxed_slice();
                    let mut octs = OctetsMut::with_slice(&mut buf);
                    octs.put_varint_with_len(lane_index, lane_index_len)
                        .unwrap();
                    octs.put_bytes(&payload).unwrap();
                    Bytes::from(buf)
                })
            })
            .flatten())
    }
}
