use aeronet::{
    lane::OnLane,
    message::{TryFromBytes, TryIntoBytes},
    octs::{EncodeSize, WriteBytes},
};
use ahash::AHashMap;
use bytes::{Bytes, BytesMut};

use crate::{
    frag::{FragHeader, Fragment},
    message::PACKET_HEADER_SIZE,
    seq::Seq,
};

use super::{FragIndex, LaneState, MessageError, Messages, SentMessage};

impl<S: TryIntoBytes + OnLane, R: TryFromBytes> Messages<S, R> {
    pub fn buffer_send(&mut self, msg: S) -> Result<Seq, MessageError<S, R>> {
        let lane_index = msg.lane_index();
        let msg_bytes = msg.try_into_bytes().map_err(MessageError::IntoBytes)?;
        let msg_seq = self.next_send_msg_seq;
        self.next_send_msg_seq += Seq(1);
        let frags = self
            .frags
            .fragment(msg_seq, msg_bytes)
            .map_err(MessageError::Fragment)?;
        self.sent_msgs.insert(
            msg_seq,
            SentMessage {
                lane_index: lane_index.into_raw(),
                num_frags: frags.num_frags(),
                num_unacked: frags.num_frags(),
                frags: frags.map(|frag| Some(frag.payload)).collect(),
            },
        );
        Ok(msg_seq)
    }

    fn sent_frag_payload(&self, index: Option<FragIndex>) -> Option<&Bytes> {
        let index = index?;
        let msg = self.sent_msgs.get(&index.msg_seq)?;
        let frag = msg.frags.get(usize::from(index.frag_id))?;
        frag.as_ref()
    }

    pub fn flush<'a>(&'a mut self, bytes_left: &'a mut usize) -> impl Iterator<Item = Bytes> + '_ {
        // collect all fragments to send
        let mut frags = Self::frags_to_send(&self.sent_msgs)
            .map(Some)
            .collect::<Box<_>>();
        // sort by payload length, largest to smallest
        frags.sort_unstable_by(|a, b| {
            self.sent_frag_payload(*b)
                .map(Bytes::len)
                .cmp(&self.sent_frag_payload(*a).map(Bytes::len))
        });

        std::iter::from_fn(move || {
            let max_packet_bytes = (*bytes_left).min(self.max_packet_size);
            if max_packet_bytes < PACKET_HEADER_SIZE {
                return None;
            }
            let mut packet_bytes_left = max_packet_bytes;

            let packet_seq = self.next_send_packet_seq;
            self.next_send_packet_seq += Seq(1);
            // NOTE: don't use `max_packet_size`, because it might be a really big number
            // e.g. Steamworks already fragments messages, so we don't have to fragment ourselves,
            // so `max_packet_size` is massive
            let mut packet = BytesMut::with_capacity(self.default_packet_cap);
            packet.write(&packet_seq).unwrap();
            packet.write(&self.acks).unwrap();
            packet_bytes_left -= PACKET_HEADER_SIZE;

            let mut frags_in_packet = Vec::new();
            for frag in frags.iter_mut().flat_map(|index_opt| {
                Self::next_frag_in_packet(
                    &mut self.sent_msgs,
                    &mut self.lanes,
                    &mut packet_bytes_left,
                    index_opt,
                )
            }) {
                frags_in_packet.push(FragIndex {
                    msg_seq: frag.header.msg_seq,
                    frag_id: frag.header.frag_id,
                });
                frag.encode_into(&mut packet).unwrap();
            }
            debug_assert!(packet.len() < max_packet_bytes);

            let bytes_used = max_packet_bytes - packet_bytes_left;
            debug_assert!(*bytes_left > bytes_used);
            *bytes_left -= bytes_used;

            if frags_in_packet.is_empty() {
                // we couldn't write any fragments - nothing more to send
                None
            } else {
                // we wrote at least one fragment - we can send this packet
                // and track what fragments we're sending in this packet
                self.flushed_packets
                    .insert(packet_seq, frags_in_packet.into_boxed_slice());
                Some(packet.freeze())
            }
        })
    }

    fn frags_to_send(
        sent_msgs: &AHashMap<Seq, SentMessage>,
    ) -> impl Iterator<Item = FragIndex> + '_ {
        sent_msgs.iter().flat_map(|(msg_seq, msg)| {
            msg.frags
                .iter()
                .filter_map(Option::as_ref)
                .enumerate()
                .map(move |(frag_id, _)| FragIndex {
                    msg_seq: *msg_seq,
                    frag_id: u8::try_from(frag_id).unwrap(),
                })
        })
    }

    fn next_frag_in_packet<'a>(
        sent_msgs: &'a mut AHashMap<Seq, SentMessage>,
        lanes: &'a [LaneState<R>],
        packet_bytes_left: &'a mut usize,
        index_opt: &mut Option<FragIndex>,
    ) -> Option<Fragment<Bytes>> {
        let index = index_opt.take()?;
        // PANIC SAFETY: `frags` is a slice of *unique* frag indices.
        // If we end up removing a frag from `sent_msgs`, then we will
        // also remove the corresponding frag from `frags`.
        // There should be no way for an index in `frags` to point to a
        // frag that we've deleted.
        let msg = sent_msgs
            .get_mut(&index.msg_seq)
            .expect("frag index should point to a valid sent message");
        let payload_opt = msg
            .frags
            .get_mut(usize::from(index.frag_id))
            .expect("frag index should point to a valid fragment in this message");

        // how does the outgoing lane want to handle this fragment?
        let lane = lanes
            .get(msg.lane_index)
            .expect("lane index of message should be in range");
        let payload = if lane.drop_on_flush() {
            payload_opt
                .take()
                .expect("frag index should point to a non-dropped fragment in this message")
        } else {
            payload_opt
                .as_ref()
                .cloned()
                .expect("frag index should point to a non-dropped fragment in this message")
        };

        // compose the fragment
        let frag = Fragment {
            header: FragHeader {
                msg_seq: index.msg_seq,
                num_frags: msg.num_frags,
                frag_id: index.frag_id,
            },
            payload,
        };

        // don't add this frag if it's too big for this packet
        let encode_size = frag.encode_size();
        if encode_size > *packet_bytes_left {
            *index_opt = Some(index);
            *payload_opt = Some(frag.payload);
            return None;
        }
        *packet_bytes_left -= encode_size;

        Some(frag)
    }
}
