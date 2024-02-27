mod unreliable;

use enum_dispatch::enum_dispatch;
use octets::Octets;
pub use unreliable::*;

use crate::{FragmentError, Seq};

const LANE_INDEX_SIZE: usize = 10;

#[enum_dispatch]
pub trait LaneState {
    fn update(&mut self);

    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError>;

    fn recv(&mut self, msg: &[u8]) -> Result<(), LaneRecvError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneSendError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneRecvError {
    #[error("failed to read lane index")]
    ReadLaneIndex,
    #[error("invalid lane index {lane_index}")]
    InvalidLane { lane_index: usize },
}

#[derive(Debug)]
#[enum_dispatch(LaneState)]
pub enum Lane {
    UnreliableUnsequenced,
}

#[derive(Debug)]
pub struct Lanes {
    lanes: Box<[Lane]>,
}

impl Lanes {
    pub fn buffer_send(&mut self, lane_index: usize, msg: &[u8]) -> Result<Seq, LaneSendError> {
        self.lanes[lane_index].buffer_send(msg)
    }

    pub fn update(&mut self) {
        for lane in self.lanes.iter_mut() {
            lane.update();
        }
    }

    pub fn recv(&mut self, packet: &[u8]) -> Result<(), LaneRecvError> {
        let mut packet = Octets::with_slice(packet);
        let lane_index = packet
            .get_varint()
            .map_err(|_| LaneRecvError::ReadLaneIndex)? as usize;
        let lane = self
            .lanes
            .get_mut(lane_index)
            .ok_or(LaneRecvError::InvalidLane { lane_index })?;
        lane.recv(packet.as_ref())
    }
}
