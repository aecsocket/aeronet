mod config;
mod recv;
mod send;

pub use {config::*, recv::*, send::*};

use std::{fmt, mem};

use aeronet::lane::{LaneIndex, LaneKind};
use ahash::{AHashMap, AHashSet};
use datasize::{data_size, DataSize};
use derivative::Derivative;
use octs::{Bytes, FixedEncodeLenHint};
use web_time::Instant;

use crate::{
    limit::TokenBucket,
    msg::{FragmentReceiver, MessageSplitter},
    rtt::{RttEstimator, INITIAL_RTT},
    ty::{Acknowledge, FragmentHeader, FragmentMarker, MessageSeq, PacketHeader, PacketSeq},
};

#[derive(Derivative, DataSize)]
#[derivative(Debug)]
pub struct Session {
    connected_at: Instant,
    #[derivative(Debug(format_with = "fmt_flushed_packets"))]
    #[data_size(with = size_of_flushed_packets)]
    flushed_packets: AHashMap<PacketSeq, FlushedPacket>,
    acks: Acknowledge,
    max_memory_usage: usize,

    // send
    send_lanes: Box<[SendLane]>,
    splitter: MessageSplitter,
    min_mtu: usize,
    mtu: usize,
    bytes_left: TokenBucket,
    next_packet_seq: PacketSeq,
    next_keep_alive_at: Instant,
    bytes_sent: usize,

    // recv
    recv_lanes: Box<[RecvLane]>,
    bytes_recv: usize,
    rtt: RttEstimator,
}

fn fmt_flushed_packets(
    value: &AHashMap<PacketSeq, FlushedPacket>,
    fmt: &mut fmt::Formatter,
) -> Result<(), fmt::Error> {
    fmt.debug_set()
        .entries(value.iter().map(|(seq, _)| seq))
        .finish()
}

fn size_of_flushed_packets(value: &AHashMap<PacketSeq, FlushedPacket>) -> usize {
    value
        .iter()
        .map(|(_, packet)| mem::size_of_val(packet) + data_size(packet))
        .sum()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DataSize)]
struct FragmentPath {
    #[data_size(skip)]
    lane_index: LaneIndex,
    msg_seq: MessageSeq,
    frag_index: u8,
}

#[derive(Debug, DataSize)]
struct SentMessage {
    frags: Box<[Option<SentFragment>]>,
}

#[derive(Derivative, DataSize)]
#[derivative(Debug)]
struct SentFragment {
    marker: FragmentMarker,
    #[derivative(Debug = "ignore")]
    #[data_size(with = Bytes::len)]
    payload: Bytes,
    sent_at: Instant,
    next_flush_at: Instant,
}

#[derive(Debug, DataSize)]
struct FlushedPacket {
    flushed_at: Instant,
    frags: Box<[FragmentPath]>,
}

#[derive(Derivative, DataSize)]
#[derivative(Debug)]
struct SendLane {
    #[derivative(Debug(format_with = "fmt_sent_msgs"))]
    #[data_size(with = size_of_sent_msgs)]
    sent_msgs: AHashMap<MessageSeq, SentMessage>,
    next_msg_seq: MessageSeq,
    kind: SendLaneKind,
}

fn fmt_sent_msgs(
    value: &AHashMap<MessageSeq, SentMessage>,
    fmt: &mut fmt::Formatter,
) -> Result<(), fmt::Error> {
    fmt.debug_set()
        .entries(value.iter().map(|(seq, _)| seq))
        .finish()
}

fn size_of_sent_msgs(value: &AHashMap<MessageSeq, SentMessage>) -> usize {
    value
        .iter()
        .map(|(_, msg)| mem::size_of_val(msg) + data_size(msg))
        .sum()
}

#[derive(Debug, DataSize)]
enum SendLaneKind {
    Unreliable,
    Reliable,
}

#[derive(Debug, DataSize)]
struct RecvLane {
    frags: FragmentReceiver,
    kind: RecvLaneKind,
}

#[derive(Derivative)]
#[derivative(Debug)]
enum RecvLaneKind {
    UnreliableUnordered,
    UnreliableSequenced {
        pending_seq: MessageSeq,
    },
    ReliableUnordered {
        pending_seq: MessageSeq,
        recv_seq_buf: AHashSet<MessageSeq>,
    },
    ReliableOrdered {
        pending_seq: MessageSeq,
        #[derivative(Debug(format_with = "fmt_recv_buf"))]
        recv_buf: AHashMap<MessageSeq, Bytes>,
    },
}

fn fmt_recv_buf(value: &AHashMap<MessageSeq, Bytes>, fmt: &mut fmt::Formatter) -> fmt::Result {
    fmt.debug_set()
        .entries(value.iter().map(|(seq, _)| seq))
        .finish()
}

