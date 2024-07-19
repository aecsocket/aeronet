//! See [`Session`].

mod config;
mod recv;
mod send;

pub use {config::*, recv::*};

use std::convert::Infallible;

use aeronet::lane::{LaneIndex, LaneKind};
use ahash::{AHashMap, AHashSet};
use bevy_replicon::prelude::ClientDiagnosticsPlugin;
use datasize::DataSize;
use derivative::Derivative;
use octs::{BufTooShortOr, Bytes, FixedEncodeLen, VarIntTooLarge};
use web_time::{Duration, Instant};

use crate::{
    ack::Acknowledge,
    byte_count::ByteBucket,
    frag::{
        Fragment, FragmentHeader, FragmentReceiver, FragmentSender, MessageTooBig, ReassembleError,
    },
    packet::{MessageSeq, PacketHeader, PacketSeq},
    rtt::{RttEstimator, INITIAL_RTT},
    seq::Seq,
};

/// Manages the state of a data transport session between two peers.
///
/// To use the aeronet protocol, this is the main type you should be interacting
/// with when receiving or sending out byte messages.
///
/// # MTU
///
/// When dealing with individual packets, we have to be careful of the MTU
/// (maximum transmissible unit) of the underlying connection. If we send a
/// packet which is too large, then peers along our route will likely drop it.
/// Therefore, we break large messages down into smaller fragments and send
/// those over instead.
///
/// - The maximum length of a fragment is `min_mtu` minus some packet overhead
/// - `min_mtu` must be at least [`OVERHEAD`] bytes
/// - This remains constant for the whole session
/// - Both sides must use the exact same `min_mtu`
/// - It is recommended to use a compile-time constant for `min_mtu`
///
/// However, while the connection is active, the path MTU may change. If so,
/// we would like to adapt the session to make the most of this new MTU.
/// This means that the maximum packet length increases, and we can send
/// more fragments out in a single packet (remember, though, that we can't
/// make the fragments themselves larger).
///
/// - The maximum length of a packet is defined by `initial_mtu`
/// - You may change this during the lifetime of the session by using
///   [`Session::set_mtu`]
/// - Both sides may have a different MTU
/// - The MTU must always be greater than or equal to `min_mtu`, otherwise you
///   get [`MtuTooSmall`]
#[derive(Derivative, datasize::DataSize)]
#[derivative(Debug)]
pub struct Session {
    /// Stores messages which have been sent using [`Session::send`], but still
    /// need to be flushed in [`Session::flush`].
    ///
    /// # Insertion policy
    ///
    /// In [`Session::send`].
    ///
    /// # Removal policy
    ///
    /// At the start of [`Session::flush`], messages with no fragments, or only
    /// [`None`] fragment slots, are removed.
    ///
    /// When a fragment is flushed,
    /// - if the message is unreliable, the fragment slot is immediately set to
    ///   [`None`]
    /// - if the message is reliable, the fragment is kept until the packet it
    ///   was flushed in was acked, at which point the fragment slot is set to
    ///   [`None`]
    #[data_size(with = sent_msgs_data_size)]
    #[derivative(Debug(format_with = "sent_msgs_fmt"))]
    sent_msgs: AHashMap<MessageSeq, SentMessage>,
    /// Tracks which packets have been flushed but not acknowledged by the peer
    /// yet, and what fragments those packets contained.
    ///
    /// # Insertion policy
    ///
    /// In [`Session::flush`].
    ///
    /// # Removal policy
    ///
    /// In [`Session::recv`], when we get an ack for a packet sequence, its
    /// entry in this map is removed.
    ///
    /// If a peer never sends acks, our side will keep the fragments around in
    /// memory forever, until `max_memory_usage` bytes are used up, and we end
    /// the connection with an [`OutOfMemory`].
    #[data_size(with = flushed_packets_data_size)]
    #[derivative(Debug(format_with = "flushed_packets_fmt"))]
    flushed_packets: AHashMap<PacketSeq, FlushedPacket>,
    /// Tracks which packets we have acknowledged from the peer.
    acks: Acknowledge,
    /// Maximum number of bytes that this session can use to buffer messages.
    ///
    /// This applies to:
    /// - how many bytes can be used by `recv_frags` to store incomplete
    ///   messages received from our peer
    /// - how many bytes can be used by `sent_msgs` and `flushed_packets` to
    ///   store reliable message fragments which haven't been acked yet
    max_memory_usage: usize,

