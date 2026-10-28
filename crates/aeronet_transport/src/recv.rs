//! Receiving logic for [`Transport`]s.

use {
    crate::{
        FlushedPacket, MessageKey, RecvMessage, Transport, TransportConfig,
        frag::{FragmentReceiver, ReassembleError},
        lane::{LaneIndex, LaneKind, LaneReliability},
        packet::{Fragment, MessageSeq, PacketHeader, PacketSeq},
        rtt::RttEstimator,
        send::TransportSend,
        seq_buf::SeqBuf,
    },
    aeronet_io::{
        Session,
        connection::{DisconnectReason, Disconnected},
    },
    alloc::{boxed::Box, vec::Vec},
    bevy_ecs::prelude::*,
    bevy_platform::{
        collections::{HashMap, HashSet},
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    },
    core::{iter, num::Saturating},
    derive_more::{Display, Error},
    either::Either,
    log::{trace, warn},
    octs::{Buf, Read},
};

/// Access to the receiving half of a [`Transport`].
#[derive(Debug)]
pub struct TransportRecv {
    lanes: Box<[RecvLane]>,
    /// Payload bytes retained in reassembly and ordered-delivery buffers.
    ///
    /// For tracking allocated memory sizes, we have 2 counters: 1 here, and 1
    /// in the `RecvBuffer`, representing memory used in different stages of
    /// the packet process lifecycle. When memory is transferred from one stage
    /// to the other, we subtract one counter and add the delta to the next
    /// counter. To read the total memory usage, just sum the two.
    pending_payload_size: usize,
    /// Buffer of received messages.
    ///
    /// This must be drained by the user on every update.
    pub msgs: RecvBuffer<RecvMessage>,
    /// Buffer of received message acknowledgements for messages previously
    /// sent via [`TransportSend::push`].
    ///
    /// This must be drained by the user on every update.
    ///
    /// [`TransportSend::push`]: crate::send::TransportSend::push
    pub acks: RecvBuffer<MessageKey>,
}

/// Buffer storing data received by a [`Transport`].
///
/// This is effectively a wrapper around [`Vec`] which only publicly allows
/// draining elements from it.
#[derive(Debug)]
pub struct RecvBuffer<T> {
    items: Vec<T>,
    // used on `recv.msgs`; unused on `recv.acks` for simplicity
    queued_payload_size: usize,
}

impl TransportRecv {
    fn unreliable_eviction_candidates(
        &self,
        protected_lane: usize,
        protected_seq: MessageSeq,
    ) -> Vec<(usize, MessageSeq, Instant)> {
        let mut candidates = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, lane)| lane.kind().reliability() == LaneReliability::Unreliable)
            .flat_map(|(index, lane)| {
                let protected = (index == protected_lane).then_some(protected_seq);
                lane.frags
                    .eviction_candidates(protected)
                    .map(move |(seq, at)| (index, seq, at))
            })
            .collect::<Vec<_>>();
        candidates.sort_unstable_by_key(|(_, _, at)| *at);
        candidates
    }

    pub(crate) fn new(lanes: impl IntoIterator<Item = impl Into<LaneKind>>) -> Self {
        Self {
            pending_payload_size: 0,
            lanes: lanes
                .into_iter()
                .map(Into::into)
                .map(RecvLane::new)
                .collect(),
            msgs: RecvBuffer {
                items: Vec::new(),
                queued_payload_size: 0,
            },
            acks: RecvBuffer {
                items: Vec::new(),
                queued_payload_size: 0,
            },
        }
    }

    #[must_use]
    pub(crate) const fn memory_used(&self) -> usize {
        self.pending_payload_size
            .checked_add(self.msgs.queued_payload_size)
            .expect("receive payload lengths fit in usize")
    }

    /// Gets access to the state of the receiving-side lanes.
    #[must_use]
    pub const fn lanes(&self) -> &[RecvLane] {
        &self.lanes
    }
}

impl<T> RecvBuffer<T> {
    /// Drains all items from this buffer.
    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.queued_payload_size = 0;
        self.items.drain(..)
    }
}

/// State of a lane used for receiving incoming messages on a [`Transport`].
#[derive(Debug, Clone)]
pub struct RecvLane {
    frags: FragmentReceiver,
    state: LaneState,
}

#[derive(Debug, Clone)]
enum LaneState {
    UnreliableUnordered,
    UnreliableSequenced {
        pending: MessageSeq,
    },
    ReliableUnordered {
        pending: MessageSeq,
        recv_buf: HashSet<MessageSeq>,
    },
    ReliableOrdered {
        pending: MessageSeq,
        recv_buf: HashMap<MessageSeq, Vec<u8>>,
    },
}

impl RecvLane {
    fn new(kind: LaneKind) -> Self {
        Self {
            frags: FragmentReceiver::default(),
            state: match kind {
                LaneKind::UnreliableUnordered => LaneState::UnreliableUnordered,
                LaneKind::UnreliableSequenced => LaneState::UnreliableSequenced {
                    pending: MessageSeq::default(),
                },
                LaneKind::ReliableUnordered => LaneState::ReliableUnordered {
                    pending: MessageSeq::default(),
                    recv_buf: HashSet::default(),
                },
                LaneKind::ReliableOrdered => LaneState::ReliableOrdered {
                    pending: MessageSeq::default(),
                    recv_buf: HashMap::default(),
                },
            },
        }
    }

