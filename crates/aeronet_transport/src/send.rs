use std::collections::hash_map::Entry;

use ahash::HashMap;
use arbitrary::Arbitrary;
use octs::Bytes;
use thiserror::Error;
use typesize::derive::TypeSize;
use web_time::Instant;

use crate::{
    frag,
    lane::{LaneIndex, LaneReliability},
    packet::{FragmentPosition, MessageSeq},
    sized,
};

#[derive(Debug, TypeSize)]
pub struct TransportSend {
    max_frag_len: usize,
    lanes: Box<[Lane]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Arbitrary, TypeSize)]
pub struct MessageKey {
    lane: LaneIndex,
    seq: MessageSeq,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SendError {
    #[error("too many messages buffered")]
    TooManyMessages,
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
    pub fn push(&mut self, lane_index: LaneIndex, msg: Bytes) -> Result<MessageKey, SendError> {
        self.push_internal(Instant::now(), lane_index, msg)
    }

    fn push_internal(
        &mut self,
        now: Instant,
        lane_index: LaneIndex,
        msg: Bytes,
    ) -> Result<MessageKey, SendError> {
        let lane = &mut self.lanes[lane_index.into_usize()];
        let msg_seq = lane.next_msg_seq;
        let Entry::Vacant(entry) = lane.sent_msgs.entry(msg_seq) else {
            return Err(SendError::TooManyMessages);
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
        Ok(MessageKey {
            lane: lane_index,
            seq: msg_seq,
        })
    }
}