    // send
    /// Outgoing lane state.
    send_lanes: Box<[SendLane]>,
    /// Allows splitting a message into smaller fragments.
    send_frags: FragmentSender,
    /// Minimum MTU as defined in [`Session::new`].
    min_mtu: usize,
    /// Maximum length of a single flushed packet (maximum transmissible unit).
    mtu: usize,
    /// Tracks how many bytes remaining we have to send to our peer.
    ///
    /// This should be filled up by the user using [`Session::refill_bytes`].
    bytes_left: ByteBucket,
    /// Next outgoing message sequence.
    next_msg_seq: MessageSeq,
    /// Next outgoing packet sequence.
    next_packet_seq: PacketSeq,
    /// When we should send the next keep-alive packet.
    next_keep_alive_at: Instant,
    /// Total number of bytes sent.
    bytes_sent: usize,

    // recv
    /// Incoming lane state.
    recv_lanes: Box<[RecvLane]>,
    /// Buffers fragments received from the peer for reassembly.
    ///
    /// # Insertion policy
    ///
    /// In [`Session::recv`] when reading a fragment in a packet.
    ///
    /// The maximum number of bytes this receiver can hold is given by
    /// `max_memory_usage`. If we receive a fragment but do not have enough
    /// capacity to insert it into the receiver, the connection is closed.
    ///
    /// Note that since this buffer has no concept of lanes or reliability, we
    /// can't just drop some unreliable messages to make room, since we don't
    /// know if a given fragment is for a reliable or unreliable message.
    ///
    /// # Removal policy
    ///
    /// In [`Session::recv`] when a message has been fully reassembled, by
    /// receiving all of its fragments.
    recv_frags: FragmentReceiver,
    /// Total number of bytes received.
    bytes_recv: usize,
    /// Estimates RTT.
    rtt: RttEstimator,
}

fn sent_msgs_data_size(value: &AHashMap<MessageSeq, SentMessage>) -> usize {
    std::mem::size_of_val(value)
        + value
            .iter()
            .map(|(_, msg)| std::mem::size_of_val(msg) + datasize::data_size(msg))
            .sum::<usize>()
}

fn sent_msgs_fmt(
    value: &AHashMap<MessageSeq, SentMessage>,
    fmt: &mut std::fmt::Formatter,
) -> Result<(), std::fmt::Error> {
    fmt.debug_set()
        .entries(value.iter().map(|(MessageSeq(Seq(seq)), _)| seq))
        .finish()
}

fn flushed_packets_data_size(value: &AHashMap<PacketSeq, FlushedPacket>) -> usize {
    std::mem::size_of_val(value)
        + value
            .iter()
            .map(|(_, packet)| std::mem::size_of_val(packet) + datasize::data_size(packet))
            .sum::<usize>()
}

fn flushed_packets_fmt(
    value: &AHashMap<PacketSeq, FlushedPacket>,
    fmt: &mut std::fmt::Formatter,
) -> Result<(), std::fmt::Error> {
    fmt.debug_set()
        .entries(value.iter().map(|(PacketSeq(Seq(seq)), _)| seq))
        .finish()
}

/// Error when attempting to buffer a message for sending using
/// [`Session::send`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum SendError {
    /// See [`MessageTooBig`].
    #[error(transparent)]
    MessageTooBig(#[from] MessageTooBig),
    /// Attempted to buffer a message into a slot for a [`MessageSeq`] which is
    /// already occupied.
    #[error("too many buffered messages")]
    TooManyMessages,
    /// Attempted to send a message on a lane which does not exist.
    #[error("invalid lane")]
    InvalidLane,
}

