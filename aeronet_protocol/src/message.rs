use aeronet::lane::{LaneConfig, LaneIndex, LaneKind};
use ahash::AHashMap;
use bytes::{Buf, Bytes};

use crate::{
    ack::{AckHeader, Acknowledge},
    bytes::BytesError,
    frag::{FragHeader, Fragment, FragmentError, Fragmentation, ReassembleError},
    seq::Seq,
};

#[derive(Debug)]
pub struct Messages {
    frag: Fragmentation,
    ack: Acknowledge,
    lanes: Vec<LaneState>,
    max_packet_len: usize,
    next_send_packet_seq: Seq,
    next_send_msg_seq: Seq,
    unacked_msgs: AHashMap<Seq, UnackedMessage>,
    sent_packets: AHashMap<Seq, Vec<SentFrag>>,
}

#[derive(Debug)]
struct UnackedMessage {
    frags_remaining: u8,
    unacked_frags: Box<[Option<Fragment>]>,
}

#[derive(Debug)]
struct SentFrag {
    msg_seq: Seq,
    frag_id: u8,
}

#[derive(Debug)]
enum LaneState {
    Unordered,
    Sequenced { last_recv_msg_seq: Seq },
    Ordered,
}

impl LaneState {
    pub fn new(kind: LaneKind) -> Self {
        match kind {
            LaneKind::UnreliableUnordered | LaneKind::ReliableUnordered => Self::Unordered,
            LaneKind::UnreliableSequenced | LaneKind::ReliableSequenced => Self::Sequenced {
                last_recv_msg_seq: Seq(0),
            },
            LaneKind::ReliableOrdered => Self::Ordered,
        }
    }

