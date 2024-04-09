mod lane;
mod recv;
mod send;

pub use {lane::*, recv::*, send::*};

use std::{borrow::Borrow, fmt::Debug, marker::PhantomData, time::Duration};

use aeronet::{
    lane::{LaneKind, LaneMapper},
    message::BytesMapper,
    octs::ConstEncodeLen,
    stats::{MessageByteStats, MessageStats},
};
use ahash::AHashMap;
use bytes::Bytes;
use derivative::Derivative;
use web_time::Instant;

use self::lane::{LaneReceiver, LaneSenderKind};
use crate::{
    ack::Acknowledge,
    byte_count::ByteBucket,
    frag::{FragmentReceiver, FragmentSender},
    seq::Seq,
};

const PACKET_HEADER_LEN: usize = Seq::ENCODE_LEN + Acknowledge::ENCODE_LEN;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct PacketStats {
    /// See [`MessageStats::msgs_sent`].
    pub msgs_sent: usize,
    /// See [`MessageStats::msgs_recv`].
    pub msgs_recv: usize,
    /// See [`MessageByteStats::msg_bytes_sent`].
    pub msg_bytes_sent: usize,
    /// See [`MessageByteStats::msg_bytes_recv`].
    pub msg_bytes_recv: usize,
}

impl MessageStats for PacketStats {
    fn msgs_sent(&self) -> usize {
        self.msgs_sent
    }

    fn msgs_recv(&self) -> usize {
        self.msgs_recv
    }
}

impl MessageByteStats for PacketStats {
    fn msg_bytes_sent(&self) -> usize {
        self.msg_bytes_sent
    }

    fn msg_bytes_recv(&self) -> usize {
        self.msg_bytes_recv
    }
}

#[derive(Derivative)]
#[derivative(Debug(bound = "M: Debug"))]
pub struct PacketManager<S, R, M> {
    pub stats: PacketStats,
    mapper: M,
    acks: Acknowledge,
    // insertion policy: on buffer send
    // removal policy: on read acks, after all frags of the message are acked
    sent_msgs: AHashMap<Seq, SentMessage>,
    // insertion policy: on flush
    // removal policy: on read acks, after all fragments in a packet are acked TODO
    flushed_packets: AHashMap<Seq, FlushedPacket>,
    send: PacketSender<S, M>,
    recv: PacketReceiver<R, M>,
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

#[derive(Debug, Clone)]
struct FlushedPacket {
    frags: Box<[FragmentKey]>,
    num_unacked: usize,
}

#[derive(Debug, Clone, Copy)]
struct FragmentKey {
    msg_seq: Seq,
    frag_index: u8,
}

impl<S, R, M> PacketManager<S, R, M>
where
    M: BytesMapper<S> + LaneMapper<S>,
    M: BytesMapper<R> + LaneMapper<R>,
{
    pub fn new(
        max_packet_len: usize,
        default_packet_cap: usize,
        bandwidth: usize,
        lanes_send: impl IntoIterator<Item = impl Borrow<LaneConfig>>,
        lanes_recv: impl IntoIterator<Item = impl Borrow<LaneKind>>,
        mapper: M,
    ) -> Self {
        assert!(max_packet_len > PACKET_HEADER_LEN);
        let max_payload_len = max_packet_len - PACKET_HEADER_LEN;
        Self {
            stats: PacketStats::default(),
            // send
            send_lanes: lanes_send
                .into_iter()
                .map(|config| LaneSenderKind::new(config.borrow()))
                .collect(),
            send_frags: FragmentSender::new(max_payload_len),
            max_packet_len,
            default_packet_cap,
            next_send_packet_seq: Seq(0),
            next_send_msg_seq: Seq(0),
            bytes_left: ByteBucket::new(bandwidth),
            // recv
            recv_lanes: lanes_recv
                .into_iter()
                .map(|kind| LaneReceiver::new(*kind.borrow()))
                .collect(),
            recv_frags: FragmentReceiver::new(max_payload_len),
            // general
            mapper,
            acks: Acknowledge::new(),
            sent_msgs: AHashMap::new(),
            flushed_packets: AHashMap::new(),
            _phantom: PhantomData,
        }
    }
}
