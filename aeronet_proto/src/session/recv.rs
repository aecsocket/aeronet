use ahash::{AHashMap, AHashSet};
use octs::{BufError, BufTooShortOr, Bytes, Read};

use crate::{ack::Acknowledge, seq::Seq};

use super::Session;

#[derive(Debug, Clone)]
pub(super) enum RecvLane {
    UnreliableUnordered,
    UnreliableSequenced {
        last_recv_seq: Seq,
    },
    ReliableUnordered {
        pending_seq: Seq,
        recv_seq_buf: AHashSet<Seq>,
    },
    ReliableOrdered {
        pending_seq: Seq,
        recv_buf: AHashMap<Seq, Bytes>,
    },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RecvError {}

impl BufError for RecvError {}

impl Session {
    // pub fn new(send_lanes: impl IntoIterator<Item = LaneConfig>) -> Self {
    //     Session {}
    // }

    // pub fn buffer_send(&mut self, msg: Bytes, lane: LaneIndex) -> Result<(), SendError> {
    //     let lane = self
    //         .send_lanes
    //         .get_mut(lane.into_raw())
    //         .ok_or(SendError::InvalidLane)?;
    // }

    pub fn start_recv(&mut self, packet: impl Into<Bytes>) -> ReadAcks<'_> {
        let packet = packet.into();
        self.bytes_recv = self.bytes_recv.saturating_add(packet.len());
        ReadAcks {
            session: self,
            packet,
        }
    }
}

#[derive(Debug)]
pub struct ReadAcks<'s> {
    session: &'s mut Session,
    packet: Bytes,
}

impl<'s> ReadAcks<'s> {
    pub fn read_acks(
        mut self,
    ) -> Result<(impl Iterator<Item = Seq> + 's, ReadFrags<'s>), BufTooShortOr<RecvError>> {
        // mark this packet as acked;
        // this ack will later be sent out to the peer in `flush`
        let packet_seq = self.packet.read::<Seq>()?;
        self.session.acks.ack(packet_seq);

        // read packet seqs the peer has reported they've acked..
        // ..turn those into message seqs via our mappings..
        // ..perform our internal bookkeeping..
        // ..and return those message seqs to the caller
        let acks = self.packet.read::<Acknowledge>()?;
    }
}

#[derive(Debug)]
pub struct ReadFrags<'s> {
    session: &'s mut Session,
    packet: Bytes,
}
