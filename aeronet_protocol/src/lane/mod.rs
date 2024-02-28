mod ord;
mod reliable;
mod unreliable;

use self::ord::{Ordered, Unordered};

pub use {
    ord::{Sequenced, Unsequenced},
    reliable::*,
    unreliable::*,
};

use aeronet::MessageState;
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
    fn update(&mut self) -> Result<(), LaneUpdateError>;

    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError>;

    fn recv(&mut self, packet: &[u8]) -> (Vec<Bytes>, Result<(), LaneRecvError>);
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
    pub fn buffer_send(&mut self, lane_index: usize, msg: &[u8]) -> Result<Seq, LaneSendError> {
        self.lanes[lane_index].buffer_send(msg)
    }

    pub fn update(&mut self) -> Result<(), LaneUpdateError> {
        for lane in self.lanes.iter_mut() {
            lane.update()?;
        }
        Ok(())
    }

    pub fn recv(&mut self, packet: &[u8]) -> (Vec<Bytes>, Result<(), LaneRecvError>) {
        let mut packet = Octets::with_slice(packet);
        let lane_index = match packet.get_varint().map_err(|_| LaneRecvError::NoLaneIndex) {
            Ok(lane_index) => lane_index as usize,
            Err(err) => return (Vec::new(), Err(err)),
        };
        let lane = match self
            .lanes
            .get_mut(lane_index)
            .ok_or(LaneRecvError::InvalidLane { lane_index })
        {
            Ok(lane) => lane,
            Err(err) => return (Vec::new(), Err(err)),
        };
        lane.recv(packet.as_ref())
    }
}