/// Error when attempting to read a packet using [`Session::recv`].
///
/// It is safe to ignore this error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RecvError {
    /// Failed to read the [`PacketHeader`].
    #[error("failed to read header")]
    ReadHeader(#[source] BufTooShortOr<Infallible>),
    /// Failed to read a [`Fragment`].
    #[error("failed to read fragment")]
    ReadFragment(#[source] BufTooShortOr<VarIntTooLarge>),
    /// Failed to reassemble a [`Fragment`] using [`FragmentReceiver`].
    #[error("failed to reassemble fragment")]
    Reassemble(#[source] ReassembleError),
    /// Failed to read the [`LaneIndex`] of a reassembled message.
    #[error("failed to read lane index")]
    ReadLaneIndex(#[source] BufTooShortOr<VarIntTooLarge>),
    /// The [`LaneIndex`] we read from a reassembled message is not a valid
    /// receive lane index that we have in our session.
    #[error("invalid lane index `{index}`")]
    InvalidLaneIndex {
        /// Index of the lane that we received.
        index: usize,
    },
}

/// Ran out of memory while attempting to buffer some data.
///
/// To avoid a malicious peer using up all of our memory, we set limits on how
/// many bytes can be used to buffer data such as incoming fragments. While
/// using a [`Session`], if we exceed this limit, an [`OutOfMemory`] error is
/// returned and the connection must be forcibly closed.
#[derive(Debug, Clone, thiserror::Error)]
#[error("out of memory")]
pub struct OutOfMemory;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, datasize::DataSize)]
struct FragmentPath {
    msg_seq: MessageSeq,
    index: u8,
}

/// State of a [`Session`] lane used for sending messages.
#[derive(Debug, datasize::DataSize)]
pub struct SendLane {
    /// Tracks how many bytes we have left for sending along this lane.
    pub bytes_left: ByteBucket,
    kind: SendLaneKind,
}

#[derive(Debug, datasize::DataSize)]
enum SendLaneKind {
    Unreliable,
    Reliable { resend_after: Duration },
}

#[derive(Debug, datasize::DataSize)]
struct SentMessage {
    #[data_size(skip)]
    lane_index: LaneIndex,
    frags: Box<[Option<SentFragment>]>,
}

#[derive(Debug, datasize::DataSize)]
struct SentFragment {
    frag: Fragment,
    next_flush_at: Instant,
}

#[derive(Debug, datasize::DataSize)]
struct FlushedPacket {
    flushed_at: Instant,
    frags: Box<[FragmentPath]>,
}

#[derive(Derivative)]
#[derivative(Debug)]
enum RecvLane {
    UnreliableUnordered,
    UnreliableSequenced {
        pending_seq: MessageSeq,
    },
    ReliableUnordered {
        pending_seq: MessageSeq,
        #[derivative(Debug(format_with = "recv_seq_buf_fmt"))]
        recv_seq_buf: AHashSet<MessageSeq>,
    },
    ReliableOrdered {
        pending_seq: MessageSeq,
        #[derivative(Debug(format_with = "recv_buf_fmt"))]
        recv_buf: AHashMap<MessageSeq, Bytes>,
    },
}

fn recv_seq_buf_fmt(
    value: &AHashSet<MessageSeq>,
    fmt: &mut std::fmt::Formatter,
) -> Result<(), std::fmt::Error> {
    fmt.debug_set()
        .entries(value.iter().map(|MessageSeq(Seq(seq))| seq))
        .finish()
}

fn recv_buf_fmt(
    value: &AHashMap<MessageSeq, Bytes>,
    fmt: &mut std::fmt::Formatter,
) -> Result<(), std::fmt::Error> {
    fmt.debug_set()
        .entries(value.iter().map(|(MessageSeq(Seq(seq)), _)| seq))
        .finish()
}

// TODO: datasize::DataSize derive is broken on enums. PR a fix or switch dep?
impl datasize::DataSize for RecvLane {
    const IS_DYNAMIC: bool = true;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        match self {
            Self::UnreliableUnordered => 0,
            Self::UnreliableSequenced { pending_seq } => datasize::data_size(pending_seq),
            Self::ReliableUnordered {
                pending_seq,
                recv_seq_buf,
            } => datasize::data_size(pending_seq) + recv_seq_buf_byte_size(recv_seq_buf),
            Self::ReliableOrdered {
                pending_seq,
                recv_buf,
            } => datasize::data_size(pending_seq) + recv_buf_byte_size(recv_buf),
        }
    }
}
// END

