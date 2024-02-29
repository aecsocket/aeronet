use aeronet::{LaneConfig, LaneKind};
use bytes::Bytes;
use octets::{Octets, OctetsMut};

use crate::Seq;

use super::{Lane, LaneError, LaneRecv, LaneState, Reliable, Unreliable};

/// Manages the internal state of multiple lanes.
///
/// See the [module-level docs](crate::lane).
#[derive(Debug)]
pub struct Lanes {
    lanes: Box<[Lane]>,
}

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

    pub fn poll(&mut self) -> Result<(), LaneError> {
        for lane in self.lanes.iter_mut() {
            lane.poll()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> impl Iterator<Item = Box<[u8]>> + '_ {
        self.lanes
            .iter_mut()
            .enumerate()
            .flat_map(|(lane_index, lane)| lane.flush().map(move |packet| (lane_index, packet)))
            .map(|(lane_index, packet)| {
                let lane_index = lane_index as u64;
                let lane_index_len = octets::varint_len(lane_index);
                let mut buf = vec![0; lane_index_len + packet.encode_len()].into_boxed_slice();
                let mut octs = OctetsMut::with_slice(&mut buf);
                octs.put_varint_with_len(lane_index, lane_index_len)
                    .unwrap();
                packet.encode(&mut octs).unwrap();
                buf
            })
    }
}
