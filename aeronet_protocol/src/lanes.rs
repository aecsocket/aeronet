use aeronet::{LaneConfig, LaneKind};
use bytes::Bytes;
use integer_encoding::VarInt;

use crate::{
    Fragmentation, FragmentationError, ReassemblyError, Sequenced, Unsequenced, FRAG_HEADER_LEN,
};

#[derive(Debug)]
pub struct Lanes {
    pub max_packet_len: usize,
    lanes: Vec<LaneState>,
}

#[derive(Debug)]
enum LaneState {
    UnreliableUnsequenced { frag: Fragmentation<Unsequenced> },
    UnreliableSequenced { frag: Fragmentation<Sequenced> },
    ReliableUnordered {},
    ReliableOrdered {},
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneSendError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentationError),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneRecvError {
    #[error("failed to read lane index")]
    ReadIndex,
    #[error("received message on invalid lane index {lane_index}")]
    InvalidLane { lane_index: usize },
    #[error("failed to reassemble packet")]
    Reassemble(#[source] ReassemblyError),
}

#[derive(Debug)]
pub struct LanePacket<'a> {
    pub header: Bytes,
    pub payload: &'a [u8],
}

impl Lanes {
    // todo docs
    /// # Panics
    ///
    /// Panics if `max_packet_len` is 0, or if `lanes.len() > u64::MAX`.
    #[must_use]
    pub fn new(max_packet_len: usize, lanes: &[LaneConfig]) -> Self {
        assert!(max_packet_len > 0);
        u64::try_from(lanes.len()).expect("should be less than `u64::MAX` lanes");

        let lanes = lanes
            .iter()
            .map(|config| match config.kind {
                LaneKind::UnreliableUnsequenced => LaneState::UnreliableUnsequenced {
                    frag: Fragmentation::unsequenced(),
                },
                LaneKind::UnreliableSequenced => LaneState::UnreliableSequenced {
                    frag: Fragmentation::sequenced(),
                },
                LaneKind::ReliableUnordered => LaneState::ReliableUnordered {},
                LaneKind::ReliableOrdered => LaneState::ReliableOrdered {},
            })
            .collect();

        Self {
            max_packet_len,
            lanes,
        }
    }

    pub fn update(&mut self) {
        for lane in &mut self.lanes {
            match lane {
                LaneState::UnreliableUnsequenced { frag } => frag.clean_up(),
                LaneState::UnreliableSequenced { frag } => frag.clean_up(),
                LaneState::ReliableUnordered {} => todo!(),
                LaneState::ReliableOrdered {} => todo!(),
            }
        }
    }

    // todo
    /// # Panics
    ///
    /// Panics if `lane` is not a valid index into the lanes configured on
    /// creation.
    pub fn send<'a>(
        &mut self,
        msg: &'a [u8],
        lane: usize,
    ) -> Result<Vec<LanePacket<'a>>, LaneSendError> {
        let lane_state = &mut self.lanes[lane];
        let lane_index = u64::try_from(lane).expect("should be validated on construction");

        match lane_state {
            LaneState::UnreliableUnsequenced { frag } => {
                send_unreliable(self.max_packet_len, msg, lane_index, frag)
            }
            LaneState::UnreliableSequenced { frag } => {
                send_unreliable(self.max_packet_len, msg, lane_index, frag)
            }
            _ => todo!(),
        }
    }

    pub fn recv(&mut self, packet: &[u8]) -> Result<Option<Bytes>, LaneRecvError> {
        let (lane_index, bytes_read) = u64::decode_var(packet).ok_or(LaneRecvError::ReadIndex)?;
        let lane_index = usize::try_from(lane_index).map_err(|_| LaneRecvError::ReadIndex)?;
        let lane_state = self
            .lanes
            .get_mut(lane_index)
            .ok_or(LaneRecvError::InvalidLane { lane_index })?;

        let packet = &packet[bytes_read..];
        match lane_state {
            LaneState::UnreliableUnsequenced { frag } => {
                frag.reassemble(packet).map_err(LaneRecvError::Reassemble)
            }
            LaneState::UnreliableSequenced { frag } => {
                frag.reassemble(packet).map_err(LaneRecvError::Reassemble)
            }
            LaneState::ReliableUnordered {} => todo!(),
            LaneState::ReliableOrdered {} => todo!(),
        }
    }
}

fn send_unreliable<'a, S>(
    max_packet_len: usize,
    msg: &'a [u8],
    lane_index: u64,
    frag: &mut Fragmentation<S>,
) -> Result<Vec<LanePacket<'a>>, LaneSendError> {
    let lane_header_len = lane_index.required_space();
    let payload_len = max_packet_len - lane_header_len - FRAG_HEADER_LEN;

    Ok(frag
        .fragment(msg, payload_len)
        .map_err(LaneSendError::Fragment)?
        .map(|frag_packet| {
            let mut header = vec![0; lane_header_len + FRAG_HEADER_LEN].into_boxed_slice();

            let bytes_written = lane_index.encode_var(&mut header[..lane_header_len]);
            debug_assert_eq!(lane_header_len, bytes_written);
            header[lane_header_len..].copy_from_slice(&frag_packet.header);

            LanePacket {
                header: Bytes::from(header),
                payload: frag_packet.payload,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use assert_matches::assert_matches;

    use super::*;

    const MTU: usize = 1024;
    const MSG1: &[u8] = b"Message 1";

    const ONE_LANE: &[LaneConfig] = &[LaneConfig {
        kind: LaneKind::UnreliableUnsequenced,
    }];

    fn b(packet: &LanePacket<'_>) -> Vec<u8> {
        packet
            .header
            .iter()
            .chain(packet.payload)
            .copied()
            .collect::<Vec<_>>()
    }

    #[test]
    fn one_lane() {
        let mut lanes = Lanes::new(MTU, ONE_LANE);
        let packets = lanes.send(MSG1, 0).unwrap();

        assert_eq!(1, packets.len());
        assert_matches!(lanes.recv(&b(&packets[0])), Ok(Some(m)) if &m == MSG1);
    }
}
