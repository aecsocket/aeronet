//! See [`Session`].

mod config;
mod recv;
mod send;

pub use {config::*, recv::*, send::*};

use std::{fmt, mem, num::Saturating};

use aeronet::lane::{LaneIndex, LaneKind};
use ahash::{AHashMap, AHashSet};
use datasize::{data_size, DataSize};
use derivative::Derivative;
use octs::{Bytes, FixedEncodeLenHint};
use web_time::{Duration, Instant};

use crate::{
    limit::TokenBucket,
    msg::{FragmentReceiver, MessageSplitter},
    rtt::{RttEstimator, INITIAL_RTT},
    seq::SeqBuf,
    ty::{Acknowledge, FragmentHeader, FragmentMarker, MessageSeq, PacketHeader, PacketSeq},
};

/// Manages the messages sent and received over a transport's connection without
/// performing any I/O.
///
/// See the [crate-level documentation](crate).
#[derive(Derivative, DataSize)]
#[derivative(Debug)]
pub struct Session {
    #[data_size(skip)]
    connected_at: Instant,
    flushed_packets: SeqBuf<FlushedPacket, 1024>,
    acks: Acknowledge,
    max_memory_usage: usize,

    // send
    send_lanes: Box<[SendLane]>,
    splitter: MessageSplitter,
    min_mtu: usize,
    mtu: usize,
    bytes_left: TokenBucket,
    next_packet_seq: PacketSeq,
    #[data_size(skip)]
    next_ack_at: Instant,
    #[data_size(skip)]
    packets_sent: Saturating<usize>,
    #[data_size(skip)]
    bytes_sent: Saturating<usize>,

    // recv
    recv_lanes: Box<[RecvLane]>,
    #[data_size(skip)]
    packets_recv: Saturating<usize>,
    #[data_size(skip)]
    packets_acked: Saturating<usize>,
    #[data_size(skip)]
    bytes_recv: Saturating<usize>,
    rtt: RttEstimator,
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
    #[data_size(skip)]
    sent_at: Instant,
    #[data_size(skip)]
    next_flush_at: Instant,
}

#[derive(Debug, DataSize)]
struct FlushedPacket {
    #[data_size(skip)]
    flushed_at: Instant,
    frags: Box<[FragmentPath]>,
}

impl FlushedPacket {
    fn new(flushed_at: Instant) -> Self {
        Self {
            flushed_at,
            frags: Box::new([]),
        }
    }
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
/// See [`Session`], *MTU*.
#[derive(Debug, Clone, thiserror::Error)]
#[error("MTU of {mtu} is too small (min {min})")]
pub struct MtuTooSmall {
    /// Minimum MTU.
    pub min: usize,
    /// MTU value that you attempted to set.
    pub mtu: usize,
}

/// This [`Session`] is occupying too many bytes in memory for buffering
/// messages, and the session can no longer be used.
///
/// See [`Session`], *Memory management*.
#[derive(Debug, Clone, thiserror::Error)]
#[error("out of memory")]
pub struct OutOfMemory;

/// How many bytes of overhead a packet requires to encode the header and at
/// least one fragment.
pub const OVERHEAD: usize = PacketHeader::MAX_ENCODE_LEN + FragmentHeader::MAX_ENCODE_LEN + 1;

/// Maximum amount of time to wait until we will be forced to send a packet,
/// even if it is empty.
///
/// If we receive a packet from the peer, we will immediately try to send a
/// response packet. Likewise, when the peer receives our response, it will
/// immediately try to send a response. Due to this, in theory there should
/// always be a ping-pong of packets happening over the connection.
///
/// However, if there are connection errors e.g. packet loss or a sudden RTT
/// change, this ping-pong might be interrupted and will not restart again until
/// the user code explicitly sends a message. In order to automatically restart
/// this ping-pong without user intervention, if we haven't sent a packet in the
/// last [`MAX_ACK_DELAY`], we will forcefully send an empty one.
const MAX_ACK_DELAY: Duration = Duration::from_millis(500);

impl Session {
    fn new<const CLIENT: bool>(
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
        let (send_lanes, recv_lanes) = if CLIENT {
            (config.client_lanes, config.server_lanes)
        } else {
            (config.server_lanes, config.client_lanes)
        };

        Ok(Self {
            connected_at: now,
            flushed_packets: SeqBuf::new_from_fn(|_| FlushedPacket::new(now)),
            acks: Acknowledge::new(),
            max_memory_usage: config.max_memory_usage,

            send_lanes: send_lanes
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
            next_ack_at: now + MAX_ACK_DELAY,
            packets_sent: Saturating(0),
            bytes_sent: Saturating(0),

            recv_lanes: recv_lanes
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
            packets_recv: Saturating(0),
            packets_acked: Saturating(0),
            bytes_recv: Saturating(0),
            rtt: RttEstimator::new(INITIAL_RTT),
        })
    }

