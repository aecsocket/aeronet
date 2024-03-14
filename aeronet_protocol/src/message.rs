use aeronet::lane::{LaneConfig, LaneIndex, LaneKind, LaneReliability};
use ahash::AHashMap;
use bytes::{Buf, Bytes, BytesMut};
use derivative::Derivative;

use crate::{
    ack::{AckHeader, Acknowledge},
    bytes::BytesError,
    frag::{FragHeader, Fragment, FragmentError, Fragmentation, ReassembleError},
    seq::Seq,
};

/*

problem:
* when sending a frag, we need to add it to a vec of outgoing frags
* we need this vec to be sorted before `flush`
  * on insertion?
  * right before the `flush` logic?
* in `flush`, we need to send out all frags which haven't been acked yet
  * if the frag is sent unreliably, no problem, just remove it immediately after
  * if the frag is sent reliably, we keep it in the send buffer, but how/when do
    we remove it?
* when receiving a packet ack, we map it to a (msg_seq, frag_id) -
  we need to then somehow stop `flush` from sending out this frag anymore
  * removing from a map or something?

solution 1:
* fields:
  * send_buf: Vec<SendFrag>
  * acked_frags: AHashSet<(msg_seq, frag_id)>
* on `recv_acks`, add all acked (msg_seq, frag_id) pairs to `acked_frags`
* on `flush`, when we iterate through all `SendFrag`s;
  * if the frag is in `recv_acks`, remove it from both the `send_buf` and
    `acked_frags`, and don't send it
  * PROBLEM: a frag might have been already removed (unreliable frag), so flush
    will never find it, and it will never be removed from `acked_frags`,
    leaking memory
  * MAYBE: clear old entries in `acked_frags`? but that feels hacky

solution 2:
* fields
  * send_buf: AHashMap<Seq, Vec<SentFrag>>
* on `buffer_send`, add the frag to `send_buf`
* on `flush`, iterate thru `send_buf`, get refs to all the frags, and sort it
  by the biggest frags
* on `recv_acks`, remove the (msg_seq, frag_id) pair from `send_buf`
* I like this solution right now

*/

#[derive(Debug)]
pub struct Messages {
    // stores current state of lanes, allowing them to influence packet sending
    // and receiving
    lanes: Vec<LaneState>,
    // maximum byte length of a single packet produced by `flush`
    max_packet_len: usize,
    // allows breaking a message into fragments, and buffers received fragments
    // to reassemble them into messages
    frag: Fragmentation,
    // tracks which packet seqs have been received
    ack: Acknowledge,
    // seq number of the next packet sent out in `flush`
    next_send_packet_seq: Seq,
    // seq number of the next message buffered in `buffer_send`
    next_send_msg_seq: Seq,
    //
    send_msg_buf: AHashMap<Seq, SendMessage>,
    // tracks which packets have been sent out, and what frags they contained
    // so that when we receive an ack for that packet, we know what frags have
    // been acked, and therefore what messages have been acked
    sent_packets: AHashMap<Seq, Vec<SentFrag>>,
}

#[derive(Debug)]
struct SendMessage {}

#[derive(Derivative, Debug, Clone)]
#[derivative(PartialEq, Eq, PartialOrd, Ord)]
struct SendFrag {
    frag: Fragment,
    #[derivative(PartialEq = "ignore", PartialOrd = "ignore", Ord = "ignore")]
    reliability: LaneReliability,
}

#[derive(Debug)]
struct SentFrag {
    msg_seq: Seq,
    frag_id: u8,
}

#[derive(Debug)]
enum LaneState {
    UnreliableUnordered,
    UnreliableSequenced { last_recv_msg_seq: Seq },
    ReliableUnordered,
    ReliableSequenced { last_recv_msg_seq: Seq },
    ReliableOrdered,
}

impl LaneState {
    pub fn new(kind: LaneKind) -> Self {
        match kind {
            LaneKind::UnreliableUnordered => Self::UnreliableUnordered,
            LaneKind::UnreliableSequenced => Self::UnreliableSequenced {
                last_recv_msg_seq: Seq(0),
            },
            LaneKind::ReliableUnordered => Self::ReliableUnordered,
            LaneKind::ReliableSequenced => Self::ReliableSequenced {
                last_recv_msg_seq: Seq(0),
            },
            LaneKind::ReliableOrdered => Self::ReliableOrdered,
        }
    }

    pub fn kind(&self) -> LaneKind {
        match self {
            Self::UnreliableUnordered => LaneKind::UnreliableUnordered,
            Self::UnreliableSequenced { .. } => LaneKind::UnreliableSequenced,
            Self::ReliableUnordered => LaneKind::ReliableUnordered,
            Self::ReliableSequenced { .. } => LaneKind::ReliableSequenced,
            Self::ReliableOrdered => LaneKind::ReliableOrdered,
        }
    }

