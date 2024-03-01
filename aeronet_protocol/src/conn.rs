use std::collections::BinaryHeap;

use ahash::AHashMap;
use bitvec::vec::BitVec;

use crate::{
    ack::{AckHeader, AckReceiver},
    bytes::prelude::*,
    frag::{Fragment, FragmentError, Fragmentation, ReassembleError},
    seq::Seq,
};

#[derive(Debug)]
pub struct Connection {
    frag: Fragmentation,
    next_send_msg_seq: Seq,
    next_send_packet_seq: Seq,
    ack_receiver: AckReceiver,
    send_buf: BinaryHeap<Fragment>,
    sent_msgs: AHashMap<Seq, SentMessage>,
    sent_packets: AHashMap<Seq, Vec<SentFrag>>,
}

#[derive(Debug)]
struct SentMessage {
    acked_frag_ids: BitVec,
}

#[derive(Debug)]
struct SentFrag {
    msg_seq: Seq,
    frag_id: u8,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConnectionError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
    #[error("failed to read ack header")]
    ReadAckHeader(#[source] ReadError),
    #[error("failed to read fragment")]
    ReadFrag(#[source] ReadError),
    #[error("failed to reassemble fragment")]
    Reassemble(#[source] ReassembleError),
}

impl Connection {
    pub fn buffer_send(&mut self, lane_index: usize, msg: Bytes) -> Result<(), ConnectionError> {
        let msg_seq = self.next_send_msg_seq.get_inc();
        self.sent_msgs.insert(msg_seq, SentMessage {});
        self.send_buf.extend(
            self.frag
                .fragment(msg_seq, msg)
                .map_err(ConnectionError::Fragment)?,
        );
        Ok(())
    }

    pub fn flush(&mut self, available_bytes: &mut usize) -> impl Iterator<Item = Box<[u8]>> {
        std::iter::from_fn(|| {})
    }

    pub fn recv(
        &mut self,
        mut packet: Bytes,
    ) -> Result<
        (
            impl Iterator<Item = Seq>,
            impl Iterator<Item = Result<Vec<u8>, ConnectionError>>,
        ),
        ConnectionError,
    > {
        let acks = AckHeader::decode(&mut packet).map_err(ConnectionError::ReadAckHeader)?;
        self.recv_acks(acks);
        Ok(std::iter::from_fn(|| {
            while packet.remaining() > 0 {
                match self.try_next() {
                    Ok(Some(msg)) => return Some(Ok(msg)),
                    Ok(None) => continue,
                    Err(err) => return Some(Err(err)),
                }
            }
            None
        }))
    }

    fn packet_acks_to_msg_acks<'a>(
        &'a mut self,
        acks: impl IntoIterator<Item = Seq> + 'a,
    ) -> impl Iterator<Item = Seq> + 'a {
        let mut acks = acks.into_iter();
        std::iter::from_fn(move || {
            while let Some(acked_packet_seq) = acks.next() {
                let acked_frags = match self.sent_packets.get(&acked_packet_seq) {
                    Some(t) => t,
                    None => continue,
                };
                for acked_frag in acked_frags {
                    let sent_msg = match self.sent_msgs.get_mut(&acked_frag.msg_seq) {
                        Some(t) => t,
                        None => continue,
                    };
                    sent_msg
                        .acked_frag_ids
                        .set(usize::from(acked_frag.frag_id), true);
                    if sent_msg.acked_frag_ids.all() {
                        // this message has been fully reassembled
                        // on the receiver's side
                        // TODO return this without dropping the acked_frags iterator
                    }
                }
            }
            None
        })
    }
}
