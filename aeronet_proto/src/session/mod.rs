//! See [`Session`].

mod recv;
mod send;

pub use recv::*;

use std::convert::Infallible;

use aeronet::lane::{LaneIndex, LaneKind};
use ahash::{AHashMap, AHashSet};
use octs::{BufTooShortOr, Bytes, FixedEncodeLen, VarIntTooLarge};
use web_time::{Duration, Instant};

use crate::{
    ack::Acknowledge,
    byte_count::ByteBucket,
    frag::{
        Fragment, FragmentHeader, FragmentReceiver, FragmentSender, MessageTooBig, ReassembleError,
    },
    packet::{MessageSeq, PacketHeader, PacketSeq},
};

/*
potential attack vectors:
- peer sends us a lot of incomplete fragments, which we buffer forever, leading
  to OOM
  - we set a memory cap on the recv_frags buffer
  - when we attempt to buffer a new message but we've hit the cap...
    - for unreliable frags: the last message buf to receive a new frag is dropped
    - for reliable frags: the connection is reset
- peer never sends acks for our packets
  - we keep reliable frags around forever constantly trying to resend them,
    leading to OOM
  - solution: ??? idk i need to figure this out
 */

/// Manages the state of a data transport session between two peers.
///
/// To use the aeronet protocol, this is the main type you should be interacting
/// with when receiving or sending out byte messages.
#[derive(Debug)]
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
    /// If a peer never sends acks, **TODO! This may OOM our side!**
    flushed_packets: AHashMap<PacketSeq, FlushedPacket>,
    /// Tracks which packets we have acknowledged from the peer.
    acks: Acknowledge,

    // send
    /// Allows splitting a message into smaller fragments.
    send_frags: FragmentSender,
    /// Outgoing lane state.
    send_lanes: Box<[SendLane]>,
    /// Default byte buffer capacity to allocate when flushing packets.
    default_packet_cap: usize,
    /// Maximum length of a single flushed packet.
    max_packet_len: usize,
    /// Tracks how many bytes remaining we have to send to our peer.
    ///
    /// This should be filled up by the user using [`Session::refill_bytes`].
    bytes_left: ByteBucket,
    /// Next outgoing message sequence.
    next_msg_seq: MessageSeq,
    /// Next outgoing packet sequence.
    next_packet_seq: PacketSeq,

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
    /// `recv_frags_cap`. If we receive a fragment but do not have enough
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
    /// Maximum number of bytes that `recv_frags` is allowed to use to buffer
    /// incomplete messages.
    recv_frags_cap: usize,
}

/// Configuration for a [`Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    /// Configurations for the lanes which can be used to send out data.
    pub send_lanes: Vec<LaneConfig>,
    /// Configurations for the lanes which can be used to receive data.
    pub recv_lanes: Vec<LaneConfig>,
    /// Default packet capacity for new packets created in [`Session::flush`].
    ///
    /// When we start building up a packet to flush out, we may pre-allocate
    /// some space for the bytes. This value determines how many bytes we
    /// pre-allocate.
    ///
    /// You may keep this at 0 as a reasonable default.
    pub default_packet_cap: usize,
    /// Maximum length of a packet that is returned in [`Session::flush`].
    ///
    /// Often, transports may set a hard limit on how long packets may be (e.g.
    /// if a UDP datagram is too large, peers may just drop the datagram
    /// entirely). To get around this, you may set a hard limit on the size of
    /// a packet, and the [`Session`] will never produce packets larger than
    /// this size.
    ///
    /// If a transport can accurately identify the maximum size of a packet that
    /// it can send, it should use that value here, and override any user-given
    /// value.
    pub max_packet_len: usize,
    /// How many total bytes we can [`Session::flush`] out per second.
    ///
    /// When flushing, if we do not have enough bytes to send out any more
    /// packets, we will stop returning any packets. You must remember to call
    /// [`Session::refill_bytes`] in your update loop to refill this!
    pub send_bytes_per_sec: usize,
    /// Maximum number of bytes of memory which can be used for receiving
    /// fragments from the peer.
    ///
    /// A malicious peer may send us an infinite amount of fragments which
    /// never get fully reassembled, leaving us having to buffer up all of their
    /// fragments. We are not allowed to drop any fragments since they may be
    /// part of a reliable message, in which case dropping breaks the guarantees
    /// of the lane (we don't know if a fragment is part of a reliable or
    /// unreliable message until we fully reassemble it).
    ///
    /// To avoid running out of memory, if we attempt to buffer more than this
    /// amount of bytes when receiving fragments, the connection will be
    /// forcibly reset by emitting an [`OutOfMemory`].
    pub max_recv_memory_usage: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            send_lanes: Vec::new(),
            recv_lanes: Vec::new(),
            default_packet_cap: 0,
            max_packet_len: 1024,
            send_bytes_per_sec: usize::MAX,
            max_recv_memory_usage: usize::MAX,
        }
    }
}