fn recv_seq_buf_byte_size(value: &AHashSet<MessageSeq>) -> usize {
    std::mem::size_of_val(value) + datasize::data_size(&value)
}

fn recv_buf_byte_size(value: &AHashMap<MessageSeq, Bytes>) -> usize {
    std::mem::size_of_val(value)
        + value
            .iter()
            .map(|(_, buf)| crate::util::bytes_data_size(buf))
            .sum::<usize>()
}

/// Minimum number of bytes of overhead in a packet produced by
/// [`Session::flush`].
pub const OVERHEAD: usize = PacketHeader::ENCODE_LEN + FragmentHeader::ENCODE_LEN + 1;

impl Session {
    /// Creates a new session from the given configuration.
    ///
    /// See [`Session`] for an explanation of MTU.
    ///
    /// If you are unsure for the value of `min_mtu`, you should go for a
    /// conservative estimate. [RFC 9000 Section 14.2], the specification behind
    /// the QUIC transport, requires that a connection supports an MTU of at
    /// least 1200.
    ///
    /// `initial_mtu` may be the same as `min_mtu` if you are unsure.
    ///
    /// # Errors
    ///
    /// Errors if `min_mtu` or `initial_mtu` are too small.
    ///
    /// [RFC 9000 Section 14.2]: https://www.rfc-editor.org/rfc/rfc9000.html#section-14-2
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
            sent_msgs: AHashMap::new(),
            flushed_packets: AHashMap::new(),
            acks: Acknowledge::new(),
            max_memory_usage: config.max_memory_usage,

            // send
            send_lanes: config
                .send_lanes
                .into_iter()
                .map(|config| SendLane {
                    bytes_left: ByteBucket::new(config.send_bytes_per_sec),
                    kind: match config.kind {
                        LaneKind::UnreliableUnordered | LaneKind::UnreliableSequenced => {
                            SendLaneKind::Unreliable
                        }
                        LaneKind::ReliableUnordered | LaneKind::ReliableOrdered => {
                            SendLaneKind::Reliable {
                                resend_after: config.resend_after,
                            }
                        }
                    },
                })
                .collect(),
            send_frags: FragmentSender::new(max_payload_len),
            min_mtu,
            mtu: initial_mtu,
            bytes_left: ByteBucket::new(config.send_bytes_per_sec),
            next_msg_seq: MessageSeq::default(),
            next_packet_seq: PacketSeq::default(),
            next_keep_alive_at: now,
            bytes_sent: 0,

