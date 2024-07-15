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
    /// This should be filled up by the user using the `refill_bytes_*`
    /// functions.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    pub send_lanes: Vec<LaneConfig>,
    pub recv_lanes: Vec<LaneConfig>,
    pub default_packet_cap: usize,
    pub max_packet_len: usize,
    pub send_cap: usize,
    pub recv_frags_cap: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            send_lanes: Vec::new(),
            recv_lanes: Vec::new(),
            default_packet_cap: 0,
            max_packet_len: 1024,
            send_cap: usize::MAX,
            recv_frags_cap: usize::MAX,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneConfig {
    pub kind: LaneKind,
    pub send_cap: usize,
    pub resend_after: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self::new(LaneKind::UnreliableUnordered)
    }
}

impl LaneConfig {
    pub const fn new(kind: LaneKind) -> Self {
        Self {
            kind,
            send_cap: usize::MAX,
            resend_after: Duration::from_millis(100),
        }
    }
}

/// Error when attempting to buffer a message for sending using
/// [`Session::send`].
///
/// If sending an [unreliable] message, it is safe to ignore this.
///
/// If sending a [reliable] message, an error must close the session.
///
/// [unreliable]: aeronet::lane::LaneReliability::Unreliable
/// [reliable]: aeronet::lane::LaneReliability::Reliable
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
    #[error("failed to read header")]
    ReadHeader(#[source] BufTooShortOr<Infallible>),
    #[error("failed to read fragment")]
    ReadFragment(#[source] BufTooShortOr<VarIntTooLarge>),
    #[error("failed to reassemble fragment")]
    Reassemble(#[source] ReassembleError),
    #[error("failed to read lane index")]
    ReadLaneIndex(#[source] BufTooShortOr<VarIntTooLarge>),
    #[error("invalid lane index `{index}`")]
    InvalidLaneIndex { index: usize },
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("out of memory")]
pub struct OutOfMemory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FragmentPath {
    msg_seq: MessageSeq,
    index: u8,
}

#[derive(Debug)]
struct SendLane {
    bytes_left: ByteBucket,
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

impl Session {
    pub fn new(config: SessionConfig) -> Self {
        // so that we can store, at minimum, the packet header and at least 1 fragment,
        // we need to set a [minimum [maximum packet len]] (confusing, I know)
        const MIN_PACKET_LEN: usize = PacketHeader::ENCODE_LEN + FragmentHeader::ENCODE_LEN + 1;

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
                    bytes_left: ByteBucket::new(config.send_cap),
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
            bytes_left: ByteBucket::new(config.send_cap),
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
            recv_frags_cap: config.recv_frags_cap,
        }
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
            send_cap: usize::MAX,
            recv_frags_cap: usize::MAX,
        }
    }

    fn now() -> Instant {
        Instant::now()
    }

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
