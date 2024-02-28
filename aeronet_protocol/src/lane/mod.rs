pub mod ord;
pub mod reliable;
pub mod unreliable;

pub use ord::{Ordered, Sequenced, Unordered, Unsequenced};
pub use reliable::Reliable;
pub use unreliable::Unreliable;

use aeronet::{LaneConfig, LaneKind};
use bytes::Bytes;
use enum_dispatch::enum_dispatch;
use octets::Octets;

use crate::{FragmentError, ReassembleError, Seq};

const VARINT_MAX_SIZE: usize = 10;

// impl details: I considered adding a `message_state` here to get the current
// state of a message by its Seq, but imo this is too unreliable and we can't
// provide strong enough guarantees to get the *current* state of a message
#[enum_dispatch]
pub trait LaneState {
    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError>;

    fn recv<'packet>(&mut self, packet: &'packet [u8]) -> LaneRecv<'_, 'packet>;

    fn poll(&mut self) -> Result<(), LaneUpdateError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneUpdateError {
    #[error("failed to receive a message in time")]
    RecvTimeout,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneSendError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneRecvError {
    #[error("input too short to contain lane index")]
    NoLaneIndex,
    #[error("invalid lane index {lane_index}")]
    InvalidLane { lane_index: usize },
    #[error("fragment reported {len} bytes, but buffer only had {cap} more")]
    TooLong { len: usize, cap: usize },
    #[error("input too short to contain sequence number")]
    NoSeq,
    #[error("input too short to contain header data")]
    NoHeader,
    #[error("header is invalid")]
    InvalidHeader,
    #[error("failed to reassemble payload")]
    Reassemble(#[source] ReassembleError),
}

#[derive(Debug)]
#[enum_dispatch(LaneState)]
pub enum Lane {
    UnreliableUnsequenced(Unreliable<Unsequenced>),
    UnreliableSequenced(Unreliable<Sequenced>),
    ReliableUnordered(Reliable<Unordered>),
    ReliableSequenced(Reliable<Sequenced>),
    ReliableOrdered(Reliable<Ordered>),
}

pub enum LaneRecv<'l, 'p> {
    UnreliableUnsequenced(unreliable::Recv<'l, 'p, Unsequenced>),
    UnreliableSequenced(unreliable::Recv<'l, 'p, Sequenced>),
    ReliableUnordered(reliable::Recv<'l, 'p, Unordered>),
    ReliableSequenced(reliable::Recv<'l, 'p, Sequenced>),
    ReliableOrdered(reliable::Recv<'l, 'p, Ordered>),
}

impl Iterator for LaneRecv<'_, '_> {
    type Item = Result<Bytes, LaneRecvError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::UnreliableUnsequenced(iter) => iter.next(),
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
                    Unreliable::unsequenced(max_packet_len, config).into()
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

    pub fn buffer_send(&mut self, lane_index: usize, msg: &[u8]) -> Result<Seq, LaneSendError> {
        self.lanes[lane_index].buffer_send(msg)
    }

    pub fn recv<'p>(&mut self, packet: &'p [u8]) -> Result<LaneRecv<'_, 'p>, LaneRecvError> {
        let mut octs = Octets::with_slice(packet);
        let lane_index = octs.get_varint().map_err(|_| LaneRecvError::NoLaneIndex)?;
        let lane_index = lane_index as usize;
        let lane = self
            .lanes
            .get_mut(lane_index)
            .ok_or(LaneRecvError::InvalidLane { lane_index })?;
        // manually slice here rather than `octs.as_ref()`
        // because we need to prove that this is bound by 'p
        Ok(lane.recv(&packet[octs.off()..]))
    }

    pub fn poll(&mut self) -> Result<(), LaneUpdateError> {
        for lane in self.lanes.iter_mut() {
            lane.poll()?;
        }
        Ok(())
    }
}