    pub fn on_recv(&mut self, frag: &FragHeader) -> LaneRecvResult {
        match self {
            Self::Unordered => LaneRecvResult::Recv,
            Self::Sequenced { last_recv_msg_seq } => {
                if frag.msg_seq < *last_recv_msg_seq {
                    LaneRecvResult::Drop
                } else {
                    *last_recv_msg_seq = frag.msg_seq;
                    LaneRecvResult::Recv
                }
            }
            Self::Ordered => todo!(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum LaneRecvResult {
    Recv,
    Drop,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum MessageError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),

    #[error("failed to read packet seq")]
    ReadPacketSeq(#[source] BytesError),
    #[error("failed to read ack header")]
    ReadAckHeader(#[source] BytesError),
    #[error("failed to read fragment")]
    ReadFragment(#[source] BytesError),
    #[error("failed to reassemble fragment")]
    Reassemble(#[source] ReassembleError),
    #[error("invalid lane index {lane_index:?}")]
    InvalidLaneIndex { lane_index: LaneIndex },
}

const PACKET_HEADER_LEN: usize = Seq::ENCODE_SIZE + AckHeader::ENCODE_SIZE;

impl Messages {
    pub fn new(max_packet_len: usize, lanes: impl IntoIterator<Item = LaneConfig>) -> Self {
        assert!(max_packet_len > PACKET_HEADER_LEN);
        Self {
            frag: Fragmentation::new(max_packet_len - PACKET_HEADER_LEN),
            ack: Acknowledge::new(),
            lanes: lanes
                .into_iter()
                .map(|config| LaneState::new(config.kind))
                .collect(),
            max_packet_len,
            next_send_msg_seq: Seq(0),
            next_send_packet_seq: Seq(0),
            unacked_msgs: AHashMap::new(),
            sent_packets: AHashMap::new(),
        }
    }

    pub fn buffer_send(&mut self, lane_index: LaneIndex, msg: Bytes) -> Result<Seq, MessageError> {
        let msg_seq = self.next_send_msg_seq.get_inc();
        let frags = self
            .frag
            .fragment(msg_seq, lane_index, msg)
            .map_err(MessageError::Fragment)?;
        self.unacked_msgs.insert(
            msg_seq,
            UnackedMessage {
                frags_remaining: frags.num_frags(),
                unacked_frags: frags.map(Some).collect(),
            },
        );
        Ok(msg_seq)
    }

    pub fn flush(&mut self, available_bytes: &mut usize) -> impl Iterator<Item = Bytes> + '_ {
        // TODO
        let mut frags = self
            .unacked_msgs
            .iter()
            .flat_map(|(_, msg)| msg.unacked_frags.iter().filter_map(Option::as_ref));
        std::iter::empty()
    }

    // pub fn flush<'a>(
    //     &'a mut self,
    //     available_bytes: &'a mut usize,
    // ) -> impl Iterator<Item = Bytes> + 'a {
    //     let mut frags = self
    //         .unacked_msgs
    //         .iter()
    //         .flat_map(|(_, msg)| msg.unacked_frags.iter().filter_map(Option::as_ref));
    //     // we're fighting with two capacities here, effectively:
    //     // * `available_bytes`
    //     // * `packet`, which has `max_packet_len` capacity
    //     //   * `PACKET_HEADER_LEN` is reserved for packet header info
    //     //   * some more is reserved for each fragment's header info
    //     std::iter::from_fn(move || {
    //         if *available_bytes < PACKET_HEADER_LEN {
    //             return None;
    //         }
    //         let mut packet = BytesMut::with_capacity(self.max_packet_len.min(*available_bytes));
    //         // PANIC SAFETY: `PACKET_HEADER_LEN` defines how big the encoding of the packer header will be
    //         // the packet's capacity is `min(max_packet_len, available_bytes)`
    //         // we just checked that `available_bytes > PACKET_HEADER_LEN`,
    //         // and in `new` we checked that `max_packet_len > PACKET_HEADER_LEN`
    //         // therefore these unwraps will never panic
    //         self.next_send_msg_seq
    //             .get_inc()
    //             .encode(&mut packet)
    //             .unwrap();
    //         self.ack.create_header().encode(&mut packet).unwrap();
    //         *available_bytes -= PACKET_HEADER_LEN;

    //         // try to encode as many fragments as we can in the limited buffer space we have
    //         let mut frags_encoded: u32 = 0;
    //         while let Some(frag) = frags.next() {
    //             let frag_encode_len = frag.max_encode_len();
    //             if frag_encode_len > *available_bytes || frag_encode_len > packet.remaining_mut() {
    //                 break;
    //             }
    //             // PANIC SAFETY: we just checked that the encoded len of the frag
    //             // won't exceed our current bounds
    //             *available_bytes -= frag.encode(&mut packet).unwrap();
    //             frags_encoded += 1;
    //         }
    //         if frags_encoded > 0 {
    //             Some(packet.freeze())
    //         } else {
    //             None
    //         }
    //     })
    // }

    pub fn read_acks(
        &mut self,
        packet: &mut Bytes,
    ) -> Result<impl Iterator<Item = Seq> + '_, MessageError> {
        // mark this packet as acked;
        // this ack will later be sent out to the peer
        let packet_seq = Seq::decode(packet).map_err(MessageError::ReadPacketSeq)?;
        self.ack.ack(packet_seq);

        // read packet seqs the peer has reported they've acked..
        // ..turn those into message seqs via our mappings..
        // ..perform our internal bookkeeping..
        // ..and return those message seqs to the caller
        let acks = AckHeader::decode(packet).map_err(MessageError::ReadAckHeader)?;
        let iter =
            Self::packet_to_msg_acks(&self.sent_packets, &mut self.unacked_msgs, acks.seqs());
        Ok(iter)
    }

    pub fn read_frags(
        &mut self,
        mut packet: Bytes,
    ) -> impl Iterator<Item = Result<(Bytes, LaneIndex), MessageError>> + '_ {
        let frags = &mut self.frag;
        let lanes = &mut self.lanes;
        std::iter::from_fn(move || {
            // read in all fragments..
            while packet.remaining() > 0 {
                let frag = match Fragment::decode(&mut packet).map_err(MessageError::ReadFragment) {
                    Ok(frag) => frag,
                    Err(err) => return Some(Err(err)),
                };

                // ..ask the lane if it even wants to receive this fragment..
                let lane_index = frag.header.lane_index;
                let lane = match lanes.get_mut(lane_index.into_raw()) {
                    Some(lane) => lane,
                    None => return Some(Err(MessageError::InvalidLaneIndex { lane_index })),
                };
                match lane.on_recv(&frag.header) {
                    LaneRecvResult::Recv => {}
                    LaneRecvResult::Drop => continue,
                };

                // ..and reassemble from the payloads of the fragments
                match frags
                    .reassemble(&frag.header, &frag.payload)
                    .map_err(MessageError::Reassemble)
                {
                    Ok(Some(msg)) => return Some(Ok((Bytes::from(msg), lane_index))),
                    Ok(None) => continue,
                    Err(err) => return Some(Err(err)),
                }
            }
            None
        })
    }

    fn packet_to_msg_acks<'a>(
        sent_packets: &'a AHashMap<Seq, Vec<SentFrag>>,
        unacked_msgs: &'a mut AHashMap<Seq, UnackedMessage>,
        acked_packet_seqs: impl Iterator<Item = Seq> + 'a,
    ) -> impl Iterator<Item = Seq> + 'a {
        acked_packet_seqs
            .filter_map(|acked_packet_seq| sent_packets.get(&acked_packet_seq))
            .flatten()
            .filter_map(|acked_frag| {
                let msg_seq = acked_frag.msg_seq;
                let unacked_msg = unacked_msgs.get_mut(&msg_seq)?;
                if let Some(frag_slot) = unacked_msg
                    .unacked_frags
                    .get_mut(usize::from(acked_frag.frag_id))
                {
                    // mark this frag as acked
                    unacked_msg.frags_remaining -= 1;
                    *frag_slot = None;
                }
                if unacked_msg.frags_remaining == 0 {
                    // message is no longer unacked,
                    // we've just acked all the fragments
                    unacked_msgs.remove(&msg_seq);
                    Some(msg_seq)
                } else {
                    None
                }
            })
    }
}