    /// Creates a new client session.
    ///
    /// See [`Session`] for an explanation of what the `min_mtu` and
    /// `initial_mtu` values mean.
    ///
    /// If you are unsure what to use for `min_mtu`, see if your underlying
    /// transport has a minimum packet size that it supports. If not, consider
    /// using what [RFC 9000 Section 14.2] (the spec behind QUIC) uses, which
    /// is `1200`.
    ///
    /// `initial_mtu` should be an initial path MTU estimate if you have one,
    /// otherwise it may be the same as `min_mtu`.
    ///
    /// # Errors
    ///
    /// Errors if `min_mtu` is smaller than [`OVERHEAD`], or if
    /// `initial_mtu < min_mtu`.
    ///
    /// [RFC 9000 Section 14.2]: https://www.rfc-editor.org/rfc/rfc9000.html#section-14-2
    pub fn client(
        now: Instant,
        config: SessionConfig,
        min_mtu: usize,
        initial_mtu: usize,
    ) -> Result<Self, MtuTooSmall> {
        Self::new::<true>(now, config, min_mtu, initial_mtu)
    }

    /// Creates a new server session.
    ///
    /// See [`Session`] for an explanation of what the `min_mtu` and
    /// `initial_mtu` values mean.
    ///
    /// If you are unsure what to use for `min_mtu`, see if your underlying
    /// transport has a minimum packet size that it supports. If not, consider
    /// using what [RFC 9000 Section 14.2] (the spec behind QUIC) uses, which
    /// is `1200`.
    ///
    /// `initial_mtu` should be an initial path MTU estimate if you have one,
    /// otherwise it may be the same as `min_mtu`.
    ///
    /// # Errors
    ///
    /// Errors if `min_mtu` is smaller than [`OVERHEAD`], or if
    /// `initial_mtu < min_mtu`.
    ///
    /// [RFC 9000 Section 14.2]: https://www.rfc-editor.org/rfc/rfc9000.html#section-14-2
    pub fn server(
        now: Instant,
        config: SessionConfig,
        min_mtu: usize,
        initial_mtu: usize,
    ) -> Result<Self, MtuTooSmall> {
        Self::new::<false>(now, config, min_mtu, initial_mtu)
    }

    /// Gets when this session was created.
    #[must_use]
    pub const fn connected_at(&self) -> Instant {
        self.connected_at
    }

    /// Gets the minimum MTU.
    #[must_use]
    pub const fn min_mtu(&self) -> usize {
        self.min_mtu
    }

    /// Gets the current MTU.
    #[must_use]
    pub const fn mtu(&self) -> usize {
        self.mtu
    }

    /// Sets the current MTU.
    ///
    /// # Errors
    ///
    /// Errors if `mtu < min_mtu`.
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

    /// Gets the current RTT estimation state.
    #[must_use]
    pub const fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    /// Gets how many packets have been sent out in total through
    /// [`Session::flush`].
    #[must_use]
    pub const fn packets_sent(&self) -> usize {
        self.packets_sent.0
    }

    /// Gets how many packets have been received in total through
    /// [`Session::recv`].
    #[must_use]
    pub const fn packets_recv(&self) -> usize {
        self.packets_recv.0
    }

    /// Gets how many of our packets the peer has acknowledged as received.
    #[must_use]
    pub const fn packets_acked(&self) -> usize {
        self.packets_acked.0
    }

    /// Gets how many bytes this session have been sent out in total through
    /// [`Session::flush`].
    #[must_use]
    pub const fn bytes_sent(&self) -> usize {
        self.bytes_sent.0
    }

    /// Gets how many bytes this session has received in total through
    /// [`Session::recv`].
    #[must_use]
    pub const fn bytes_recv(&self) -> usize {
        self.bytes_recv.0
    }

    /// Gets how many bytes this session can still send out until its byte
    /// send bucket gets refilled.
    #[must_use]
    pub const fn bytes_left(&self) -> &TokenBucket {
        &self.bytes_left
    }

    /// Gets the maximum amount of bytes this session may occupy in memory until
    /// operations fail with [`OutOfMemory`].
    #[must_use]
    pub const fn max_memory_usage(&self) -> usize {
        self.max_memory_usage
    }

    /// Gets the total number of bytes this session occupies in memory.
    ///
    /// This includes both on the stack and on the heap.
    #[must_use]
    pub fn memory_usage(&self) -> usize {
        mem::size_of_val(self) + data_size(self)
    }

    /// Updates the internal state of this session, accepting the time delta
    /// between this update and the last.
    ///
    /// This should be called once per update.
    ///
    /// # Errors
    ///
    /// Errors if the session is using too much memory. If this return an error,
    /// the session must be dropped and the connection must be immediately
    /// closed.
    pub fn update(&mut self, delta_time: Duration) -> Result<(), OutOfMemory> {
        if self.memory_usage() > self.max_memory_usage {
            return Err(OutOfMemory);
        }

        let f = delta_time.as_secs_f32();
        self.bytes_left.refill_portion(f);

        Ok(())
    }
}

/// Indicates that this client transport may be backed by a [`Session`].
pub trait SessionBacked {
    /// Gets the [`Session`] that this client transport is currently using to
    /// manage its connection.
    fn get_session(&self) -> Option<&Session>;
}

#[cfg(feature = "condition")]
impl<T: aeronet::client::ClientTransport + SessionBacked> SessionBacked
    for aeronet::condition::ConditionedClient<T>
{
    fn get_session(&self) -> Option<&Session> {
        self.inner().get_session()
    }
}