    pub fn on_recv(&mut self, frag: &FragHeader) -> LaneRecvResult {
        match self {
            Self::UnreliableUnordered | Self::ReliableUnordered => LaneRecvResult::Recv,
            Self::UnreliableSequenced { last_recv_msg_seq }
            | Self::ReliableSequenced { last_recv_msg_seq } => {
                if frag.msg_seq < *last_recv_msg_seq {
                    LaneRecvResult::Drop
                } else {
                    *last_recv_msg_seq = frag.msg_seq;
                    LaneRecvResult::Recv
                }
            }
            Self::ReliableOrdered => todo!(),
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
            lanes: lanes
                .into_iter()
                .map(|config| LaneState::new(config.kind))
                .collect(),
            max_packet_len,
            frag: Fragmentation::new(max_packet_len - PACKET_HEADER_LEN),
            ack: Acknowledge::new(),
            next_send_msg_seq: Seq(0),
            next_send_packet_seq: Seq(0),
            send_buf: Vec::new(),
            sent_packets: AHashMap::new(),
        }
    }

    pub fn buffer_send(&mut self, lane_index: LaneIndex, msg: Bytes) -> Result<Seq, MessageError> {
        let msg_seq = self.next_send_msg_seq.get_inc();
        let lane = &self.lanes[lane_index.into_raw()];
        let frags = self
            .frag
            .fragment(msg_seq, lane_index, msg)
            .map_err(MessageError::Fragment)?;
        self.send_buf.extend(frags.map(|frag| SendFrag {
            frag,
            reliability: lane.kind().reliability(),
        }));

        // self.abc_send_buf.insert(
        //     msg_seq,
        //     SendMessage {
        //         frags_remaining: frags.num_frags(),
        //         unacked_frags: frags.map(Some).collect(),
        //     },
        // );
        Ok(msg_seq)
    }

    pub fn flush<'a>(
        &'a mut self,
        available_bytes: &'a mut usize,
    ) -> impl Iterator<Item = Bytes> + 'a {
        // sort `send_buf` from largest to smallest, used by `next_frags_in_packet`
        self.send_buf.sort_unstable_by(|a, b| b.cmp(a));

        std::iter::from_fn(move || {
            if *available_bytes < PACKET_HEADER_LEN {
                return None;
            }

            let packet_seq = self.next_send_packet_seq.get_inc();
            let mut packet = BytesMut::with_capacity(self.max_packet_len);
            // PANIC SAFETY: `max_packet_len > PACKET_HEADER_LEN` is asserted on construction
            // and encoding these values takes `PACKET_HEADER_LEN` bytes
            packet_seq.encode(&mut packet).unwrap();
            self.ack.header().encode(&mut packet).unwrap();
            *available_bytes -= PACKET_HEADER_LEN;

            let available_bytes_for_frags = (*available_bytes).min(self.max_packet_len);
            let mut available_bytes_for_frags_after = available_bytes_for_frags;
            let mut sent_frags = Vec::new();
            for frag in self.next_frags_in_packet(&mut available_bytes_for_frags_after) {
                frag.encode(&mut packet);
                sent_frags.push(SentFrag {
                    msg_seq: frag.header.msg_seq,
                    frag_id: frag.header.frag_id,
                });
            }
            *available_bytes -= available_bytes_for_frags - available_bytes_for_frags_after;

            // we've fully built the packet that we're about to send out;
            // track its packet sequence, and what frags it contained
            // so that when we receive an ack for this packet, we know what frags
            // have been acked, and therefore what messages have been acked
            self.sent_packets.insert(packet_seq, sent_frags);

            Some(packet.freeze())
        })
    }

    fn next_frags_in_packet<'a>(
        &'a mut self,
        available_bytes: &'a mut usize,
    ) -> impl Iterator<Item = Fragment> + 'a {
        // this will have been sorted by `flush` beforehand
        let send_buf = &mut self.send_buf;
        let mut i = 0;
        std::iter::from_fn(move || {
            // TODO `extract_if`
            while i < send_buf.len() {
                if send_buf[i].frag.encode_size() < *available_bytes {
                    // skip this fragment, try to find the next smallest frag
                    i += 1;
                    continue;
                }

                let frag = send_buf.remove(i);
                match frag.reliability {
                    LaneReliability::Unreliable => {
                        // discard this fragment; we won't ever send it again
                    }
                    LaneReliability::Reliable => {
                        // keep this fragment around, we might need to resend it later
                        send_buf.push(frag.clone());
                    }
                }
                *available_bytes -= frag.frag.encode_size();
                return Some(frag.frag);
            }
            None
        })
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
        // this ack will later be sent out to the peer in `flush`
        let packet_seq = Seq::decode(packet).map_err(MessageError::ReadPacketSeq)?;
        self.ack.ack(packet_seq);

        // read packet seqs the peer has reported they've acked..
        // ..turn those into message seqs via our mappings..
        // ..perform our internal bookkeeping..
        // ..and return those message seqs to the caller
        let acks = AckHeader::decode(packet).map_err(MessageError::ReadAckHeader)?;
        let iter =
            Self::packet_to_msg_acks(&self.sent_packets, &mut self.abc_send_buf, acks.seqs());
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
        unacked_msgs: &'a mut AHashMap<Seq, SendMessage>,
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