    /// Gets what kind of lane this state represents.
    #[must_use]
    pub const fn kind(&self) -> LaneKind {
        match self.state {
            LaneState::UnreliableUnordered => LaneKind::UnreliableUnordered,
            LaneState::UnreliableSequenced { .. } => LaneKind::UnreliableSequenced,
            LaneState::ReliableUnordered { .. } => LaneKind::ReliableUnordered,
            LaneState::ReliableOrdered { .. } => LaneKind::ReliableOrdered,
        }
    }

    /// Gets the number of messages which are currently being reassembled on
    /// this lane, but have not been fully reassembled yet.
    #[must_use]
    pub fn num_reassembling_msgs(&self) -> usize {
        self.frags.len()
    }

    /// Gets the number of messages which have been received and fully
    /// reassembled, but have not been forwarded to the user yet because some
    /// previous message has not been received yet.
    #[must_use]
    pub fn num_unordered_msgs(&self) -> usize {
        match &self.state {
            LaneState::UnreliableUnordered | LaneState::UnreliableSequenced { .. } => 0,
            LaneState::ReliableUnordered { recv_buf, .. } => recv_buf.len(),
            LaneState::ReliableOrdered { recv_buf, .. } => recv_buf.len(),
        }
    }
}

/// Clears all [`TransportRecv::msgs`] and [`TransportRecv::acks`] buffers,
/// emitting warnings if there were any items left in the buffers.
///
/// The equivalent for [`Transport::send`] does not exist, because the transport
/// layer itself is responsible for draining that buffer.
pub fn clear_buffers(mut sessions: Query<(Entity, &mut Transport)>) {
    for (entity, mut transport) in &mut sessions {
        let len = transport.recv.msgs.items.len();
        if len > 0 {
            warn!(
                "{entity} has {len} received messages which have not been consumed - this \
                 indicates a bug in code above the transport layer"
            );
            drop(transport.recv.msgs.drain());
        }

        let len = transport.recv.acks.items.len();
        if len > 0 {
            warn!(
                "{entity} has {len} received acks which have not been consumed - this indicates a \
                 bug in code above the transport layer"
            );
            transport.recv.acks.items.clear();
        }
    }
}

pub(crate) fn poll(
    mut commands: Commands,
    mut sessions: Query<(Entity, &mut Session, &mut Transport, &TransportConfig)>,
) {
    for (entity, mut session, mut transport, config) in &mut sessions {
        for packet in session.recv.drain(..) {
            if let Err(err) = recv_on(&mut transport, config, packet.recv_at, &packet.payload) {
                warn!("{entity} received invalid packet, disconnecting: {err:?}");
                commands.trigger(Disconnected {
                    entity,
                    reason: DisconnectReason::by_error(err),
                });
                break;
            }
        }
    }
}

/// Why a packet could not be received by a [`Transport`].
#[derive(Debug, Display, Error)]
pub enum RecvError {
    /// Packet was too short to contain a packet header.
    #[display("not enough bytes to read header")]
    ReadHeader,
    /// Packet was too short to contain a fragment header.
    #[display("not enough bytes to read fragment")]
    ReadFragment,
    /// Packet specified that it is a message on a lane that we don't have.
    #[display("invalid lane {lane:?}")]
    InvalidLane {
        /// Lane that the message claims it was sent on.
        lane: LaneIndex,
    },
    /// Failed to reassemble the packet's fragments into a message.
    #[display("failed to reassemble fragment")]
    Reassemble(ReassembleError),
}

/// Forces a [`Transport`] to receive a packet, attempting to decode it into a
/// message and buffer it.
///
/// This function is advanced and has the potential to screw up the transport
/// state - only use it if you know what you're doing!
///
/// Every update, for all [`Session`] with an associated [`Transport`], the
/// session's buffered received packets are passed to this function along with
/// the paired transport.
///
/// # Errors
///
/// Errors if the packet was malformed or invalid in such a way that the session
/// should not be allowed to continue. Errors are fatal.
pub fn recv_on(
    transport: &mut Transport,
    config: &TransportConfig,
    recv_at: Instant,
    mut packet: &[u8],
) -> Result<(), RecvError> {
    trace!("Receiving packet of length {}", packet.len());

    let header = packet
        .read::<PacketHeader>()
        .map_err(|_| RecvError::ReadHeader)?;

    trace!(
        "Received packet header with sequence {} ({} bytes left)",
        header.seq.0.0,
        packet.len()
    );

    let frags = recv_frags_on(transport, config, recv_at, packet);
    let mut frag_index = Saturating(0);
    let mut frags_recv = Saturating(0);
    for result in frags {
        let bytes_left = result?;
        frags_recv += 1;
        trace!("Successfully received fragment {frag_index} ({bytes_left} bytes left)");
        frag_index += 1;
    }

    // only acknowledge this packet once we're sure that we've received all the
    // fragments this packet contains (and there are no more fallible paths in
    // this function), otherwise we've violated our reliability guarantee :(
    transport.peer_acks.ack(header.seq);
    for seq in header.acks.seqs() {
        transport.packet_loss.ack(seq);
    }
    transport.recv.acks.items.extend(packet_acks_to_msg_keys(
        &mut transport.flushed_packets,
        &transport.flushed_packets_size,
        &mut transport.send,
        &mut transport.rtt,
        &mut transport.packet_acks_recv,
        &mut transport.msg_acks_recv,
        recv_at,
        header.acks.seqs(),
    ));

    trace!(
        "Finished receiving packet; successfully received {frags_recv} of {frag_index} fragments",
    );

    Ok(())
}

