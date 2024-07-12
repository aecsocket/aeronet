mod packet;
mod recv;
mod send;

use std::{
    convert::Infallible,
    ops::{Deref, DerefMut},
};

pub use {recv::*, send::*};

use aeronet::lane::{LaneIndex, LaneKind};
use ahash::AHashMap;
use octs::{
    chunks::ByteChunksExt, BufTooShortOr, Bytes, Decode, Encode, EncodeLen, FixedEncodeLen, Read,
    VarInt, VarIntTooLarge, Write,
};
use packet::PacketHeader;
use web_time::{Duration, Instant};

use crate::{
    ack::Acknowledge,
    frag::{Fragment, FragmentHeader, FragmentReceiver},
    packet::MessageSeq,
    seq::Seq,
};

#[derive(Debug)]
pub struct Session {
    send_lanes: Box<[SendLane]>,
    sent_msgs: AHashMap<MessageSeq, SentMessage>,
}

#[derive(Debug)]
enum SendLane {
    Unreliable,
    Reliable { resend_after: Duration },
}

#[derive(Debug)]
struct SentMessage {
    lane_index: LaneIndex,
    frags: Box<[Option<SentFragment>]>,
}

#[derive(Debug)]
struct SentFragment {
    payload: Bytes,
    next_flush_at: Instant,
}

impl Session {
    pub fn new(max_packet_len: usize) -> Self {
        todo!()
    }

    pub fn send(&mut self, msg: impl Into<Bytes>) {
        let msg: Bytes = msg.into();
        msg.byte_chunks(chunk_len)
    }
}
