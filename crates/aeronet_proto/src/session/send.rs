use std::{collections::hash_map::Entry, iter};

use aeronet::lane::LaneIndex;
use octs::{Bytes, BytesMut, EncodeLen, FixedEncodeLen, Write};
use web_time::{Duration, Instant};

use crate::{
    limit::Limit,
    msg::MessageTooLarge,
    ty::{Fragment, FragmentHeader, MessageSeq, PacketHeader, PacketSeq},
};

use super::{FlushedPacket, FragmentPath, SendLaneKind, SentFragment, SentMessage, Session};

#[derive(Debug, Clone, thiserror::Error)]
pub enum SendError {
    #[error("invalid lane")]
    InvalidLane,
    #[error("too many buffered messages")]
    TooManyMessages,
    #[error(transparent)]
    MessageTooLarge(MessageTooLarge),
}

impl Session {
    pub fn refill_bytes(&mut self, delta_time: Duration) {
        let f = delta_time.as_secs_f32();
        self.bytes_left.refill_portion(f);
    }

    pub fn send(
        &mut self,
        now: Instant,
        msg: impl Into<Bytes>,
        lane: impl Into<LaneIndex>,
    ) -> Result<MessageSeq, SendError> {
        let lane_index =
            usize::try_from(lane.into().into_raw()).map_err(|_| SendError::InvalidLane)?;

        let Some(lane) = self.send_lanes.get_mut(lane_index) else {
            return Err(SendError::InvalidLane);
        };
        let msg_seq = lane.next_msg_seq;
        let Entry::Vacant(entry) = lane.sent_msgs.entry(msg_seq) else {
            return Err(SendError::TooManyMessages);
        };

        let frags = self
            .splitter
            .split(msg)
            .map_err(SendError::MessageTooLarge)?;
        entry.insert(SentMessage {
            frags: frags
                .map(|(marker, payload)| {
                    Some(SentFragment {
                        marker,
                        payload,
                        sent_at: now,
                        next_flush_at: now,
                    })
                })
                .collect(),
        });

        lane.next_msg_seq += MessageSeq::ONE;
        Ok(msg_seq)
    }

    pub fn flush(&mut self, now: Instant) -> impl Iterator<Item = Bytes> + '_ {
        // collect the paths of the frags to send, along with how old they are
        let mut frag_paths = self
            .send_lanes
            .iter_mut()
            .enumerate()
            .flat_map(|(lane_index, lane)| {
                let lane_index = u64::try_from(lane_index).unwrap();
                let lane_index = LaneIndex::from_raw(lane_index);

                // drop any messages which have no frags to send
                lane.sent_msgs
                    .retain(|_, msg| msg.frags.iter().any(Option::is_some));

                // grab the frag paths from this lane's messages
                lane.sent_msgs.iter().flat_map(move |(msg_seq, msg)| {
                    msg.frags
                        .iter()
                        .filter_map(Option::as_ref)
                        .filter(move |frag| now >= frag.next_flush_at)
                        .enumerate()
                        .map(move |(frag_index, frag)| {
                            (
                                FragmentPath {
                                    lane_index,
                                    msg_seq: *msg_seq,
                                    frag_index: u8::try_from(frag_index).unwrap(),
                                },
                                frag.sent_at,
                            )
                        })
                })
            })
            .collect::<Vec<_>>();

        // sort by oldest sent to newest
        frag_paths.sort_unstable_by(|(_, sent_at_a), (_, sent_at_b)| sent_at_a.cmp(sent_at_b));

        let mut frag_paths = frag_paths
            .into_iter()
            .map(|(path, _)| Some(path))
            .collect::<Vec<_>>();

        iter::from_fn(move || {
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
                    seq: packet_seq,
                    acks: self.acks,
                })
                .unwrap();

            // collect the paths of the frags we want to put into this packet
            // so that we can track which ones have been acked later
            let mut packet_frags = Vec::new();
            for frag_path_opt in frag_paths.iter_mut() {
                let res = (|| {
                    let path = frag_path_opt.ok_or(())?;
                    let lane = &mut self
                        .send_lanes
                        .get_mut(usize::try_from(path.lane_index.into_raw()).unwrap())
                        .unwrap();
                    let msg = lane.sent_msgs.get_mut(&path.msg_seq).unwrap();
                    let mut sent_frag = msg.frags.get_mut(usize::from(path.frag_index));
                    let sent_frag = sent_frag.as_mut().unwrap().as_mut().unwrap();

                    let frag = Fragment {
                        header: FragmentHeader {
                            lane_index: path.lane_index,
                            msg_seq: path.msg_seq,
                            marker: sent_frag.marker,
                        },
                        payload: sent_frag.payload.clone(),
                    };
                    bytes_left.consume(frag.encode_len()).map_err(|_| ())?;
                    packet.write(frag).unwrap();

                    // what does the lane do with this after sending?
                    match &lane.kind {
                        SendLaneKind::Unreliable => {
                            // drop the frag
                            // if we've dropped all frags of this message, then
                            // on the next `flush`, we'll drop the message
                            *frag_path_opt = None;
                        }
                        SendLaneKind::Reliable => {
                            // don't drop the frag, just attempt to resend it later
                            // it'll be dropped when the peer acks it
                            sent_frag.next_flush_at = now + self.rtt.pto();
                        }
                    }

                    packet_frags.push(path);
                    Ok::<_, ()>(())
                })();

                // if we successfully wrote this frag out,
                // remove it from the candidate frag paths
                if res.is_ok() {
                    *frag_path_opt = None;
                }
            }

            let send_keep_alive = now >= self.next_keep_alive_at;
            if packet_frags.is_empty() && !send_keep_alive {
                None
            } else {
                self.flushed_packets.insert(
                    packet_seq,
                    FlushedPacket {
                        flushed_at: now,
                        frags: packet_frags.into_boxed_slice(),
                    },
                );

                let packet = packet.freeze();
                self.bytes_sent = self.bytes_sent.saturating_add(packet.len());
                // TODO keepalive
                // without this placeholder code we loop infinitely
                self.next_keep_alive_at = now + Duration::from_millis(100);
                // END
                self.next_packet_seq += PacketSeq::ONE;
                Some(packet)
            }
        })
    }
}
