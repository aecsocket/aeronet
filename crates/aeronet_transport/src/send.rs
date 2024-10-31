use std::{collections::hash_map::Entry, iter};

use aeronet_io::packet::{PacketBuffers, PacketMtu};
use ahash::HashMap;
use arbitrary::Arbitrary;
use bevy_ecs::prelude::*;
use octs::{Bytes, EncodeLen, FixedEncodeLen, Write};
use tracing::{trace, trace_span};
use typesize::derive::TypeSize;
use web_time::Instant;

use crate::{
    frag,
    lane::{LaneIndex, LaneKind, LaneReliability},
    limit::Limit,
    packet::{
        FragmentPayload, FragmentPosition, MessageFragment, MessageSeq, PacketHeader, PacketSeq,
    },
    rtt::RttEstimator,
    sized, FlushedPacket, FragmentPath, Transport,
};

#[derive(Debug, TypeSize)]
pub struct TransportSend {
    max_frag_len: usize,
    lanes: Box<[Lane]>,
    too_many_msgs: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Arbitrary, TypeSize)]
pub struct MessageKey {
    lane: LaneIndex,
    seq: MessageSeq,
}

#[derive(Debug, Clone, TypeSize)]
struct Lane {
    sent_msgs: HashMap<MessageSeq, SentMessage>,
    next_msg_seq: MessageSeq,
    reliability: LaneReliability,
}

#[derive(Debug, Clone, TypeSize)]
struct SentMessage {
    frags: Box<[Option<SentFragment>]>,
}

#[derive(Debug, Clone, TypeSize)]
struct SentFragment {
    position: FragmentPosition,
    payload: sized::Bytes,
    sent_at: sized::Instant,
    next_flush_at: sized::Instant,
}

impl TransportSend {
    pub(crate) fn new(
        max_frag_len: usize,
        lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
    ) -> Self {
        Self {
            max_frag_len,
            lanes: lanes
                .into_iter()
                .map(Into::into)
                .map(|kind| Lane {
                    sent_msgs: HashMap::default(),
                    next_msg_seq: MessageSeq::default(),
                    reliability: kind.reliability(),
                })
                .collect(),
            too_many_msgs: false,
        }
    }

    pub fn push(&mut self, now: Instant, lane_index: LaneIndex, msg: Bytes) -> Option<MessageKey> {
        let lane_index_u = lane_index.into_usize();
        let lane = &mut self.lanes[lane_index_u];
        let msg_seq = lane.next_msg_seq;
        let Entry::Vacant(entry) = lane.sent_msgs.entry(msg_seq) else {
            self.too_many_msgs = true;
            return None;
        };

        let frags = frag::split(self.max_frag_len, msg);
        entry.insert(SentMessage {
            frags: frags
                .map(|(position, payload)| {
                    Some(SentFragment {
                        position,
                        payload: sized::Bytes(payload),
                        sent_at: sized::Instant(now),
                        next_flush_at: sized::Instant(now),
                    })
                })
                .collect(),
        });

        lane.next_msg_seq += MessageSeq::new(1);
        Some(MessageKey {
            lane: lane_index,
            seq: msg_seq,
        })
    }
}

impl Transport {
    fn flush(&mut self, now: Instant, mtu: usize) -> impl Iterator<Item = Vec<u8>> + '_ {
        // collect the paths of the frags to send, along with how old they are
        let mut frag_paths = self
            .send
            .lanes
            .iter_mut()
            .enumerate()
            .flat_map(|(lane_index, lane)| frag_paths_in_lane(now, lane_index, lane))
            .collect::<Vec<_>>();

        // sort by time sent, oldest to newest
        frag_paths.sort_unstable_by(|(_, sent_at_a), (_, sent_at_b)| sent_at_a.cmp(sent_at_b));

        let mut frag_paths = frag_paths
            .into_iter()
            .map(|(path, _)| Some(path))
            .collect::<Vec<_>>();