/// Configuration for a lane in a [`Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneConfig {
    /// Kind of lane that this creates.
    pub kind: LaneKind,
    /// For send lanes: how many total bytes we can [`Session::flush`] out on
    /// this lane per second.
    ///
    /// See [`SessionConfig::send_bytes_per_sec`].
    pub send_bytes_per_sec: usize,
    /// For reliable send lanes: after flushing out a fragment, how long do we
    /// wait until attempting to flush out this fragment again?
    ///
    /// If last update we flushed out a fragment of a reliable message, then it
    /// would be pointless to flush out the same fragment on this update, since
    /// [RTT] probably hasn't even elapsed yet, and there's no way the peer
    /// could have acknowledged it yet.
    ///
    /// [RTT]: aeronet::stats::Rtt
    // TODO: could we make this automatic and base it on RTT? i.e. if RTT is
    // 100ms, then we set resend_after to 100ms by default, and if RTT changes,
    // then we re-adjust it
    pub resend_after: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self::new(LaneKind::UnreliableUnordered)
    }
}

impl LaneConfig {
    /// Creates a new default lane configuration for the given lane kind.
    #[must_use]
    pub const fn new(kind: LaneKind) -> Self {
        Self {
            kind,
            send_bytes_per_sec: usize::MAX,
            resend_after: Duration::from_millis(100),
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FragmentPath {
    msg_seq: MessageSeq,
    index: u8,
}

/// State of a [`Session`] lane used for sending messages.
#[derive(Debug)]
pub struct SendLane {
    /// Tracks how many bytes we have left for sending along this lane.
    pub bytes_left: ByteBucket,
    kind: SendLaneKind,
}

#[derive(Debug)]
enum SendLaneKind {
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
    frag: Fragment,
    next_flush_at: Instant,
}

#[derive(Debug)]
struct FlushedPacket {
    frags: Box<[FragmentPath]>,
}

#[derive(Debug)]
enum RecvLane {
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
        recv_buf: AHashMap<MessageSeq, Bytes>,
    },
}

/// Minimum length of a packet emitted by [`Session::flush`].
pub const MIN_PACKET_LEN: usize = PacketHeader::ENCODE_LEN + FragmentHeader::ENCODE_LEN + 1;

impl Session {
    /// Creates a new session from the given configuration.
    ///
    /// # Panics
    ///
    /// Panics if [`SessionConfig::max_packet_len`] is less than
    /// [`MIN_PACKET_LEN`].
    #[must_use]
    pub fn new(config: SessionConfig) -> Self {
        assert!(config.max_packet_len >= MIN_PACKET_LEN);
        let max_payload_len = config.max_packet_len - MIN_PACKET_LEN;
        Self {
            sent_msgs: AHashMap::new(),
            flushed_packets: AHashMap::new(),
            acks: Acknowledge::new(),
            send_frags: FragmentSender::new(max_payload_len),
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
            default_packet_cap: config.default_packet_cap,
            max_packet_len: config.max_packet_len,
            bytes_left: ByteBucket::new(config.send_bytes_per_sec),
            next_msg_seq: MessageSeq::default(),
            next_packet_seq: PacketSeq::default(),
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
            recv_frags_cap: config.max_recv_memory_usage,
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOW_PRIO: LaneIndex = LaneIndex::from_raw(0);

    fn config() -> SessionConfig {
        let lanes = vec![LaneConfig::new(LaneKind::UnreliableUnordered)];
        SessionConfig {
            send_lanes: lanes.clone(),
            recv_lanes: lanes,
            default_packet_cap: 0,
            max_packet_len: 30,
            send_bytes_per_sec: usize::MAX,
            max_recv_memory_usage: usize::MAX,
        }
    }

    fn now() -> Instant {
        Instant::now()
    }

    // todo proper unit tests

    #[test]
    fn round_trip() {
        let mut session = Session::new(config());

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