            // recv
            recv_lanes: config
                .recv_lanes
                .into_iter()
                .map(|config| match config.kind {
                    LaneKind::UnreliableUnordered => RecvLane::UnreliableUnordered,
                    LaneKind::UnreliableSequenced => RecvLane::UnreliableSequenced {
                        pending_seq: MessageSeq::default(),
                    },
                    LaneKind::ReliableUnordered => RecvLane::ReliableUnordered {
                        pending_seq: MessageSeq::default(),
                        recv_seq_buf: AHashSet::new(),
                    },
                    LaneKind::ReliableOrdered => RecvLane::ReliableOrdered {
                        pending_seq: MessageSeq::default(),
                        recv_buf: AHashMap::new(),
                    },
                })
                .collect(),
            recv_frags: FragmentReceiver::new(max_payload_len),
            bytes_recv: 0,
            rtt: RttEstimator::new(INITIAL_RTT),
        })
    }

    /// Gets the number of bytes sent over the lifetime of this session.
    ///
    /// When calling [`Session::flush`], the length of each packet returned is
    /// added to this value.
    #[must_use]
    pub const fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    /// Gets the number of bytes received over the lifetime of this session.
    ///
    /// When calling [`Session::recv`], the length of the packet is added to
    /// this value.
    #[must_use]
    pub const fn bytes_recv(&self) -> usize {
        self.bytes_recv
    }

    /// Gets the state of the [`RttEstimator`], allowing you to read RTT values.
    #[must_use]
    pub const fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    /// Gets the [`ByteBucket`] used to track how many bytes we have left for
    /// sending.
    #[must_use]
    pub const fn bytes_left(&self) -> &ByteBucket {
        &self.bytes_left
    }

    /// Gets the state of the [`SendLane`]s in this session.
    #[must_use]
    pub const fn send_lanes(&self) -> &[SendLane] {
        &self.send_lanes
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

    /// Sets the MTU of this session.
    ///
    /// See [`Session`] for an explanation of MTU values.
    ///
    /// # Errors
    ///
    /// Errors if `mtu` is less than `min_mtu` as defined on creation.
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

    /// Gets the maximum number of bytes that this session can use to store
    /// buffered messages.
    ///
    /// If this value is exceeded, operations on this session will fail with
    /// [`OutOfMemory`].
    #[must_use]
    pub const fn max_memory_usage(&self) -> usize {
        self.max_memory_usage
    }

    /// Gets the total number of bytes used for buffering messages.
    ///
    /// If this value exceeds [`Session::max_memory_usage`], operations on this
    /// session will fail with [`OutOfMemory`].
    ///
    /// This function is not expensive to call, but do note that it will iterate
    /// over all buffered incoming + outgoing messages, so try to avoid repeated
    /// calls.
    #[must_use]
    pub fn memory_used(&self) -> usize {
        datasize::data_size(self)
    }

    pub fn sent_msgs_mem(&self) -> usize {
        sent_msgs_data_size(&self.sent_msgs)
    }

    pub fn flushed_packets_mem(&self) -> usize {
        flushed_packets_data_size(&self.flushed_packets)
    }

    pub fn recv_lanes_mem(&self) -> usize {
        tracing::trace!("lanes:");
        for lane in self.recv_lanes.iter() {
            tracing::trace!("- {lane:?}");
        }
        datasize::data_size(&self.recv_lanes)
    }

    pub fn recv_frags_mem(&self) -> usize {
        datasize::data_size(&self.recv_frags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOW_PRIO: LaneIndex = LaneIndex::from_raw(0);

    fn config() -> SessionConfig {
        SessionConfig::new(usize::MAX).with_lanes([LaneConfig::new(LaneKind::UnreliableUnordered)])
    }

    fn now() -> Instant {
        Instant::now()
    }

    // todo proper unit tests

    #[test]
    fn round_trip() {
        let mut session = Session::new(now(), config(), 30, 30).unwrap();

        println!("{}", session.bytes_left.get());
        // session.send(now(), b"hi", LOW_PRIO).unwrap();
        session.send(now(), &b"A".repeat(100), LOW_PRIO).unwrap();
        // session.send(now(), b"world", LOW_PRIO).unwrap();
        println!("{}", session.bytes_left.get());

        let packets = session.flush(now()).collect::<Vec<_>>();
        for packet in &packets {
            println!("{packet:?} len = {}", packet.len());
        }
        println!("{}", session.bytes_left.get());

        for packet in packets {
            let (acks, mut msgs) = session.recv(now(), packet).unwrap();

            for ack in acks {
                println!("ack {ack:?}");
            }

            msgs.for_each_msg(|res| {
                let (msg, lane) = res.unwrap();
                println!("{lane:?} > {msg:?}");
            })
            .unwrap();
        }
    }
}