/// Builds an iterator over the fragments in a packet, after parsing the packet
/// headaer.
///
/// We split this out from `recv_on` to test the fragment iteration loop
/// ourselves, and it keeps things a bit more functional and iterator-oriented.
///
/// Errors must be treated as fatal - see [`recv_on`].
fn recv_frags_on<'a>(
    transport: &'a mut Transport,
    config: &'a TransportConfig,
    recv_at: Instant,
    mut packet: &'a [u8],
) -> impl Iterator<Item = Result<usize, RecvError>> + 'a {
    iter::from_fn(move || {
        if !packet.has_remaining() {
            return None;
        }

        // ensure this fragment is well-formed first
        let Ok(frag) = packet.read::<Fragment>() else {
            return Some(Err(RecvError::ReadFragment));
        };

        // then try to actually receive it
        match recv_frag(transport, config, recv_at, frag) {
            Ok(()) => Some(Ok(packet.len())),
            Err(err) => Some(Err(err)),
        }
    })
}

fn packet_acks_to_msg_keys<'s, const N: usize>(
    flushed_packets: &'s mut SeqBuf<FlushedPacket, N>,
    flushed_packets_size: &'s AtomicUsize,
    send: &'s mut TransportSend,
    rtt: &'s mut RttEstimator,
    packet_acks_recv: &'s mut Saturating<usize>,
    msgs_acks_recv: &'s mut Saturating<usize>,
    recv_at: Instant,
    acked_seqs: impl Iterator<Item = PacketSeq> + 's,
) -> impl Iterator<Item = MessageKey> + 's {
    acked_seqs
        // we now know that our packet with sequence `seq` was acked by the peer
        // let's find what fragments that packet contained when we flushed it out
        .filter_map(move |acked_seq| {
            flushed_packets
                .remove_with(acked_seq.0.0, FlushedPacket::new(recv_at))
                .inspect(|removed| {
                    flushed_packets_size.fetch_sub(
                        core::mem::size_of_val(removed.frags.as_ref()),
                        Ordering::Relaxed,
                    );
                })
                .map(|packet| (acked_seq, packet))
        })
        .flat_map(move |(acked_seq, packet)| {
            let packet_rtt = recv_at.saturating_duration_since(packet.flushed_at);
            rtt.update(packet_rtt);

            let rtt_now = rtt.get();
            trace!(
                "Got peer ack for packet {} - packet RTT: {packet_rtt:?} / RTT now: {rtt_now:?}",
                acked_seq.0.0
            );

            *packet_acks_recv += 1;
            Box::into_iter(packet.frags)
        })
        .filter_map(move |frag_path| {
            // for each of those fragments, we'll mark that fragment as acked
            let lane_index = usize::from(frag_path.lane_index.0);
            let lane = send
                .lanes
                .get_mut(lane_index)
                .expect("frag path should point into a valid lane index");
            // fallible instead of panicking, because these messages may have
            // already been removed by a previous ack that we received
            let msg = lane.sent_msgs.get_mut(&frag_path.msg_seq)?;
            let frag_opt = msg.frags.get_mut(usize::from(frag_path.frag_index))?;
            // take this fragment out so it stops being resent
            let frag = frag_opt.take()?;
            send.sent_msgs_size = send
                .sent_msgs_size
                .checked_sub(usize::from(frag.payload.len()))
                .expect("acknowledged fragment payload bytes should be accounted for");

            // if all the fragments are now acked, then we report that
            // the entire message is now acked
            if msg.frags.iter().all(Option::is_none) {
                *msgs_acks_recv += 1;
                Some(MessageKey {
                    lane: frag_path.lane_index,
                    seq: frag_path.msg_seq,
                })
            } else {
                None
            }
        })
}

