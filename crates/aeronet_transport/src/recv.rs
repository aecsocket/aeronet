//! Receiving logic for [`Transport`]s.

use {
    crate::{
        FlushedPacket, MessageKey, RecvMessage, Transport, TransportConfig,
        frag::{FragmentReceiver, ReassembleError},
        lane::{LaneIndex, LaneKind},
        packet::{Fragment, MessageSeq, PacketHeader, PacketSeq},
        rtt::RttEstimator,
        send::SendLane,
        seq_buf::SeqBuf,
    },
    aeronet_io::Session,
    alloc::{boxed::Box, vec::Vec},
    bevy_ecs::prelude::*,
    bevy_platform_support::{
        collections::{HashMap, HashSet},
        time::Instant,
    },
    core::{iter, num::Saturating},
    derive_more::{Display, Error},
    either::Either,
    log::{trace, warn},
    octs::{Buf, Read},
    typesize::{TypeSize, derive::TypeSize},
};

/// Access to the receiving half of a [`Transport`].
#[derive(Debug, TypeSize)]
pub struct TransportRecv {
    lanes: Box<[RecvLane]>,
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
#[derive(Debug, TypeSize)]
pub struct RecvBuffer<T: TypeSize>(Vec<T>);

impl TransportRecv {
    pub(crate) fn new(lanes: impl IntoIterator<Item = impl Into<LaneKind>>) -> Self {
        Self {
            lanes: lanes
                .into_iter()
                .map(Into::into)
                .map(RecvLane::new)
                .collect(),
            msgs: RecvBuffer(Vec::new()),
            acks: RecvBuffer(Vec::new()),
        }
    }

    /// Gets access to the state of the receiving-side lanes.
    #[must_use]
    pub const fn lanes(&self) -> &[RecvLane] {
        &self.lanes
    }
}

impl<T: TypeSize> RecvBuffer<T> {
    /// Drains all items from this buffer.
    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.0.drain(..)
    }
}

/// State of a lane used for receiving incoming messages on a [`Transport`].
#[derive(Debug, Clone, TypeSize)]
pub struct RecvLane {
    frags: FragmentReceiver,
    state: LaneState,
}

#[derive(Debug, Clone, TypeSize)]
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
        let len = transport.recv.msgs.0.len();
        if len > 0 {
            warn!(
                "{entity} has {len} received messages which have not been consumed - this \
                 indicates a bug in code above the transport layer"
            );
            transport.recv.msgs.0.clear();
        }

        let len = transport.recv.acks.0.len();
        if len > 0 {
            warn!(
                "{entity} has {len} received acks which have not been consumed - this indicates a \
                 bug in code above the transport layer"
            );
            transport.recv.acks.0.clear();
        }
    }
}

pub(crate) fn poll(mut sessions: Query<(Entity, &mut Session, &mut Transport, &TransportConfig)>) {
    for (entity, mut session, mut transport, config) in &mut sessions {
        for packet in session.recv.drain(..) {
            if let Err(err) = recv_on(&mut transport, config, packet.recv_at, &packet.payload) {
                trace!("{entity} received invalid packet: {err:?}");
            }
        }
    }
}

#[derive(Debug, Display, Error)]
enum RecvError {
    #[display("not enough bytes to read header")]
    ReadHeader,
    #[display("not enough bytes to read fragment")]
    ReadFragment,
    #[display("invalid lane {lane:?}")]
    InvalidLane { lane: LaneIndex },
    #[display("failed to reassemble fragment")]
    Reassemble(ReassembleError),
}

/// Exposes `recv_on` for fuzz tests.
#[cfg(fuzzing)]
pub fn fuzz_recv_on(
    transport: &mut Transport,
    packet: &[u8],
) -> Result<(), Box<dyn core::error::Error>> {
    recv_on(
        transport,
        &TransportConfig::default(),
        Instant::now(),
        packet,
    )
    .map_err(|err| Box::new(err) as Box<_>)
}

