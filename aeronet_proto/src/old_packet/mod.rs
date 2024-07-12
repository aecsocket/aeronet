//! TODO docs
//!
//! # Process
//!
//! ## Sending
//!
//! * Create your message and pass it to [`PacketManager::buffer_send`]
//!   * On [`Err`], close the connection immediately
//!   * On [`Ok`], store the resulting [`Seq`] so that you know when this
//!     message gets acknowledged by the peer
//! * At the end of the app update loop, call [`PacketManager::flush`] and send
//!   all resulting [`Bytes`] packets to the peer
//!
//! # Connection errors
//!
//! Connection errors can be split into two kinds: fatal and non-fatal. Fatal
//! errors force the implementation to close the connection, as there is some
//! fundamental issue in the connection, and we cannot safely continue.
//! Non-fatal errors are abnormalities, but the connection can still continue
//! safely.
//!
//! ## Fatal connection errors
//!
//! If a function returns an [`Err`] variant of a type which is a fatal error,
//! then you must immediately close the connection.

mod recv;
mod send;

pub use {recv::*, send::*};

use std::fmt::Debug;

use aeronet::{
    lane::LaneMapper,
    message::BytesMapper,
    octs::ConstEncodeLen,
    stats::{MessageByteStats, MessageStats},
};
use ahash::AHashMap;
use bytes::Bytes;
use derivative::Derivative;
use web_time::Instant;

use crate::{ack::Acknowledge, lane::LaneConfig, seq::Seq};

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
#[derivative(Debug(bound = "M: Debug"), Clone(bound = "R: Clone, M: Clone"))]
pub struct PacketManager<S, R, M> {
    stats: PacketStats,
    mapper: M,
    acks: Acknowledge,
    // insertion policy: on buffer send
    // removal policy: on read acks, after all frags of the message are acked
    sent_msgs: AHashMap<Seq, SentMessage>,
    // insertion policy: on flush
    // removal policy: on read acks, after all fragments in a packet are acked
    flushed_packets: AHashMap<Seq, FlushedPacket>,
    send: send::PacketSender<S, M>,
    recv: recv::PacketReceiver<R, M>,
}

#[derive(Debug, Clone)]
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
        lanes_send: &[LaneConfig],
        lanes_recv: &[LaneConfig],
        mapper: M,
    ) -> Self {
        let max_payload_len = max_packet_len
            .checked_sub(PACKET_HEADER_LEN)
            .unwrap_or_else(|| panic!("max_packet_len must be less than PACKET_HEADER_LEN"));
        Self {
            stats: PacketStats::default(),
            mapper,
            acks: Acknowledge::new(),
            sent_msgs: AHashMap::new(),
            flushed_packets: AHashMap::new(),
            send: send::PacketSender::new(
                max_packet_len,
                max_payload_len,
                default_packet_cap,
                bandwidth,
                lanes_send,
            ),
            recv: recv::PacketReceiver::new(max_packet_len, max_payload_len, lanes_recv),
        }
    }

    fn stats(&self) -> &PacketStats {
        &self.stats
    }
}