fn recv_frag(
    transport: &mut Transport,
    config: &TransportConfig,
    recv_at: Instant,
    frag: Fragment,
) -> Result<(), RecvError> {
    let lane_index = frag.header.lane;

    let index = usize::from(lane_index.0);
    let lane = transport
        .recv
        .lanes
        .get(index)
        .ok_or(RecvError::InvalidLane { lane: lane_index })?;
    if let LaneState::UnreliableSequenced { pending } = lane.state
        && frag.header.seq < pending
    {
        return Ok(());
    }
    let Some(prepared) = lane
        .frags
        .prepare(
            transport.send.max_frag_len,
            frag.header.seq,
            frag.header.position,
            &frag.payload,
        )
        .map_err(RecvError::Reassemble)?
    else {
        return Ok(());
    };

    let mut memory_left = config
        .max_memory_usage
        .saturating_sub(transport.memory_used());
    if prepared.payload_growth > memory_left {
        let candidates = transport
            .recv
            .unreliable_eviction_candidates(index, frag.header.seq);
        for (evict_lane, seq, _) in candidates {
            let removed = transport
                .recv
                .lanes
                .get_mut(evict_lane)
                .expect("candidate came from this lane list")
                .frags
                .discard(seq);
            transport.recv.pending_payload_size = transport
                .recv
                .pending_payload_size
                .checked_sub(removed)
                .expect("removed receive payload bytes should be accounted for");
            memory_left = config
                .max_memory_usage
                .saturating_sub(transport.memory_used());
            if prepared.payload_growth <= memory_left {
                break;
            }
        }
        if prepared.payload_growth > memory_left {
            return Err(RecvError::Reassemble(ReassembleError::OutOfMemory {
                required: prepared.payload_growth,
                left: memory_left,
            }));
        }
    }
    let lane = transport
        .recv
        .lanes
        .get_mut(index)
        .expect("lane was validated above");
    let payload_growth = prepared.payload_growth;
    let msg = lane.frags.insert_prepared(prepared, recv_at);
    transport.recv.pending_payload_size = transport
        .recv
        .pending_payload_size
        .checked_add(payload_growth)
        .expect("receive payload lengths fit in usize");

    trace!(
        "Received fragment on lane {} - message seq {} position {:?}",
        lane_index.0.0, frag.header.seq.0.0, frag.header.position,
    );

    if let Some(msg) = msg {
        let size = &mut transport.recv.pending_payload_size;
        let queued_size = &mut transport.recv.msgs.queued_payload_size;
        let msgs_with_lane = recv_on_lane(&mut lane.state, size, msg, frag.header.seq).map(|msg| {
            *size = size
                .checked_sub(msg.len())
                .expect("removed receive payload bytes should be accounted for");
            *queued_size = queued_size
                .checked_add(msg.len())
                .expect("queued payload lengths fit in usize");
            transport.msgs_recv += 1;
            RecvMessage {
                lane: lane_index,
                recv_at,
                payload: msg,
            }
        });
        transport.recv.msgs.items.extend(msgs_with_lane);
        if let LaneState::UnreliableSequenced { pending } = lane.state {
            *size = size
                .checked_sub(lane.frags.discard_before(pending))
                .expect("removed receive payload bytes should be accounted for");
        }
        trace!("Fragment finished reassembling this message");
    }

    Ok(())
}

