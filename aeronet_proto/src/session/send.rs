use std::collections::hash_map::Entry;

use aeronet::lane::LaneIndex;
use ahash::AHashMap;
use octs::{Bytes, BytesMut, EncodeLen, FixedEncodeLen, VarInt, Write};
use web_time::Instant;

use crate::{
    byte_count::{ByteBucket, ByteLimit},
    packet::{MessageSeq, PacketHeader, PacketSeq},
};

use super::{
    FlushedPacket, FragmentPath, SendError, SendLaneKind, SentFragment, SentMessage, Session,
};

impl Session {
    #[must_use]
    pub const fn bytes_left(&self) -> &ByteBucket {
        &self.bytes_left
    }

    pub fn refill_bytes_exact(&mut self, n: usize) {
        self.bytes_left.refill_exact(n);
        for lane in self.send_lanes.iter_mut() {
            lane.bytes_left.refill_exact(n)
        }
    }

    pub fn refill_bytes_portion(&mut self, f: f32) {
        self.bytes_left.refill_portion(f);
        for lane in self.send_lanes.iter_mut() {
            lane.bytes_left.refill_portion(f)
        }
    }

    pub fn send(
        &mut self,
        now: Instant,
        msg: &[u8],
        lane_index: LaneIndex,
    ) -> Result<MessageSeq, SendError> {
        if self.send_lanes.get(lane_index.into_raw()).is_none() {
            return Err(SendError::InvalidLane);
        }

        // encode the lane index directly into the start of the message payload
        let lane_index_enc = VarInt(lane_index.into_raw());
        let mut buf = BytesMut::with_capacity(lane_index_enc.encode_len() + msg.len());
        buf.write(lane_index_enc).unwrap();
        buf.write_from(msg).unwrap();
        let buf = buf.freeze();

        let msg_seq = self.next_msg_seq;
        let frags = self.send_frags.fragment(msg_seq, buf)?;

        let Entry::Vacant(entry) = self.sent_msgs.entry(msg_seq) else {
            return Err(SendError::TooManyMessages);
        };
        self.next_msg_seq += MessageSeq::new(1);
        entry.insert(SentMessage {
            lane_index,
            frags: frags
                .map(|frag| {
                    Some(SentFragment {
                        frag,
                        next_flush_at: now,
                    })
                })
                .collect(),
        });
        Ok(msg_seq)
    }

    fn get_frag(
        sent_msgs: &AHashMap<MessageSeq, SentMessage>,
        path: FragmentPath,
    ) -> &SentFragment {
        sent_msgs[&path.msg_seq].frags[usize::from(path.index)]
            .as_ref()
            .unwrap()
    }

    pub fn flush(&mut self, now: Instant) -> impl Iterator<Item = Bytes> + '_ {
        // drop any messages which have no frags to send
        self.sent_msgs
            .retain(|_, msg| msg.frags.iter().any(Option::is_some));

        // collect the paths of the fragments to send
        let mut frag_paths = self
            .sent_msgs
            .iter()
            .flat_map(move |(msg_seq, msg)| {
                msg.frags
                    .iter()
                    .filter_map(Option::as_ref)
                    .filter(move |frag| now >= frag.next_flush_at)
                    .enumerate()
                    .map(move |(frag_index, _)| FragmentPath {
                        msg_seq: *msg_seq,
                        index: u8::try_from(frag_index).unwrap(),
                    })
            })
            // wrap in an Option, since we're gonna be taking individual frags out
            // once we've added them to a packet
            .map(Some)
            .collect::<Box<_>>();
        // sort them by payload length, largest to smallest
        frag_paths.sort_unstable_by(|a, b| {
            let a = Self::get_frag(&self.sent_msgs, a.unwrap());
            let b = Self::get_frag(&self.sent_msgs, b.unwrap());
            b.frag.payload.len().cmp(&a.frag.payload.len())
        });

        std::iter::from_fn(move || {
            // this iteration, we want to build up one full packet

            // make a buffer for the packet
            // NOTE: we don't use `max_packet_len`, because that might be a big length
            // e.g. Steamworks already fragments messages, so we don't fragment messages
            // ourselves, leading to very large `max_packet_len`s (~512KiB)
            let mut packet = BytesMut::with_capacity(self.default_packet_cap);

            // we can't put more than either `max_packet_len` or `bytes_left`
            // bytes into this packet, so we track this as well
            let mut bytes_left = (&mut self.bytes_left).min_of(self.max_packet_len);
            let packet_seq = self.next_packet_seq;
            bytes_left.consume(PacketHeader::ENCODE_LEN).ok()?;
            packet
                .write(PacketHeader {
                    packet_seq,
                    acks: self.acks,
                })
                .unwrap();

            // collect the paths of the frags we want to put into this packet
            // so that we can track which ones have been acked later
            let mut frags = Vec::new();
            for frag_path_opt in frag_paths.iter_mut() {
                let Some(path) = frag_path_opt.take() else {
                    continue;
                };

                let res = (|| {
                    let msg = self.sent_msgs.get_mut(&path.msg_seq).unwrap();
                    let sent_frag = msg.frags[usize::from(path.index)].as_mut().unwrap();

                    // in theory, we can just store the fragment payload instead of header + payload
                    // and then here, we recreate the header, since we theoretically have the info to do it
                    // but I would rather take the slightly higher memory usage here, because reforming the header
                    // is error-prone - it's basically a FragmentSender impl detail
                    let frag = &sent_frag.frag;

                    // write the payload into the packet
                    // make sure we have enough bytes available in the bucket first though
                    // the lane index is encoded in `sent_frag.payload` itself, done in `send`
                    let lane = &mut self.send_lanes[msg.lane_index.into_raw()];
                    let mut bytes_left = (&mut bytes_left).min_of(&mut lane.bytes_left);
                    bytes_left.consume(frag.encode_len()).map_err(|_| ())?;
                    packet.write(frag).unwrap();

                    // how does the lane want to handle this?
                    match &lane.kind {
                        SendLaneKind::Unreliable => {
                            // drop the frag
                            // if we've dropped all frags of this message, then
                            // on the next `flush`, we'll drop the message
                            *frag_path_opt = None;
                        }
                        SendLaneKind::Reliable { resend_after } => {
                            // don't drop the frag, just attempt to resend it later
                            // it'll be dropped when the peer acks it
                            sent_frag.next_flush_at = now + *resend_after;
                        }
                    }

                    frags.push(path);
                    Ok::<_, ()>(())
                })();

                if res.is_err() {
                    // if we failed to write this frag, then replace it back
                    *frag_path_opt = Some(path);
                }
            }

            if frags.is_empty() {
                // we couldn't write any fragments - no more packets to send
                None
            } else {
                // we wrote at least one fragment - we can send this packet
                // and track what fragments we're sending in this packet
                self.next_packet_seq += PacketSeq::new(1);
                self.flushed_packets.insert(
                    packet_seq,
                    FlushedPacket {
                        frags: frags.into_boxed_slice(),
                    },
                );
                Some(packet.freeze())
            }
        })
    }
}
