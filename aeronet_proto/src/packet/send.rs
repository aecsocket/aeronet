use aeronet::{
    lane::LaneMapper,
    message::BytesMapper,
    octs::{EncodeLen, WriteBytes},
};
use ahash::AHashMap;
use bytes::{Bytes, BytesMut};

use crate::{
    frag::{FragHeader, Fragment},
    packet::PACKET_HEADER_LEN,
    seq::Seq,
};

use super::{lane::LaneSend, FragIndex, Packets, SendError, SentMessage};

impl<S, R, M> Packets<S, R, M>
where
    M: BytesMapper<S> + BytesMapper<R> + LaneMapper<S> + LaneMapper<R>,
{
    /// Buffers up a message for sending.
    ///
    /// This message will be stored until the next [`Packets::flush`] call.
    ///
    /// # Errors
    ///
    /// Errors if it could not buffer this message for sending.
    pub fn buffer_send(
        &mut self,
        msg: S,
    ) -> Result<Seq, SendError<<M as BytesMapper<S>>::IntoError>> {
        let lane_index = self.mapper.lane_index(&msg);
        let msg_bytes = self
            .mapper
            .try_into_bytes(msg)
            .map_err(SendError::IntoBytes)?;
        let msg_seq = self.next_send_msg_seq;
        let frags = self
            .frags
            .fragment(msg_seq, msg_bytes)
            .map_err(SendError::Fragment)?;
        // only increment the seq after successfully fragmenting
        self.next_send_msg_seq += Seq(1);

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
            let max_packet_bytes = (*bytes_left).min(self.max_packet_len);
            if max_packet_bytes < PACKET_HEADER_LEN {
                return None;
            }
            let mut packet_bytes_left = max_packet_bytes;

            let packet_seq = self.next_send_packet_seq;
            self.next_send_packet_seq += Seq(1);
            // NOTE: don't use `max_packet_size`, because it might be a really big number
            // e.g. Steamworks already fragments messages, so we don't have to fragment
            // ourselves, so `max_packet_size` is massive,
            // but we don't want to allocate a 512KiB buffer
            let mut packet = BytesMut::with_capacity(self.default_packet_cap);
            packet.write(&packet_seq).unwrap();
            packet.write(&self.acks).unwrap();
            packet_bytes_left -= PACKET_HEADER_LEN;
            debug_assert_eq!(packet.len(), PACKET_HEADER_LEN);

            let mut frags_in_packet = Vec::new();
            for frag in frags.iter_mut().filter_map(|index_opt| {
                Self::next_frag_in_packet(
                    &mut self.sent_msgs,
                    &self.lanes_out,
                    &mut packet_bytes_left,
                    index_opt,
                )
            }) {
                frags_in_packet.push(FragIndex {
                    msg_seq: frag.header.msg_seq,
                    frag_id: frag.header.frag_id,
                });
                let orig_len = packet.len();
                let encode_len = frag.encode_len();
                frag.encode_into(&mut packet).unwrap();
                debug_assert_eq!(orig_len + encode_len, packet.len());
            }
            let bytes_used = max_packet_bytes - packet_bytes_left;
            debug_assert!(packet.len() <= max_packet_bytes);
            debug_assert_eq!(packet.len(), bytes_used);

            if frags_in_packet.is_empty() {
                // we couldn't write any fragments - nothing more to send
                None
            } else {
                // we wrote at least one fragment - we can send this packet
                // and track what fragments we're sending in this packet
                *bytes_left -= bytes_used;
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
        lanes_out: &'a [LaneSend],
        packet_bytes_left: &'a mut usize,
        index_opt: &mut Option<FragIndex>,
    ) -> Option<Fragment<Bytes>> {
        let index = index_opt.take()?;
        // CORRECTNESS: `frags` is a slice of *unique* frag indices.
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
        let lane = lanes_out
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
        let encode_len = frag.encode_len();
        if encode_len > *packet_bytes_left {
            *index_opt = Some(index);
            *payload_opt = Some(frag.payload);
            return None;
        }
        *packet_bytes_left -= encode_len;

        Some(frag)
    }
}