fn recv_on_lane<'a>(
    lane: &'a mut LaneState,
    pending_payload_size: &mut usize,
    msg: Vec<u8>,
    msg_seq: MessageSeq,
) -> impl Iterator<Item = Vec<u8>> + 'a {
    match lane {
        LaneState::UnreliableUnordered => {
            // always just return the message
            Either::Left(Some(msg))
        }
        LaneState::UnreliableSequenced { pending } => {
            if msg_seq < *pending {
                // msg is older than the message we're expecting to get next,
                // drop it
                *pending_payload_size = pending_payload_size
                    .checked_sub(msg.len())
                    .expect("removed receive payload bytes should be accounted for");
                drop(msg);
                Either::Left(None)
            } else {
                // msg is the one we're expecting to get or newer, return it
                *pending = msg_seq + MessageSeq::new(1);
                Either::Left(Some(msg))
            }
        }
        LaneState::ReliableUnordered { pending, recv_buf } => {
            if msg_seq < *pending || !recv_buf.insert(msg_seq) {
                // msg is guaranteed to already be received, drop it
                *pending_payload_size = pending_payload_size
                    .checked_sub(msg.len())
                    .expect("removed receive payload bytes should be accounted for");
                drop(msg);
                Either::Left(None)
            } else {
                // here's an example to visualize what this does:
                // msg_seq: 40
                // pending_seq: 40, recv_buf: [41, 45]
                recv_buf.insert(msg_seq);
                // pending_seq: 40, recv_buf: [40, 41, 45]
                while recv_buf.remove(pending) {
                    *pending += MessageSeq::new(1);
                    // iter 1: pending_seq: 41, recv_buf: [41, 45]
                    // iter 2: pending_seq: 42, recv_buf: [45]
                }
                Either::Left(Some(msg))
            }
        }
        LaneState::ReliableOrdered { pending, recv_buf } => {
            if msg_seq < *pending {
                // msg is guaranteed to already be received, drop it
                *pending_payload_size = pending_payload_size
                    .checked_sub(msg.len())
                    .expect("removed receive payload bytes should be accounted for");
                drop(msg);
                Either::Left(None)
            } else {
                // almost identical to above, but we also return the
                // messages that we remove
                if let Some(replaced) = recv_buf.insert(msg_seq, msg) {
                    *pending_payload_size = pending_payload_size
                        .checked_sub(replaced.len())
                        .expect("replaced receive payload bytes should be accounted for");
                }
                Either::Right(iter::from_fn(move || {
                    let msg = recv_buf.remove(pending)?;
                    *pending += MessageSeq::new(1);
                    Some(msg)
                }))
            }
        }
    }
    .into_iter()
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            Transport, TransportConfig,
            lane::{LaneIndex, LaneKind},
            packet::{
                Acknowledge, Fragment, FragmentHeader, FragmentPayload, FragmentPosition,
                MessageSeq, PacketHeader, PacketSeq,
            },
        },
        aeronet_io::Session,
        bevy_platform::time::Instant,
        octs::{Bytes, Write},
    };

    const LANES: [LaneKind; 1] = [LaneKind::ReliableOrdered];
    const LANE: LaneIndex = LaneIndex::new(0);

    #[test]
    fn payload_accounting_gaps_and_duplicate_fragments() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();
        let config = TransportConfig::default();
        let len = usize::from(transport.send.max_frag_len);
        partial(&mut transport, &config, 0, 0, 3, now);
        assert_eq!(transport.recv.memory_used(), 4 * len);
        partial(&mut transport, &config, 0, 0, 3, now);
        partial(&mut transport, &config, 0, 0, 0, now);
        assert_eq!(transport.recv.memory_used(), 4 * len);
        partial(&mut transport, &config, 0, 0, 1, now);
        partial(&mut transport, &config, 0, 0, 2, now);
        super::recv_frag(
            &mut transport,
            &config,
            now,
            Fragment {
                header: FragmentHeader {
                    lane: LANE,
                    seq: MessageSeq::new(0),
                    position: FragmentPosition::last(4u16).unwrap(),
                },
                payload: FragmentPayload::new(Bytes::from_static(b"end")).unwrap(),
            },
        )
        .unwrap();
        assert_eq!(transport.recv.memory_used(), 4 * len + 3);
        drop(transport.recv.msgs.drain());
        assert_eq!(transport.recv.memory_used(), 0);
    }

    #[test]
    fn payload_accounting_delivery_duplicates_and_drain() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        for kind in [LaneKind::ReliableOrdered, LaneKind::ReliableUnordered] {
            let mut transport = Transport::new(&session, [kind], [kind], now).unwrap();
            let config = TransportConfig::default();
            for (seq, payload) in [
                (1, b"abc".as_slice()),
                (1, b"longer"),
                (0, b"x"),
                (0, b"obsolete"),
            ] {
                super::recv_frag(
                    &mut transport,
                    &config,
                    now,
                    Fragment {
                        header: FragmentHeader {
                            lane: LANE,
                            seq: MessageSeq::new(seq),
                            position: FragmentPosition::last(0u16).unwrap(),
                        },
                        payload: FragmentPayload::new(Bytes::copy_from_slice(payload)).unwrap(),
                    },
                )
                .unwrap();
                let retained = if kind == LaneKind::ReliableOrdered {
                    6
                } else {
                    3
                };
                let total = if seq == 1 {
                    if kind == LaneKind::ReliableOrdered {
                        payload.len()
                    } else {
                        3
                    }
                } else {
                    retained + 1
                };
                let pending = if kind == LaneKind::ReliableOrdered && seq == 1 {
                    payload.len()
                } else {
                    0
                };
                assert_eq!(transport.recv.pending_payload_size, pending);
                assert_eq!(transport.recv.msgs.queued_payload_size, total - pending);
                assert_eq!(transport.recv.memory_used(), total);
            }
            let mut drain = transport.recv.msgs.drain();
            assert!(drain.next().is_some());
            drop(drain);
            assert_eq!(transport.recv.memory_used(), 0);
            assert_eq!(transport.recv.msgs.drain().count(), 0);
        }
    }

    #[test]
    fn payload_accounting_across_lanes_and_clear_buffers() {
        use bevy_ecs::{schedule::Schedule, world::World};

        let now = Instant::now();
        let session = Session::new(now, 1024);
        let lanes = [LaneKind::ReliableOrdered; 2];
        let mut transport = Transport::new(&session, lanes, lanes, now).unwrap();
        let config = TransportConfig::default();
        let len = usize::from(transport.send.max_frag_len);
        partial(&mut transport, &config, 0, 1, 0, now);
        partial(&mut transport, &config, 1, 1, 2, now);
        assert_eq!(transport.recv.memory_used(), 4 * len);
        for lane in [0, 1] {
            super::recv_frag(
                &mut transport,
                &config,
                now,
                Fragment {
                    header: FragmentHeader {
                        lane: LaneIndex::new(lane),
                        seq: MessageSeq::new(0),
                        position: FragmentPosition::ZERO_LAST,
                    },
                    payload: FragmentPayload::new(Bytes::from_static(b"abc")).unwrap(),
                },
            )
            .unwrap();
        }
        assert_eq!(transport.recv.memory_used(), 4 * len + 6);
        assert_eq!(transport.recv.pending_payload_size, 4 * len);
        assert_eq!(transport.recv.msgs.queued_payload_size, 6);
        assert_eq!(transport.recv.msgs.items.len(), 2);

        let mut world = World::new();
        let entity = world.spawn(transport).id();
        let mut schedule = Schedule::default();
        schedule.add_systems(super::clear_buffers);
        schedule.run(&mut world);
        let transport = world.get::<Transport>(entity).unwrap();
        assert!(transport.recv.msgs.items.is_empty());
        assert_eq!(transport.recv.memory_used(), 4 * len);
        // Clearing an already empty delivery queue preserves partial payloads.
        schedule.run(&mut world);
        assert_eq!(
            world.get::<Transport>(entity).unwrap().recv.memory_used(),
            4 * len
        );
    }

    #[test]
    fn packet_ack_size_accounting() {
        use {
            crate::{
                FlushedPacket, FragmentPath, rtt::RttEstimator, seq_buf::SeqBuf, size::MinSize,
            },
            bevy_platform::sync::atomic::{AtomicUsize, Ordering},
            core::num::Saturating,
        };

        let now = Instant::now();
        let mut packets = SeqBuf::<FlushedPacket, 1>::new_from_fn(|_| FlushedPacket::new(now));
        let size = AtomicUsize::new(0);
        for (seq, num_frags) in [(0, 1), (1, 3)] {
            let frags = vec![
                FragmentPath {
                    lane_index: LANE,
                    msg_seq: MessageSeq::new(0),
                    frag_index: MinSize(0),
                };
                num_frags
            ]
            .into_boxed_slice();
            let bytes = core::mem::size_of_val(frags.as_ref());
            let old = packets.insert(
                seq,
                FlushedPacket {
                    flushed_at: now,
                    frags,
                },
            );
            if let Some(old) = old {
                size.fetch_sub(
                    core::mem::size_of_val(old.frags.as_ref()),
                    Ordering::Relaxed,
                );
            }
            size.fetch_add(bytes, Ordering::Relaxed);
            assert_eq!(size.load(Ordering::Relaxed), bytes);
        }

        let session = Session::new(now, 1024);
        let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();
        let mut rtt = RttEstimator::default();
        let mut packet_acks = Saturating(0);
        let mut msg_acks = Saturating(0);
        let num_keys = super::packet_acks_to_msg_keys(
            &mut packets,
            &size,
            &mut transport.send,
            &mut rtt,
            &mut packet_acks,
            &mut msg_acks,
            now,
            [0, 1, 1].into_iter().map(PacketSeq::new),
        )
        .count();
        assert_eq!(num_keys, 0);
        assert!(packets.is_empty());
        assert_eq!(size.load(Ordering::Relaxed), 0);
        assert_eq!(packet_acks.0, 1);
    }

    #[test]
    fn recv_one_frag() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();

        let mut packet = Vec::<u8>::new();
        packet
            .write(&PacketHeader {
                seq: PacketSeq::new(0),
                acks: Acknowledge::default(),
            })
            .unwrap();
        packet
            .write(&Fragment {
                header: FragmentHeader {
                    lane: LANE,
                    seq: MessageSeq::new(0),
                    position: FragmentPosition::last(0u16).unwrap(),
                },
                payload: FragmentPayload::new(Bytes::from_static(b"hello world")).unwrap(),
            })
            .unwrap();

        super::recv_on(
            &mut transport,
            &TransportConfig::default(),
            Instant::now(),
            &packet,
        )
        .unwrap();

        {
            let mut msgs = transport.recv.msgs.drain();
            assert!(msgs.next().is_some());
            assert!(msgs.next().is_none());
        }
    }

    #[test]
    fn recv_no_frags() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();

        let mut packet = Vec::<u8>::new();
        packet
            .write(&PacketHeader {
                seq: PacketSeq::new(0),
                acks: Acknowledge::default(),
            })
            .unwrap();
        // we don't write a fragment here
        // so we must not receive a message

        super::recv_on(
            &mut transport,
            &TransportConfig::default(),
            Instant::now(),
            &packet,
        )
        .unwrap();

        {
            let mut msgs = transport.recv.msgs.drain();
            assert!(msgs.next().is_none());
        }
    }

    #[test]
    fn recv_truncated_frag_returns_decode_error() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();
        let config = TransportConfig::default();
        // A fragment's message sequence needs two bytes, so decoding fails
        // without consuming this trailing byte.
        let mut frags = super::recv_frags_on(&mut transport, &config, now, &[0]);
        // An outer error makes recv_on return via `?`. An inner error would
        // be logged and retried indefinitely because no bytes were consumed.
        assert!(matches!(
            frags.next(),
            Some(Err(super::RecvError::ReadFragment))
        ));
    }

    #[test]
    fn recv_fresh_frag_after_duplicate() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();
        let config = TransportConfig::default();
        let last = Fragment {
            header: FragmentHeader {
                lane: LANE,
                seq: MessageSeq::new(0),
                position: FragmentPosition::last(1u16).unwrap(),
            },
            payload: FragmentPayload::new(Bytes::from_static(b"tail")).unwrap(),
        };

        // Keep the message partially reassembled so receiving `last` again
        // exercises fragment deduplication, rather than message deduplication.
        let mut packet = Vec::<u8>::new();
        packet.write(PacketHeader::default()).unwrap();
        packet.write(&last).unwrap();
        super::recv_on(&mut transport, &config, now, &packet).unwrap();
        assert!(transport.recv.msgs.drain().next().is_none());

        let first_payload = vec![b'a'; usize::from(transport.send.max_frag_len)];
        let first = Fragment {
            header: FragmentHeader {
                lane: LANE,
                seq: MessageSeq::new(0),
                position: FragmentPosition::ZERO_NON_LAST,
            },
            payload: FragmentPayload::new(Bytes::from(first_payload.clone())).unwrap(),
        };

        // A retransmission may share a packet with a fragment we still need.
        // Rejecting the duplicate must not prevent processing the fresh
        // fragment.
        packet.clear();
        packet
            .write(PacketHeader {
                seq: PacketSeq::new(1),
                acks: Acknowledge::default(),
            })
            .unwrap();
        packet.write(&last).unwrap();
        packet.write(first).unwrap();
        super::recv_on(&mut transport, &config, now, &packet).unwrap();

        let mut expected = first_payload;
        expected.extend_from_slice(b"tail");
        let mut msgs = transport.recv.msgs.drain();
        let msg = msgs
            .next()
            .expect("fresh fragment should complete the message");
        assert_eq!(msg.lane, LANE);
        assert_eq!(msg.payload, expected);
        assert!(msgs.next().is_none());
        assert!(transport.peer_acks.is_acked(PacketSeq::new(1)));
    }

    #[test]
    fn recv_reliable_unordered_drops_duplicate_ahead_of_gap() {
        let mut lane = super::RecvLane::new(LaneKind::ReliableUnordered);
        let msg = b"message 1".to_vec();
        let seq = MessageSeq::new(1);

        // Message 0 is still missing, but unordered delivery must allow 1
        // through immediately and remember it when a retransmission arrives.
        let mut pending_payload_size = msg.len();
        let received =
            super::recv_on_lane(&mut lane.state, &mut pending_payload_size, msg.clone(), seq)
                .collect::<Vec<_>>();
        assert_eq!(received, vec![msg.clone()]);

        assert!(
            super::recv_on_lane(&mut lane.state, &mut pending_payload_size, msg, seq)
                .next()
                .is_none(),
            "a reliable-unordered message must only be delivered once, even before the gap closes"
        );
        assert_eq!(pending_payload_size, 0);
    }

    #[test]
    fn recv_out_of_memory_frag_does_not_ack_packet() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let mut transport = Transport::new(&session, LANES, LANES, now).unwrap();
        let config = TransportConfig {
            // Enough for reassembly metadata, but not the message's payload.
            max_memory_usage: transport.memory_used() + 1024,
            ..TransportConfig::default()
        };
        let packet_seq = PacketSeq::new(1);
        let mut packet = Vec::<u8>::new();
        packet
            .write(PacketHeader {
                seq: packet_seq,
                acks: Acknowledge::default(),
            })
            .unwrap();
        packet
            .write(Fragment {
                header: FragmentHeader {
                    lane: LANE,
                    seq: MessageSeq::new(0),
                    position: FragmentPosition::last(2u16).unwrap(),
                },
                payload: FragmentPayload::new(Bytes::from_static(b"tail")).unwrap(),
            })
            .unwrap();

        assert!(matches!(
            super::recv_on(&mut transport, &config, now, &packet),
            Err(super::RecvError::Reassemble(
                super::ReassembleError::OutOfMemory { .. }
            ))
        ));

        assert!(transport.recv.msgs.drain().next().is_none());
        // Rejecting the allocation leaves us below the disconnect threshold.
        assert!(transport.memory_used() <= config.max_memory_usage);
        assert!(
            !transport.peer_acks.is_acked(packet_seq),
            "acking a rejected reliable fragment would stop the sender retransmitting it"
        );
    }

    fn partial(
        transport: &mut Transport,
        config: &TransportConfig,
        lane: u16,
        seq: u16,
        index: u16,
        now: Instant,
    ) {
        let payload = vec![0; usize::from(transport.send.max_frag_len)];
        super::recv_frag(
            transport,
            config,
            now,
            Fragment {
                header: FragmentHeader {
                    lane: LaneIndex::new(u32::from(lane)),
                    seq: MessageSeq::new(seq),
                    position: FragmentPosition::non_last(index).unwrap(),
                },
                payload: FragmentPayload::new(Bytes::from(payload)).unwrap(),
            },
        )
        .unwrap();
    }

    #[test]
    fn memory_pressure_evicts_only_oldest_unreliable_message() {
        use core::time::Duration;
        let now = Instant::now();
        let later = now.checked_add(Duration::from_secs(1)).unwrap();
        let session = Session::new(now, 1024);
        let lanes = [LaneKind::UnreliableUnordered, LaneKind::ReliableOrdered];
        let mut transport = Transport::new(&session, lanes, lanes, now).unwrap();
        let mut config = TransportConfig::default();
        partial(&mut transport, &config, 1, 0, 3, now);
        partial(&mut transport, &config, 0, 0, 3, now);
        partial(&mut transport, &config, 0, 1, 3, later);
        config.max_memory_usage = transport.memory_used().saturating_add(256);
        partial(&mut transport, &config, 0, 2, 0, later);
        assert!(transport.memory_used() <= config.max_memory_usage);
        let lane = transport.recv.lanes.first().unwrap();
        assert_eq!(lane.frags.len(), 2);
        assert_eq!(
            lane.frags
                .eviction_candidates(Some(MessageSeq::new(2)))
                .next()
                .unwrap()
                .0,
            MessageSeq::new(1)
        );
        assert_eq!(transport.recv.lanes.get(1).unwrap().frags.len(), 1);
        assert_eq!(
            transport.recv.memory_used(),
            9 * usize::from(transport.send.max_frag_len)
        );
    }

    #[test]
    fn invalid_fragment_does_not_evict_or_allocate() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let lanes = [LaneKind::UnreliableUnordered];
        let mut transport = Transport::new(&session, lanes, lanes, now).unwrap();
        let mut config = TransportConfig::default();
        partial(&mut transport, &config, 0, 0, 0, now);
        let before = transport.memory_used();
        config.max_memory_usage = before;
        let result = super::recv_frag(
            &mut transport,
            &config,
            now,
            Fragment {
                header: FragmentHeader {
                    lane: LANE,
                    seq: MessageSeq::new(1),
                    position: FragmentPosition::non_last(10u16).unwrap(),
                },
                payload: FragmentPayload::empty(),
            },
        );
        assert!(matches!(
            result,
            Err(super::RecvError::Reassemble(
                super::ReassembleError::InvalidPayloadLength { .. }
            ))
        ));
        assert_eq!(transport.memory_used(), before);
        assert_eq!(transport.recv.lanes.first().unwrap().frags.len(), 1);
    }

    #[test]
    fn memory_pressure_evicts_multiple_messages_in_age_order() {
        use core::time::Duration;
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let lanes = [LaneKind::UnreliableUnordered; 2];
        let mut transport = Transport::new(&session, lanes, lanes, now).unwrap();
        let mut config = TransportConfig::default();
        partial(&mut transport, &config, 1, 0, 3, now);
        let later = now.checked_add(Duration::from_secs(1)).unwrap();
        partial(&mut transport, &config, 0, 0, 3, later);
        let latest = later.checked_add(Duration::from_secs(1)).unwrap();
        partial(&mut transport, &config, 1, 1, 3, latest);
        config.max_memory_usage = transport.memory_used().saturating_add(256);
        // This needs more than one candidate's allocation to be reclaimed.
        partial(&mut transport, &config, 0, 1, 6, latest);
        assert!(transport.memory_used() <= config.max_memory_usage);
        for lane in &transport.recv.lanes {
            assert_eq!(lane.frags.len(), 1);
            assert_eq!(
                lane.frags.eviction_candidates(None).next().unwrap().0,
                MessageSeq::new(1)
            );
        }
    }

    #[test]
    fn sequenced_delivery_discards_obsolete_partials_across_wraparound() {
        let now = Instant::now();
        let session = Session::new(now, 1024);
        let lanes = [LaneKind::UnreliableSequenced];
        let mut transport = Transport::new(&session, lanes, lanes, now).unwrap();
        let config = TransportConfig::default();
        let old = u16::MAX.saturating_sub(1);
        transport.recv.lanes.first_mut().unwrap().state = super::LaneState::UnreliableSequenced {
            pending: MessageSeq::new(old),
        };
        partial(&mut transport, &config, 0, old, 0, now);
        partial(&mut transport, &config, 0, 0, 0, now);
        super::recv_frag(
            &mut transport,
            &config,
            now,
            Fragment {
                header: FragmentHeader {
                    lane: LANE,
                    seq: MessageSeq::new(u16::MAX),
                    position: FragmentPosition::ZERO_LAST,
                },
                payload: FragmentPayload::empty(),
            },
        )
        .unwrap();
        assert_eq!(transport.recv.msgs.drain().count(), 1);
        assert_eq!(transport.recv.lanes.first().unwrap().frags.len(), 1);
        assert_eq!(
            transport.recv.memory_used(),
            usize::from(transport.send.max_frag_len)
        );
        // An obsolete fragment must not recreate its discarded buffer.
        partial(&mut transport, &config, 0, old, 0, now);
        assert_eq!(transport.recv.lanes.first().unwrap().frags.len(), 1);
    }

    #[test]
    fn recv_out_of_memory_disconnects_session() {
        use {
            aeronet_io::{AeronetIoPlugin, connection::DisconnectReason, packet::RecvPacket},
            bevy_app::{App, Update},
            bevy_ecs::prelude::*,
        };

        #[derive(Default, Resource)]
        struct DisconnectCount(usize);

        let now = Instant::now();
        let mut session = Session::new(now, 1024);
        // Unordered delivery lets message 1 arrive while message 0 is
        // incomplete.
        let lanes = [LaneKind::ReliableUnordered];
        let transport = Transport::new(&session, lanes, lanes, now).unwrap();
        let config = TransportConfig {
            max_memory_usage: transport.memory_used() + 1024,
            ..TransportConfig::default()
        };
        let packet_seq = PacketSeq::new(1);
        let mut packet = Vec::<u8>::new();
        packet
            .write(PacketHeader {
                seq: packet_seq,
                acks: Acknowledge::default(),
            })
            .unwrap();

        // The first fragment needs a large reassembly buffer. The second is
        // a complete message small enough to fit in the remaining budget.
        for (seq, position, payload) in [
            (0, FragmentPosition::last(2u16).unwrap(), b"tail".as_slice()),
            (1, FragmentPosition::ZERO_LAST, b"small".as_slice()),
        ] {
            packet
                .write(Fragment {
                    header: FragmentHeader {
                        lane: LANE,
                        seq: MessageSeq::new(seq),
                        position,
                    },
                    payload: FragmentPayload::new(Bytes::from_static(payload)).unwrap(),
                })
                .unwrap();
        }

        // Multiple queued packets must still produce only one disconnect.
        let packet = Bytes::from(packet);
        for _ in 0..2 {
            session.recv.push(RecvPacket {
                recv_at: now,
                payload: packet.clone(),
            });
        }

        let mut app = App::new();
        app.add_plugins(AeronetIoPlugin)
            .init_resource::<DisconnectCount>()
            .add_systems(Update, super::poll)
            .add_observer(
                move |event: On<super::Disconnected>,
                      mut count: ResMut<DisconnectCount>,
                      transports: Query<(&Transport, &TransportConfig)>| {
                    let DisconnectReason::ByError(err) = &event.reason else {
                        panic!("expected an OOM disconnect");
                    };
                    assert!(matches!(
                        err.downcast_ref::<super::RecvError>(),
                        Some(super::RecvError::Reassemble(
                            super::ReassembleError::OutOfMemory { .. }
                        ))
                    ));

                    let (transport, config) = transports.get(event.entity).unwrap();
                    assert!(transport.recv.msgs.items.is_empty());
                    assert!(!transport.peer_acks.is_acked(packet_seq));
                    // The rejected allocation never exceeded the global limit;
                    // the receive error itself must trigger disconnection.
                    assert!(transport.memory_used() <= config.max_memory_usage);
                    count.0 += 1;
                },
            );
        let entity = app.world_mut().spawn((session, transport, config)).id();
        // Poll directly so the IO plugin's PreUpdate buffer clearing doesn't
        // discard the packets we injected for the test.
        app.world_mut().run_schedule(Update);

        assert_eq!(app.world().resource::<DisconnectCount>().0, 1);
        assert!(app.world().get_entity(entity).is_err());
    }
}
