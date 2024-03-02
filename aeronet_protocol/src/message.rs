use std::collections::BinaryHeap;

use ahash::AHashMap;
use bitvec::{bitvec, vec::BitVec};

use crate::{
    ack::{AckHeader, Acknowledge},
    bytes::prelude::*,
    frag::{Fragment, FragmentError, Fragmentation, ReassembleError},
    seq::Seq,
};

#[derive(Debug)]
pub struct Messages {
    frag: Fragmentation,
    next_send_msg_seq: Seq,
    next_send_packet_seq: Seq,
    ack: Acknowledge,
    send_buf: BinaryHeap<Fragment>,
    unacked_msgs: AHashMap<Seq, UnackedMessage>,
    sent_packets: AHashMap<Seq, Vec<SentFrag>>,
}

#[derive(Debug)]
struct UnackedMessage {
    acked_frag_ids: BitVec,
}

#[derive(Debug)]
struct SentFrag {
    msg_seq: Seq,
    frag_id: u8,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum MessageError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),

    #[error("failed to read packet seq")]
    ReadPacketSeq(#[source] ReadError),
    #[error("failed to read ack header")]
    ReadAckHeader(#[source] ReadError),
    #[error("failed to read fragment")]
    ReadFragment(#[source] ReadError),
    #[error("failed to reassemble fragment")]
    Reassemble(#[source] ReassembleError),
}

impl Messages {
    pub fn buffer_send(&mut self, lane_index: usize, msg: Bytes) -> Result<Seq, MessageError> {
        let msg_seq = self.next_send_msg_seq.get_inc();
        let frags = self
            .frag
            .fragment(msg_seq, msg)
            .map_err(MessageError::Fragment)?;
        self.unacked_msgs.insert(
            msg_seq,
            UnackedMessage {
                acked_frag_ids: bitvec![0; frags.len()],
            },
        );
        self.send_buf.extend(frags);
        Ok(msg_seq)
    }

    pub fn flush(&mut self, available_bytes: &mut usize) -> impl Iterator<Item = Box<[u8]>> {
        std::iter::from_fn(|| todo!())
    }

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
        Ok(iter.map(|msg_seq| {
            // TODO notify lanes
            msg_seq
        }))
    }

    pub fn read_frags(
        &mut self,
        mut packet: Bytes,
    ) -> impl Iterator<Item = Result<Bytes, MessageError>> + '_ {
        let frags = &mut self.frag;
        std::iter::from_fn(move || {
            // read in all fragments..
            while packet.remaining() > 0 {
                let frag = match Fragment::decode(&mut packet).map_err(MessageError::ReadFragment) {
                    Ok(frag) => frag,
                    Err(err) => return Some(Err(err)),
                };
                // ..and reassemble from the payloads of the fragments
                match frags
                    .reassemble(&frag.header, &frag.payload)
                    .map_err(MessageError::Reassemble)
                {
                    Ok(Some(msg)) => return Some(Ok(Bytes::from(msg))),
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
                unacked_msg
                    .acked_frag_ids
                    .set(usize::from(acked_frag.frag_id), true);
                if unacked_msg.acked_frag_ids.all() {
                    // it's no longer unacked,
                    // we've just acked all the fragments
                    unacked_msgs.remove(&msg_seq);
                    // notifying lanes is left as as responsibility
                    // of the caller
                    Some(msg_seq)
                } else {
                    None
                }
            })
    }
}
