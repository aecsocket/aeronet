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
    /// See [`SessionConfig::keep_alive_interval`].
    keep_alive_interval: Duration,
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
    /// Total number of bytes received.
    bytes_recv: usize,
}

/// Configuration for a [`Session`].
///
/// Not all session-specific configurations are exposed here. Transport-specific
/// settings such as maximum packet length are not exposed to users, and are
/// instead set directly when calling [`Session::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    /// Configurations for the lanes which can be used to send out data.
    pub send_lanes: Vec<LaneConfig>,
    /// Configurations for the lanes which can be used to receive data.
    pub recv_lanes: Vec<LaneConfig>,
    /// Maximum number of bytes of memory which can be used for receiving
    /// fragments from the peer.
    ///
    /// The default is 0. You **must** either use [`SessionConfig::new`] or
    /// override this value explicitly, otherwise your session will always
    /// error with [`OutOfMemory`] when [`Session::recv`]'ing!
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
    /// How many total bytes we can [`Session::flush`] out per second.
    ///
    /// This value is [`usize::MAX`] by default.
    ///
    /// When flushing, if we do not have enough bytes to send out any more
    /// packets, we will stop returning any packets. You must remember to call
    /// [`Session::refill_bytes`] in your update loop to refill this!
    pub send_bytes_per_sec: usize,
    /// How long to wait since sending the last packet until we send an empty
    /// keep-alive packet.
    ///
    /// Even if we've got no messages to transmit, we need to send packets to
    /// the peer regularly because:
    /// - we need to keep the underlying connection alive
    /// - we need to transmit packet acknowledgements
    ///
    /// This means that any transport-specific keep-alive mechanism should be
    /// disabled, since the [`Session`] will handle it.
    // TODO can this interval be automated?
    pub keep_alive_interval: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            send_lanes: Vec::new(),
            recv_lanes: Vec::new(),
            max_recv_memory_usage: 0,
            send_bytes_per_sec: usize::MAX,
            keep_alive_interval: Duration::from_millis(500),
        }
    }
}

impl SessionConfig {
    /// Creates a new configuration with the default values set, apart from
    /// [`SessionConfig::max_recv_memory_usage`], which must be manually
    /// defined.
    #[must_use]
    pub fn new(max_recv_memory_usage: usize) -> Self {
        Self {
            max_recv_memory_usage,
            ..Default::default()
        }
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::send_lanes`].
    ///
    /// You can implement `From<LaneConfig> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_send_lanes(
        mut self,
        lanes: impl IntoIterator<Item = impl Into<LaneConfig>>,
    ) -> Self {
        self.send_lanes.extend(lanes.into_iter().map(Into::into));
        self
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::recv_lanes`].
    ///
    /// You can implement `From<LaneConfig> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_recv_lanes(
        mut self,
        lanes: impl IntoIterator<Item = impl Into<LaneConfig>>,
    ) -> Self {
        self.recv_lanes.extend(lanes.into_iter().map(Into::into));
        self
    }

    /// Adds the given lanes to this configuration's
    /// [`SessionConfig::send_lanes`] and [`SessionConfig::recv_lanes`].
    ///
    /// You can implement `From<LaneConfig> for [your own type]` to use it as
    /// the item in this iterator.
    #[must_use]
    pub fn with_lanes(mut self, lanes: impl IntoIterator<Item = impl Into<LaneConfig>>) -> Self {
        let lanes = lanes.into_iter().map(Into::into).collect::<Vec<_>>();
        self.send_lanes.extend(lanes.iter().cloned());
        self.recv_lanes.extend(lanes.iter().cloned());
        self
    }

    /// Sets [`SessionConfig::send_bytes_per_sec`] on this value.
    #[must_use]
    pub const fn with_send_bytes_per_sec(mut self, send_bytes_per_sec: usize) -> Self {
        self.send_bytes_per_sec = send_bytes_per_sec;
        self
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

/// Minimum number of bytes of overhead in a packet produced by
/// [`Session::flush`].
pub const OVERHEAD: usize = PacketHeader::ENCODE_LEN + FragmentHeader::ENCODE_LEN + 1;

impl Session {
    /// Creates a new session from the given configuration.
    ///
    /// # Errors
    ///
    /// Errors if `min_mtu` or `initial_mtu` are too small.
    ///
    /// See [`Session`].
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

            // send
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
            min_mtu,
            mtu: initial_mtu,
            bytes_left: ByteBucket::new(config.send_bytes_per_sec),
            next_msg_seq: MessageSeq::default(),
            next_packet_seq: PacketSeq::default(),
            next_keep_alive_at: now,
            keep_alive_interval: config.keep_alive_interval,
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
            recv_frags_cap: config.max_recv_memory_usage,
            bytes_recv: 0,
        })
    }

    /// Gets the nubmer of bytes sent over the lifetime of this session.
    ///
    /// When calling [`Session::flush`], the length of each packet returned is
    /// added to this value.
    #[must_use]
    pub const fn bytes_sent(&self) -> usize {
        self.bytes_sent
    }

    /// Gets the nubmer of bytes received over the lifetime of this session.
    ///
    /// When calling [`Session::recv`], the length of the packet is added to
    /// this value.
    #[must_use]
    pub const fn bytes_recv(&self) -> usize {
        self.bytes_recv
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