fn recv_on(
    transport: &mut Transport,
    config: &TransportConfig,
    recv_at: Instant,
    mut packet: &[u8],
) -> Result<(), RecvError> {
    trace!("Receiving packet of length {}", packet.len());

    let header = packet
        .read::<PacketHeader>()
        .map_err(|_| RecvError::ReadHeader)?;

    trace!("Received packet header with sequence {}", header.seq.0.0);

    transport.peer_acks.ack(header.seq);
    transport.recv.acks.0.extend(packet_acks_to_msg_keys(
        &mut transport.flushed_packets,
        &mut transport.send.lanes,
        &mut transport.rtt,
        &mut transport.stats.packet_acks_recv,
        &mut transport.stats.msg_acks_recv,
        recv_at,
        header.acks.seqs(),
    ));

    let mut frag_index = Saturating(0);
    let mut frags_recv = Saturating(0);
    while packet.has_remaining() {
        match recv_frag(transport, config, recv_at, &mut packet) {
            Ok(()) => {
                frags_recv += 1;
            }
            Err(err) => {
                trace!("Failed to receive fragment {}: {err:?}", frag_index.0);
            }
        }
        frag_index += 1;
    }

    trace!("Finished receiving packet with {} fragments", frags_recv.0);

    Ok(())
}

fn packet_acks_to_msg_keys<'s, const N: usize>(
    flushed_packets: &'s mut SeqBuf<FlushedPacket, N>,
    send_lanes: &'s mut [SendLane],
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
                .remove_with(acked_seq.0 .0, FlushedPacket::new(recv_at))
                .map(|packet| (acked_seq, packet))
        })
        .flat_map(move |(acked_seq, packet)| {
            let packet_rtt = recv_at.saturating_duration_since(packet.flushed_at);
            rtt.update(packet_rtt);

            let rtt_now = rtt.get();
            trace!("Got peer ack for packet {} - packet RTT: {packet_rtt:?} / RTT now: {rtt_now:?}", acked_seq.0.0);

            *packet_acks_recv += 1;
            Box::into_iter(packet.frags)
        })
        .filter_map(|frag_path| {
            // for each of those fragments, we'll mark that fragment as acked
            let lane_index = usize::from(frag_path.lane_index.0);
            let lane = send_lanes
                .get_mut(lane_index)
                .expect("frag path should point into a valid lane index");
            // fallible instead of panicking, because these messages may have already been
            // removed by a previous ack that we received
            let msg = lane.sent_msgs.get_mut(&frag_path.msg_seq)?;
            let frag_opt = msg.frags.get_mut(usize::from(frag_path.frag_index))?;
            // take this fragment out so it stops being resent
            *frag_opt = None;

            // if all the fragments are now acked, then we report that
            // the entire message is now acked
            if msg.frags.iter().all(Option::is_none) {
                *msgs_acks_recv += 1;
                Some(MessageKey {
                    lane: frag_path.lane_index,
                    seq: frag_path.msg_seq
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
    packet: &mut &[u8],
) -> Result<(), RecvError> {
    let frag = packet
        .read::<Fragment>()
        .map_err(|_| RecvError::ReadFragment)?;
    let lane_index = frag.header.lane;

    let memory_left = config
        .max_memory_usage
        .saturating_sub(transport.memory_used());
    let lane = transport
        .recv
        .lanes
        .get_mut(usize::from(lane_index.0))
        .ok_or(RecvError::InvalidLane { lane: lane_index })?;
    let msg = lane
        .frags
        .reassemble(
            transport.send.max_frag_len,
            memory_left,
            frag.header.seq,
            frag.header.position,
            &frag.payload,
        )
        .map_err(RecvError::Reassemble)?;

    trace!(
        "Received fragment on lane {} - message seq {} position {:?}",
        lane_index.0.0, frag.header.seq.0.0, frag.header.position,
    );

    if let Some(msg) = msg {
        let msgs_with_lane =
            recv_on_lane(&mut lane.state, msg, frag.header.seq).map(|msg| RecvMessage {
                lane: lane_index,
                recv_at,
                payload: msg,
            });
        transport.recv.msgs.0.extend(msgs_with_lane);
        trace!("Fragment finished reassembling this message");
    }

    Ok(())
}

fn recv_on_lane(
    lane: &mut LaneState,
    msg: Vec<u8>,
    msg_seq: MessageSeq,
) -> impl Iterator<Item = Vec<u8>> + '_ {
    match lane {
        LaneState::UnreliableUnordered => {
            // always just return the message
            Either::Left(Some(msg))
        }
        LaneState::UnreliableSequenced { pending } => {
            if msg_seq < *pending {
                // msg is older than the message we're expecting to get next, drop it
                Either::Left(None)
            } else {
                // msg is the one we're expecting to get or newer, return it
                *pending = msg_seq + MessageSeq::new(1);
                Either::Left(Some(msg))
            }
        }
        LaneState::ReliableUnordered { pending, recv_buf } => {
            if msg_seq < *pending {
                // msg is guaranteed to already be received, drop it
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
                Either::Left(None)
            } else {
                // almost identical to above, but we also return the
                // messages that we remove
                recv_buf.insert(msg_seq, msg);
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
