use std::collections::hash_map::Entry;

use aeronet::lane::LaneIndex;
use ahash::AHashMap;
use octs::{Bytes, BytesMut, EncodeLen, FixedEncodeLen, VarInt, Write};
use web_time::{Duration, Instant};

use crate::{
    byte_count::ByteLimit,
    packet::{MessageSeq, PacketHeader, PacketSeq},
};

use super::{
    FlushedPacket, FragmentPath, SendError, SendLaneKind, SentFragment, SentMessage, Session,
};

impl Session {
    /// Refills the amount of bytes left for sending out data, given a time
    /// delta since the last `refill_bytes` call.
    ///
    /// See [`SessionConfig::send_bytes_per_sec`].
    ///
    /// [`SessionConfig::send_bytes_per_sec`]: crate::session::SessionConfig::send_bytes_per_sec
    pub fn refill_bytes(&mut self, delta_time: Duration) {
        let f = delta_time.as_secs_f32();
        self.bytes_left.refill_portion(f);
        for lane in self.send_lanes.iter_mut() {
            lane.bytes_left.refill_portion(f);
        }
    }

    /// Buffers up a message for sending.
    ///
    /// After a message has been buffered for sending, it is considered *sent*
    /// but not *flushed*. Use [`Session::flush`] to build up the packets to
    /// send to the peer.
    ///
    /// # Errors
    ///
    /// Errors if the message or lane index were invalid in some way.
    ///
    /// It is safe to ignore this error, however if this occurs when sending a
    /// [reliable] message, a higher-level component in the stack may terminate
    /// the connection.
    ///
    /// [reliable]: aeronet::lane::LaneReliability::Reliable
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
    pub fn send(
        &mut self,
        now: Instant,
        msg: &[u8],
        lane_index: LaneIndex,
    ) -> Result<MessageSeq, SendError> {
        if self.send_lanes.get(lane_index.into_raw()).is_none() {
            return Err(SendError::InvalidLane);
        }
        let msg_seq = self.next_msg_seq;
        let Entry::Vacant(entry) = self.sent_msgs.entry(msg_seq) else {
            return Err(SendError::TooManyMessages);
        };

        // encode the lane index directly into the start of the message payload
        let lane_index_enc = VarInt(lane_index.into_raw());
        let mut buf = BytesMut::with_capacity(lane_index_enc.encode_len() + msg.len());
        buf.write(lane_index_enc).unwrap();
        buf.write_from(msg).unwrap();
        let buf = buf.freeze();

        let frags = self.send_frags.fragment(msg_seq, buf)?;

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

    /// Builds up packets to send to the peer.
    ///
    /// Each [`Bytes`] packet returned must be sent along the connection,
    /// however it is OK if some packets are dropped or duplicated.
    ///
    /// This should be run at the end of each update.
    #[allow(clippy::missing_panics_doc)] // shouldn't panic
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
            .collect::<Vec<_>>();

        // // sort them by payload length, largest to smallest
        // frag_paths.sort_unstable_by(|a, b| {
        //     let a = Self::get_frag(&self.sent_msgs, a.unwrap());
        //     let b = Self::get_frag(&self.sent_msgs, b.unwrap());
        //     b.frag.payload.len().cmp(&a.frag.payload.len())
        // });

        // sort them by oldest to newest
        frag_paths.sort_unstable_by(|a, b| {
            let dist_a = a.msg_seq.dist_to(*self.next_msg_seq);
            let dist_b = a.msg_seq.dist_to(*self.next_msg_seq);
            dist_a.cmp(&dist_b)
        });

        tracing::info!(
            "!! sending out {:?}",
            frag_paths
                .iter()
                .map(|path| path.msg_seq)
                .collect::<Vec<_>>()
        );

        let mut frag_paths = frag_paths.into_iter().map(Some).collect::<Vec<_>>();

        std::iter::from_fn(move || {
            // this iteration, we want to build up one full packet

            // make a buffer for the packet
            // note: we may want to preallocate some memory for this,
            // and have it be user-configurable, but I don't want to overcomplicate
            // the SessionConfig.
            // also, we don't preallocate `mtu` bytes, because that might be a big length
            // e.g. Steamworks already fragments messages, so we don't fragment messages
            // ourselves, leading to very large `mtu`s (~512KiB)
            let mut packet = BytesMut::new();

            // we can't put more than either `mtu` or `bytes_left`
            // bytes into this packet, so we track this as well
            let mut bytes_left = (&mut self.bytes_left).min_of(self.mtu);
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
                    // but I would rather take the slightly higher memory usage here, because recreating the header
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

            let should_send_keep_alive = false; //now >= self.next_keep_alive_at;
            if frags.is_empty() && !should_send_keep_alive {
                None
            } else {
                self.next_packet_seq += PacketSeq::new(1);

                // track what fragments we're sending in this packet
                self.flushed_packets.insert(
                    packet_seq,
                    FlushedPacket {
                        flushed_at: now,
                        frags: frags.into_boxed_slice(),
                    },
                );
                let packet = packet.freeze();

                self.bytes_sent = self.bytes_sent.saturating_add(packet.len());
                // instead of having the keep-alive interval be user-configurable,
                // it's based on the RTT of the connection
                // TODO: is this a bad idea?
                self.next_keep_alive_at = now + self.rtt.get();

                Some(packet)
            }
        })
    }
}
