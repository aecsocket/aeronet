mod lane;
mod recv;
mod send;

pub use {recv::*, send::*};

use std::{
    borrow::Borrow,
    fmt::Debug,
    marker::PhantomData,
    time::{Duration, Instant},
};

use aeronet::{
    lane::{LaneKind, LaneMapper},
    message::BytesMapper,
    octs::ConstEncodeLen,
};
use ahash::AHashMap;
use bytes::Bytes;
use derivative::Derivative;

use self::lane::{LaneReceiver, LaneSender};
use crate::{
    ack::Acknowledge,
    byte_count::ByteBucket,
    frag::{FragmentReceiver, FragmentSender},
    seq::Seq,
};

const PACKET_HEADER_LEN: usize = Seq::ENCODE_LEN + Acknowledge::ENCODE_LEN;

/// Configuration of a single outgoing [lane](aeronet::lane).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneConfig {
    /// Kind of lane this configuration will create.
    pub kind: LaneKind,
    /// Maximum amount of bytes which can be sent out on this lane per second.
    pub bandwidth: usize,
    /// For [reliable] lanes: time after initial flush
    ///
    /// [reliable]: aeronet::lane::LaneReliability::Reliable
    pub resend_after: Duration,
}

#[derive(Derivative)]
#[derivative(Debug(bound = "M: Debug"))]
pub struct PacketManager<'m, S, R, M> {
    // send
    lanes_send: Box<[LaneSender]>,
    frag_send: FragmentSender,
    default_packet_cap: usize,
    next_send_packet_seq: Seq,
    next_send_msg_seq: Seq,
    bytes_left: ByteBucket,
    // recv
    lanes_recv: Box<[LaneReceiver<R>]>,
    frag_recv: FragmentReceiver,
    // general
    mapper: &'m M,
    acks: Acknowledge,
    sent_msgs: AHashMap<Seq, SentMessage>,
    flushed_packets: AHashMap<Seq, Box<[FragmentKey]>>,
    _phantom: PhantomData<S>,
}

#[derive(Debug)]
struct SentMessage {
    lane_index: usize,
    num_frags: u8,
    num_unacked: u8,
    frags: Box<[Option<SentFragment>]>,
}

#[derive(Debug, Clone)]
struct SentFragment {
    payload: Bytes,
    next_send_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct FragmentKey {
    msg_seq: Seq,
    frag_index: u8,
}

impl<'m, S, R, M> PacketManager<'m, S, R, M>
where
    M: BytesMapper<S> + LaneMapper<S>,
    M: BytesMapper<R> + LaneMapper<R>,
{
    pub fn new(
        lanes_send: impl IntoIterator<Item = impl Borrow<LaneConfig>>,
        lanes_recv: impl IntoIterator<Item = impl Borrow<LaneKind>>,
        mapper: &'m M,
        max_payload_len: usize,
        default_packet_cap: usize,
        bandwidth: usize,
    ) -> Self {
        Self {
            // send
            lanes_send: lanes_send
                .into_iter()
                .map(|config| LaneSender::new(config.borrow()))
                .collect(),
            frag_send: FragmentSender::new(max_payload_len),
            default_packet_cap,
            next_send_packet_seq: Seq(0),
            next_send_msg_seq: Seq(0),
            bytes_left: ByteBucket::new(bandwidth),
            // recv
            lanes_recv: lanes_recv
                .into_iter()
                .map(|kind| LaneReceiver::new(*kind.borrow()))
                .collect(),
            frag_recv: FragmentReceiver::new(max_payload_len),
            // general
            mapper,
            acks: Acknowledge::new(),
            sent_msgs: AHashMap::new(),
            flushed_packets: AHashMap::new(),
            _phantom: PhantomData,
        }
    }
}
