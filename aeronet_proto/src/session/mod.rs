use std::{collections::hash_map::Entry, convert::Infallible};

use aeronet::lane::{LaneIndex, LaneKind};
use ahash::{AHashMap, AHashSet};
use either::Either;
use octs::{
    Buf, BufTooShortOr, Bytes, BytesMut, EncodeLen, FixedEncodeLen, Read, VarInt, VarIntTooLarge,
    Write,
};
use terrors::OneOf;
use web_time::{Duration, Instant};

use crate::{
    ack::Acknowledge,
    byte_count::{ByteBucket, ByteLimit},
    frag::{
        Fragment, FragmentHeader, FragmentMarker, FragmentReceiver, FragmentSender, MessageTooBig,
        ReassembleError,
    },
    packet::{MessageSeq, PacketHeader, PacketSeq},
    seq::Seq,
};

/*
potential attack vectors:
- peer sends us a lot of incomplete fragments, which we buffer forever, leading
  to OOM
  - we set a memory cap on the recv_frags buffer
  - when we attempt to buffer a new message but we've hit the cap...
    - for unreliable frags: the last message buf to receive a new frag is dropped
    - for reliable frags: the connection is resett
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
    pub recv_lanes: Vec<LaneKind>,
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
            default_packet_cap: 128,
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
        Self {
            kind: LaneKind::UnreliableUnordered,
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
    payload: Bytes,
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
        const MIN_PACKET_LEN: usize = PacketHeader::ENCODE_LEN + FragmentHeader::ENCODE_LEN;

        // use > instead of >= so that we can fit at least 1 byte of payload in
        assert!(config.max_packet_len > MIN_PACKET_LEN);
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
                .map(|kind| match kind {
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

    #[must_use]
    pub const fn bytes_left(&self) -> &ByteBucket {
        &self.bytes_left
    }

    pub fn refill_bytes_exact(&mut self, n: usize) {
        self.bytes_left.refill_exact(n);
        for lane in self.send_lanes.iter_mut() {
            lane.bytes_left.refill_exact(n)
        }
    }

    pub fn refill_bytes_portion(&mut self, f: f32) {
        self.bytes_left.refill_portion(f);
        for lane in self.send_lanes.iter_mut() {
            lane.bytes_left.refill_portion(f)
        }
    }

    pub fn send(
        &mut self,
        now: Instant,
        msg: &[u8],
        lane_index: LaneIndex,
    ) -> Result<MessageSeq, SendError> {
        if self.send_lanes.get(lane_index.into_raw()).is_none() {
            return Err(SendError::InvalidLane);
        }

        // encode the lane index directly into the start of the message payload
        let lane_index_enc = VarInt(lane_index.into_raw());
        let mut buf = BytesMut::with_capacity(lane_index_enc.encode_len() + msg.len());
        buf.write(lane_index_enc).unwrap();
        buf.write_from(msg).unwrap();
        let buf = buf.freeze();

        let msg_seq = self.next_msg_seq;
        let frags = self.send_frags.fragment(msg_seq, buf)?;

        let Entry::Vacant(entry) = self.sent_msgs.entry(msg_seq) else {
            return Err(SendError::TooManyMessages);
        };
        self.next_msg_seq += MessageSeq::new(1);
        entry.insert(SentMessage {
            lane_index,
            frags: frags
                .map(|frag| {
                    Some(SentFragment {
                        payload: frag.payload,
                        next_flush_at: now,
                    })
                })
                .collect(),
        });
        Ok(msg_seq)
    }

    fn get_frag(
        sent_msgs: &AHashMap<MessageSeq, SentMessage>,
        path: FragmentPath,
    ) -> &SentFragment {
        sent_msgs[&path.msg_seq].frags[usize::from(path.index)]
            .as_ref()
            .unwrap()
    }

    pub fn flush(&mut self, now: Instant) -> impl Iterator<Item = Bytes> + '_ {
        // drop any messages which have no frags to send
        self.sent_msgs
            .retain(|_, msg| msg.frags.iter().any(Option::is_some));

        // collect the paths of the fragments to send
        let mut frag_paths = self
            .sent_msgs
            .iter()
            .flat_map(move |(msg_seq, msg)| {
                msg.frags
                    .iter()
                    .filter_map(Option::as_ref)
                    .filter(move |frag| now >= frag.next_flush_at)
                    .enumerate()
                    .map(move |(frag_index, _)| FragmentPath {
                        msg_seq: *msg_seq,
                        index: u8::try_from(frag_index).unwrap(),
                    })
            })
            // wrap in an Option, since we're gonna be taking individual frags out
            // once we've added them to a packet
            .map(Some)
            .collect::<Box<_>>();
        // sort them by payload length, largest to smallest
        frag_paths.sort_unstable_by(|a, b| {
            let a = Self::get_frag(&self.sent_msgs, a.unwrap());
            let b = Self::get_frag(&self.sent_msgs, b.unwrap());
            b.payload.len().cmp(&a.payload.len())
        });

        std::iter::from_fn(move || {
            // this iteration, we want to build up one full packet

            // make a buffer for the packet
            // NOTE: we don't use `max_packet_len`, because that might be a big length
            // e.g. Steamworks already fragments messages, so we don't fragment messages
            // ourselves, leading to very large `max_packet_len`s (~512KiB)
            let mut packet = BytesMut::with_capacity(self.default_packet_cap);

            // we can't put more than either `max_packet_len` or `bytes_left`
            // bytes into this packet, so we track this as well
            let mut bytes_left = (&mut self.bytes_left).min_of(self.max_packet_len);
            let packet_seq = self.next_packet_seq;
            bytes_left.consume(PacketHeader::ENCODE_LEN).ok()?;
            packet
                .write(PacketHeader {
                    packet_seq,
                    acks: self.acks,
                })
                .unwrap();

            // collect the paths of the frags we want to put into this packet
            // so that we can track which ones have been acked later
            let mut frags = Vec::new();
            for frag_path_opt in frag_paths.iter_mut() {
                (|| {
                    let path = frag_path_opt.take()?;
                    let msg = self.sent_msgs.get_mut(&path.msg_seq).unwrap();
                    let num_frags = msg.frags.len();
                    let sent_frag = msg.frags[usize::from(path.index)].as_mut().unwrap();

                    let is_last = usize::from(path.index) == num_frags - 1;
                    let frag = Fragment {
                        header: FragmentHeader {
                            msg_seq: path.msg_seq,
                            marker: FragmentMarker::new(path.index, is_last).unwrap(),
                        },
                        payload: sent_frag.payload.clone(),
                    };

                    // write the payload into the packet
                    // make sure we have enough bytes available in the bucket first though
                    // the lane index is encoded in `sent_frag.payload` itself, done in `send`
                    let lane = &mut self.send_lanes[msg.lane_index.into_raw()];
                    let mut bytes_left = (&mut bytes_left).min_of(&mut lane.bytes_left);
                    bytes_left.consume(frag.encode_len()).ok()?;
                    packet.write(&frag).unwrap();

                    // how does the lane want to handle this?
                    match &lane.kind {
                        SendLaneKind::Unreliable => {
                            // drop the frag
                            // if we've dropped all frags of this message, then
                            // on the next `flush`, we'll drop the message
                            *frag_path_opt = None;
                        }
                        SendLaneKind::Reliable { resend_after } => {
                            // don't drop the frag, just attempt to resend it later
                            // it'll be dropped when the peer acks it
                            sent_frag.next_flush_at = now + *resend_after;
                        }
                    }

                    frags.push(path);
                    Some(())
                })();
            }

            if frags.is_empty() {
                // we couldn't write any fragments - no more packets to send
                None
            } else {
                // we wrote at least one fragment - we can send this packet
                // and track what fragments we're sending in this packet
                self.next_packet_seq += PacketSeq::new(1);
                self.flushed_packets.insert(
                    packet_seq,
                    FlushedPacket {
                        frags: frags.into_boxed_slice(),
                    },
                );
                Some(packet.freeze())
            }
        })
    }

    pub fn recv(
        &mut self,
        now: Instant,
        mut packet: Bytes,
    ) -> Result<
        (
            impl Iterator<Item = MessageSeq> + '_,
            impl Iterator<Item = Result<Bytes, OneOf<(RecvError, OutOfMemory)>>> + '_,
        ),
        RecvError,
    > {
        let header = packet
            .read::<PacketHeader>()
            .map_err(RecvError::ReadHeader)?;
        self.acks.ack(header.packet_seq);

        let acks = Self::recv_acks(
            &mut self.flushed_packets,
            &mut self.sent_msgs,
            header.acks.seqs(),
        );
        let msgs = Self::recv_msgs(
            now,
            &mut self.recv_lanes,
            &mut self.recv_frags,
            self.recv_frags_cap,
            packet,
        );
        Ok((acks, msgs))
    }

    fn recv_acks<'a>(
        flushed_packets: &'a mut AHashMap<PacketSeq, FlushedPacket>,
        sent_msgs: &'a mut AHashMap<MessageSeq, SentMessage>,
        acked_seqs: impl Iterator<Item = PacketSeq> + 'a,
    ) -> impl Iterator<Item = MessageSeq> + 'a {
        acked_seqs
            // we now know that our packet with sequence `seq` was acked by the peer
            // let's find what fragments that packet contained when we flushed it out
            .filter_map(|seq| flushed_packets.remove(&seq))
            // TODO Rust 1.80: Box::into_iter - https://github.com/rust-lang/rust/issues/59878
            .flat_map(|packet| packet.frags.into_vec().into_iter())
            .filter_map(|frag_path| {
                // for each of those fragments, we'll mark that fragment as acked
                let msg = sent_msgs.get_mut(&frag_path.msg_seq)?;
                let frag_opt = msg.frags.get_mut(usize::from(frag_path.index))?;
                // mark this fragment as acked, and stop it from being resent
                *frag_opt = None;

                // if all the fragments are now acked, then we report that
                // the entire message is now acked
                if msg.frags.iter().all(Option::is_none) {
                    Some(frag_path.msg_seq)
                } else {
                    None
                }
            })
    }

    fn recv_msgs<'a>(
        now: Instant,
        recv_lanes: &'a mut [RecvLane],
        recv_frags: &'a mut FragmentReceiver,
        recv_frags_cap: usize,
        mut packet: Bytes,
    ) -> impl Iterator<Item = Result<Bytes, OneOf<(RecvError, OutOfMemory)>>> + 'a {
        // this would be so much easier with coroutines...
        std::iter::from_fn(move || {
            if !packet.has_remaining() {
                return None;
            }

            let res =
                Self::recv_next_frag(now, recv_lanes, recv_frags, recv_frags_cap, &mut packet);
            Some(todo!())
            // Some(
            //     match res {
            //         Ok(iter) => Either::Left(iter.map(Ok)),
            //         Err(err) => Either::Right(std::iter::once(Err(err))),
            //     }
            //     .into_iter(),
            // )
        });
        std::iter::empty() // todo
    }

    fn recv_next_frag<'a>(
        now: Instant,
        recv_lanes: &'a mut [RecvLane],
        recv_frags: &'a mut FragmentReceiver,
        recv_frags_cap: usize,
        packet: &'a mut Bytes,
    ) -> Result<impl Iterator<Item = Bytes> + 'a, OneOf<(RecvError, OutOfMemory)>> {
        let frag = packet
            .read::<Fragment>()
            .map_err(RecvError::ReadFragment)
            .map_err(|err| OneOf::from(err).broaden())?;
        let msg_seq = frag.header.msg_seq;
        let Some(mut msg) = recv_frags
            .reassemble_frag(now, frag)
            .map_err(RecvError::Reassemble)
            .map_err(|err| OneOf::from(err).broaden())?
        else {
            return Ok(Either::Left(std::iter::empty::<Bytes>()));
        };

        if recv_frags.bytes_used() > recv_frags_cap {
            return Err(OneOf::from(OutOfMemory).broaden());
        }

        let lane_index = msg
            .read::<VarInt<usize>>()
            .map_err(RecvError::ReadLaneIndex)
            .map_err(|err| OneOf::from(err).broaden())?
            .0;
        let lane = recv_lanes
            .get_mut(lane_index)
            .ok_or(RecvError::InvalidLaneIndex { index: lane_index })
            .map_err(|err| OneOf::from(err).broaden())?;

        Ok(Either::Right(Self::recv_on_lane(lane, msg, msg_seq)))
    }

    fn recv_on_lane(
        lane: &mut RecvLane,
        msg: Bytes,
        msg_seq: MessageSeq,
    ) -> impl Iterator<Item = Bytes> + '_ {
        match lane {
            RecvLane::UnreliableUnordered => {
                // always just return the message
                Either::Left(Some(msg))
            }
            RecvLane::UnreliableSequenced { pending_seq } => {
                if msg_seq < *pending_seq {
                    // msg is older than the message we're expecting to get next, drop it
                    Either::Left(None)
                } else {
                    // msg is the one we're expecting to get or newer, return it
                    *pending_seq = msg_seq + MessageSeq::new(1);
                    Either::Left(Some(msg))
                }
            }
            RecvLane::ReliableUnordered {
                pending_seq,
                recv_seq_buf,
            } => {
                if msg_seq < *pending_seq {
                    // msg is guaranteed to already be received, drop it
                    Either::Left(None)
                } else {
                    // here's an example to visualize what this does:
                    // msg_seq: 40
                    // pending_seq: 40, recv_seq_buf: [41, 45]
                    recv_seq_buf.insert(msg_seq);
                    // pending_seq: 40, recv_seq_buf: [40, 41, 45]
                    while recv_seq_buf.remove(pending_seq) {
                        *pending_seq += MessageSeq::new(1);
                        // iter 1: pending_seq: 41, recv_seq_buf: [41, 45]
                        // iter 2: pending_seq: 42, recv_seq_buf: [45]
                    }
                    Either::Left(Some(msg))
                }
            }
            RecvLane::ReliableOrdered {
                pending_seq,
                recv_buf,
            } => {
                if msg_seq < *pending_seq {
                    // msg is guaranteed to already be received, drop it
                    Either::Left(None)
                } else {
                    // almost identical to above, but we also return the
                    // messages that we remove
                    recv_buf.insert(msg_seq, msg);
                    Either::Right(std::iter::from_fn(move || {
                        let msg = recv_buf.remove(pending_seq)?;
                        *pending_seq += MessageSeq::new(1);
                        Some(msg)
                    }))
                }
            }
        }
        .into_iter()
    }
}