        let mut sent_packet_yet = false;
        iter::from_fn(move || {
            // this iteration, we want to build up one full packet

            // make a buffer for the packet
            // note: we may want to preallocate some memory for this,
            // and have it be user-configurable, but I don't want to overcomplicate it
            // also, we don't preallocate `mtu` bytes, because that might be a big length
            // e.g. Steamworks already fragments messages, so we don't fragment messages
            // ourselves, leading to very large `mtu`s (~512KiB)
            let mut packet = Vec::<u8>::new();

            // we can't put more than either `mtu` or `bytes_left`
            // bytes into this packet, so we track this as well
            let mut bytes_left = (&mut self.bytes_left).min_of(mtu);
            let packet_seq = self.next_packet_seq;
            bytes_left.consume(PacketHeader::ENCODE_LEN).ok()?;
            packet
                .write(PacketHeader {
                    seq: packet_seq,
                    acks: self.acks,
                })
                .expect("should grow the buffer when writing over capacity");

            let span = trace_span!("flush", packet = packet_seq.0 .0);
            let _span = span.enter();

            // collect the paths of the frags we want to put into this packet
            // so that we can track which ones have been acked later
            let mut packet_frags = Vec::new();
            for path_opt in &mut frag_paths {
                let Some(path) = path_opt else {
                    continue;
                };
                let path = *path;

                if write_frag_at_path(
                    now,
                    &self.rtt,
                    &mut self.send.lanes,
                    &mut bytes_left,
                    &mut packet,
                    path,
                )
                .is_ok()
                {
                    // if we successfully wrote this frag out,
                    // remove it from the candidate frag paths
                    // and track that this frag has been sent out in this packet
                    *path_opt = None;
                    packet_frags.push(path);
                }
            }

            let send_empty = !sent_packet_yet; // TODO //&& now >= self.next_ack_at;
            let should_send = !packet_frags.is_empty() || send_empty;
            if !should_send {
                return None;
            }

            trace!(num_frags = packet_frags.len(), "Flushed packet");
            self.flushed_packets.insert(
                packet_seq.0 .0,
                FlushedPacket {
                    flushed_at: sized::Instant(now),
                    frags: packet_frags.into_boxed_slice(),
                },
            );

            self.next_packet_seq += PacketSeq::new(1);
            // self.next_ack_at = now + MAX_ACK_DELAY; // TODO
            sent_packet_yet = true;
            Some(packet)
        })
    }
}

fn frag_paths_in_lane(
    now: Instant,
    lane_index: usize,
    lane: &mut Lane,
) -> impl Iterator<Item = (FragmentPath, Instant)> + '_ {
    let lane_index = LaneIndex::from_raw(lane_index.try_into().expect("lane index too large"));

    // drop any messages which have no frags to send
    lane.sent_msgs
        .retain(|_, msg| msg.frags.iter().any(Option::is_some));

    // grab the frag paths from this lane's messages
    lane.sent_msgs.iter().flat_map(move |(msg_seq, msg)| {
        msg.frags
            .iter()
            // we have to enumerate here specifically, since we use the index
            // when building up the `FragmentPath`, and that path has to point
            // back to this exact `Option<..>`
            .enumerate()
            .filter_map(|(i, frag)| frag.as_ref().map(|frag| (i, frag)))
            .filter(move |(_, frag)| now >= frag.next_flush_at.0)
            .map(move |(frag_index, frag)| {
                (
                    FragmentPath {
                        lane_index,
                        msg_seq: *msg_seq,
                        frag_index,
                    },
                    frag.sent_at.0,
                )
            })
    })
}

fn write_frag_at_path(
    now: Instant,
    rtt: &RttEstimator,
    lanes: &mut [Lane],
    bytes_left: &mut impl Limit,
    packet: &mut Vec<u8>,
    path: FragmentPath,
) -> Result<(), ()> {
    let lane_index =
        usize::try_from(path.lane_index.into_raw()).expect("lane index should fit into a `usize`");
    let lane = lanes
        .get_mut(lane_index)
        .expect("frag path should point to a valid lane");

    let msg = lane
        .sent_msgs
        .get_mut(&path.msg_seq)
        .expect("frag path should point to a valid msg in this lane");

    let frag_index = path.frag_index;
    let frag_slot = msg
        .frags
        .get_mut(frag_index)
        .expect("frag index should point to a valid frag slot");
    let sent_frag = frag_slot
        .as_mut()
        .expect("frag path should point to a frag slot which is still occupied");

    let frag = MessageFragment {
        seq: path.msg_seq,
        lane: path.lane_index,
        position: sent_frag.position,
        payload: FragmentPayload(sent_frag.payload.clone().0),
    };
    bytes_left.consume(frag.encode_len()).map_err(drop)?;
    packet
        .write(frag)
        .expect("should grow the buffer when writing over capacity");

    // what does the lane do with this after sending?
    match &lane.reliability {
        LaneReliability::Unreliable => {
            // drop the frag
            // if we've dropped all frags of this message, then
            // on the next `flush`, we'll drop the message
            *frag_slot = None;
        }
        LaneReliability::Reliable => {
            // don't drop the frag, just attempt to resend it later
            // it'll be dropped when the peer acks it
            sent_frag.next_flush_at = sized::Instant(now + rtt.pto());
        }
    }

    Ok(())
}

pub(crate) fn flush(mut sessions: Query<(&mut Transport, &mut PacketBuffers, &PacketMtu)>) {
    let now = Instant::now();
    for (mut transport, mut packet_bufs, &PacketMtu(packet_mtu)) in &mut sessions {
        for packet in transport.flush(now, packet_mtu) {
            packet_bufs.send.push(Bytes::from(packet));
        }
    }
}