// TODO: DataSize derive is broken on enums. PR a fix or switch dep?
impl DataSize for RecvLaneKind {
    const IS_DYNAMIC: bool = true;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        match self {
            Self::UnreliableUnordered => 0,
            Self::UnreliableSequenced { pending_seq } => data_size(pending_seq),
            Self::ReliableUnordered {
                pending_seq,
                recv_seq_buf,
            } => data_size(pending_seq) + data_size(&recv_seq_buf),
            Self::ReliableOrdered {
                pending_seq,
                recv_buf,
            } => data_size(pending_seq) + size_of_recv_buf(recv_buf),
        }
    }
}

fn size_of_recv_buf(value: &AHashMap<MessageSeq, Bytes>) -> usize {
    value.iter().map(|(_, buf)| buf.len()).sum()
}

/// Attempted to set the [`Session`]'s MTU to a value below the minimum MTU.
///
/// See [`Session`] for an explanation of how MTU works.
#[derive(Debug, Clone, thiserror::Error)]
#[error("MTU of {mtu} is too small (min {min})")]
pub struct MtuTooSmall {
    /// Minimum MTU.
    pub min: usize,
    /// MTU value that you attempted to set.
    pub mtu: usize,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("out of memory")]
pub struct OutOfMemory;

const OVERHEAD: usize = PacketHeader::MAX_ENCODE_LEN + FragmentHeader::MAX_ENCODE_LEN + 1;

impl Session {
    pub fn new(
        now: Instant,
        config: SessionConfig,
        min_mtu: usize,
        initial_mtu: usize,
    ) -> Result<Self, MtuTooSmall> {
        if min_mtu < OVERHEAD {
            return Err(MtuTooSmall {
                min: OVERHEAD,
                mtu: min_mtu,
            });
        }
        if initial_mtu < min_mtu {
            return Err(MtuTooSmall {
                min: min_mtu,
                mtu: initial_mtu,
            });
        }

        let max_payload_len = min_mtu - OVERHEAD;
        Ok(Self {
            connected_at: now,
            flushed_packets: AHashMap::new(),
            acks: Acknowledge::new(),
            max_memory_usage: config.max_memory_usage,

            send_lanes: config
                .send_lanes
                .into_iter()
                .map(|kind| SendLane {
                    sent_msgs: AHashMap::new(),
                    next_msg_seq: MessageSeq::ZERO,
                    kind: match kind {
                        LaneKind::UnreliableUnordered | LaneKind::UnreliableSequenced => {
                            SendLaneKind::Unreliable
                        }
                        LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => {
                            SendLaneKind::Reliable
                        }
                    },
                })
                .collect(),
            splitter: MessageSplitter::new(max_payload_len),
            min_mtu,
            mtu: initial_mtu,
            bytes_left: TokenBucket::new(config.send_bytes_per_sec),
            next_packet_seq: PacketSeq::default(),
            next_keep_alive_at: now,
            bytes_sent: 0,

            recv_lanes: config
                .recv_lanes
                .into_iter()
                .map(|kind| RecvLane {
                    frags: FragmentReceiver::new(max_payload_len),
                    kind: match kind {
                        LaneKind::UnreliableUnordered => RecvLaneKind::UnreliableUnordered,
                        LaneKind::UnreliableSequenced => RecvLaneKind::UnreliableSequenced {
                            pending_seq: MessageSeq::ZERO,
                        },
                        LaneKind::ReliableUnordered => RecvLaneKind::ReliableUnordered {
                            pending_seq: MessageSeq::ZERO,
                            recv_seq_buf: AHashSet::new(),
                        },
                        LaneKind::ReliableOrdered => RecvLaneKind::ReliableOrdered {
                            pending_seq: MessageSeq::ZERO,
                            recv_buf: AHashMap::new(),
                        },
                    },
                })
                .collect(),
            bytes_recv: 0,
            rtt: RttEstimator::new(INITIAL_RTT),
        })
    }

    #[must_use]
    pub const fn connected_at(&self) -> Instant {
        self.connected_at
    }

    #[must_use]
    pub const fn min_mtu(&self) -> usize {
        self.min_mtu
    }

    #[must_use]
    pub const fn mtu(&self) -> usize {
        self.mtu
    }

    pub fn set_mtu(&mut self, mtu: usize) -> Result<(), MtuTooSmall> {
        if mtu < self.min_mtu {
            Err(MtuTooSmall {
                min: self.min_mtu,
                mtu,
            })
        } else {
            self.mtu = mtu;
            Ok(())
        }
    }

    #[must_use]
    pub const fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    #[must_use]
    pub const fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    #[must_use]
    pub const fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }

    #[must_use]
    pub const fn bytes_left(&self) -> &TokenBucket {
        &self.bytes_left
    }

    #[must_use]
    pub const fn max_memory_usage(&self) -> usize {
        self.max_memory_usage
    }

    #[must_use]
    pub fn memory_usage(&self) -> usize {
        // TODO proper tools for debugging usage
        // dbg!(size_of_flushed_packets(&self.flushed_packets));
        // dbg!(self.flushed_packets.len());
        // dbg!(data_size(&self.send_lanes));
        // dbg!(data_size(&self.recv_lanes));
        data_size(self)
    }
}

/// Indicates that this client may be backed by a [`Session`].
pub trait SessionBacked {
    fn get_session(&self) -> Option<&Session>;
}
