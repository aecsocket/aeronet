use std::time::Duration;

use aeronet::LaneConfig;
use bytes::Bytes;

use crate::{FragmentError, FragmentHeader, Fragmentation, ReassembleError, Seq};

#[derive(Debug)]
pub struct Lanes {
    pub max_packet_len: usize,
    lanes: Vec<LaneState>,
}

#[derive(Debug)]
enum LaneState {
    UnreliableUnsequenced {
        frag: Fragmentation,
        next_send_seq: Seq,
        clean_up_after: Duration,
    },
    UnreliableSequenced {
        frag: Fragmentation,
        next_send_seq: Seq,
        last_recv_seq: Seq,
        clean_up_after: Duration,
    },
    ReliableUnordered {},
    ReliableSequenced {},
    ReliableOrdered {},
}

impl LaneState {
    fn new(max_packet_size: usize, config: &LaneConfig) -> Self {
        const VARINT_MAX_SIZE: usize = 10;

        match config {
            LaneConfig::UnreliableUnsequenced { clean_up_after } => {
                const MIN_PACKET_SIZE: usize = VARINT_MAX_SIZE + FragmentHeader::ENCODE_SIZE;
                assert!(max_packet_size > MIN_PACKET_SIZE);
                LaneState::UnreliableUnsequenced {
                    frag: Fragmentation::new(max_packet_size),
                    next_send_seq: Seq(0),
                    clean_up_after,
                }
            }
            LaneConfig::UnreliableSequenced { clean_up_after } => {
                const MIN_PACKET_SIZE: usize = VARINT_MAX_SIZE + FragmentHeader::ENCODE_SIZE;
                assert!(max_packet_size > MIN_PACKET_SIZE);
                LaneState::UnreliableSequenced {
                    frag: Fragmentation::sequenced(),
                    next_send_seq: Seq(0),
                    last_recv_seq: Seq(0),
                    clean_up_after,
                }
            }
            LaneConfig::ReliableUnordered => LaneState::ReliableUnordered {},
            LaneConfig::ReliableSequenced => LaneState::ReliableSequenced {},
            LaneConfig::ReliableOrdered => LaneState::ReliableOrdered {},
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneSendError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LaneRecvError {
    #[error("failed to read lane index")]
    ReadIndex,
    #[error("received message on invalid lane index {lane_index}")]
    InvalidLane { lane_index: usize },
    #[error("failed to reassemble packet")]
    Reassemble(#[source] ReassembleError),
}

impl Lanes {
    // todo docs
    /// # Panics
    ///
    /// Panics if `max_packet_size` is too small one of the lanes, or if
    /// `lanes.len() > u64::MAX`.
    #[must_use]
    pub fn new(max_packet_size: usize, lanes: &[LaneConfig]) -> Self {
        assert!(max_packet_size > 0);
        u64::try_from(lanes.len()).expect("should be less than `u64::MAX` lanes");
        let lanes = lanes
            .iter()
            .map(|config| LaneState::new(max_packet_size, config))
            .collect();
        Self {
            max_packet_len: max_packet_size,
            lanes,
        }
    }

    pub fn update(&mut self) {
        for lane in &mut self.lanes {
            match lane {
                LaneState::UnreliableUnsequenced {
                    frag,
                    clean_up_after,
                    ..
                } => frag.clean_up(clean_up_after),
                LaneState::UnreliableSequenced {
                    frag,
                    clean_up_after,
                    ..
                } => frag.clean_up(clean_up_after),
                LaneState::ReliableUnordered {} => todo!(),
                LaneState::ReliableSequenced {} => todo!(),
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
        lane_index: usize,
        msg: &'a [u8],
    ) -> Result<Vec<LanePacket<'a>>, LaneSendError> {
        let lane = &mut self.lanes[lane_index];
        let lane_index = lane_index as u64;

        match lane {
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
            LaneState::ReliableSequenced {} => todo!(),
            LaneState::ReliableUnordered {} => todo!(),
            LaneState::ReliableOrdered {} => todo!(),
        }
    }
}

fn send_unreliable<'a, S>(
    max_packet_len: usize,
    msg: &'a [u8],
    lane_index: u64,
    frag: &mut Fragmentation,
) -> Result<Vec<LanePacket<'a>>, LaneSendError> {
    let lane_header_len = lane_index.required_space();
    let payload_len = max_packet_len - lane_header_len - FRAG_HEADER_LEN;

    Ok(frag
        .fragment(msg)
        .map_err(LaneSendError::Fragment)?
        .map(|data| {
            let mut header = vec![0; lane_header_len + FRAG_HEADER_LEN].into_boxed_slice();

            let bytes_written = lane_index.encode_var(&mut header[..lane_header_len]);
            debug_assert_eq!(lane_header_len, bytes_written);
            header[lane_header_len..].copy_from_slice(&data.header);

            LanePacket {
                header: Bytes::from(header),
                payload: data.payload,
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
