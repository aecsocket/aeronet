use std::collections::hash_map::Entry;

use aeronet_io::packet::PacketBuffers;
use ahash::HashMap;
use arbitrary::Arbitrary;
use bevy_ecs::prelude::*;
use octs::Bytes;
use typesize::derive::TypeSize;
use web_time::Instant;

use crate::{
    frag,
    lane::{LaneIndex, LaneReliability},
    packet::{FragmentPosition, MessageSeq},
    rtt::RttEstimator,
    sized, FragmentPath, Transport,
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
    pub fn push(&mut self, lane_index: LaneIndex, msg: Bytes) -> Option<MessageKey> {
        self.push_internal(Instant::now(), lane_index, msg)
    }

    fn push_internal(
        &mut self,
        now: Instant,
        lane_index: LaneIndex,
        msg: Bytes,
    ) -> Option<MessageKey> {
        let lane = &mut self.lanes[usize::from(lane_index)];
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
    fn flush(&mut self, now: Instant) {
        // collect the paths of the frags to send, along with how old they are
        let mut frag_paths = self
            .send
            .lanes
            .iter_mut()
            .enumerate()
            .flat_map(|(lane_index, lane)| frag_paths_in_lane(now, lane_index, lane))
            .collect::<Vec<_>>();

        // sort by oldest sent to newest
        frag_paths.sort_unstable_by(|(_, sent_at_a), (_, sent_at_b)| sent_at_a.cmp(sent_at_b));

        let mut frag_paths = frag_paths
            .into_iter()
            .map(|(path, _)| Some(path))
            .collect::<Vec<_>>();
    }
}

fn frag_paths_in_lane(
    now: Instant,
    lane_index: usize,
    lane: &mut Lane,
) -> impl Iterator<Item = (FragmentPath, Instant)> + '_ {
    let lane_index = LaneIndex::try_from(lane_index).expect("too many lanes");

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

fn write_frag_path(
    now: Instant,
    rtt: &RttEstimator,
    lanes: &mut [Lane],
    bytes_left: &mut impl Limit,
    packet: &mut Vec<u8>,
    path: FragmentPath,
) -> Result<(), ()> {
    let lane_index =
        usize::try_from(path.lane_index.into_raw()).expect("lane index should fit into a usize");
    let lane = lanes
        .get_mut(lane_index)
        .expect("frag path should point to a valid lane");

    let msg = lane
        .sent_msgs
        .get_mut(&path.msg_seq)
        .expect("frag path should point to a valid msg in this lane");

    let frag_index = usize::from(path.frag_index);
    let frag_slot = msg
        .frags
        .get_mut(frag_index)
        .expect("frag index should point to a valid frag slot");
    let sent_frag = frag_slot
        .as_mut()
        .expect("frag path should point to a frag slot which is still occupied");

    let frag = Fragment {
        header: FragmentHeader {
            lane_index: path.lane_index,
            msg_seq: path.msg_seq,
            marker: sent_frag.marker,
        },
        payload: sent_frag.payload.clone(),
    };
    bytes_left.consume(frag.encode_len()).map_err(drop)?;
    packet
        .write(frag)
        .expect("BytesMut should grow the buffer when writing over capacity");

    // what does the lane do with this after sending?
    match &lane.kind {
        SendLaneKind::Unreliable => {
            // drop the frag
            // if we've dropped all frags of this message, then
            // on the next `flush`, we'll drop the message
            *frag_slot = None;
        }
        SendLaneKind::Reliable => {
            // don't drop the frag, just attempt to resend it later
            // it'll be dropped when the peer acks it
            sent_frag.next_flush_at = now + rtt.pto();
        }
    }

    Ok(())
}

pub(crate) fn flush(mut sessions: Query<(&mut Transport, &mut PacketBuffers)>) {
    for (mut transport, mut packet_bufs) in &mut sessions {}
}
