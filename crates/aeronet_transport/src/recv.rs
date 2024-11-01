//! Receiving logic for [`Transport`]s.

use {
    crate::{
        frag::{FragmentReceiver, ReassembleError},
        lane::{LaneIndex, LaneKind},
        packet::{Fragment, MessageSeq, PacketHeader, PacketSeq},
        rtt::RttEstimator,
        send,
        seq_buf::SeqBuf,
        FlushedPacket, MessageKey, Transport,
    },
    aeronet_io::{connection::Disconnect, packet::PacketBuffers},
    ahash::{HashMap, HashSet},
    bevy_ecs::prelude::*,
    itertools::Either,
    octs::{Buf, Read},
    std::{iter, num::Saturating},
    thiserror::Error,
    tracing::{trace, trace_span, warn},
    typesize::{derive::TypeSize, TypeSize},
    web_time::Instant,
};

#[derive(Debug, TypeSize)]
pub struct TransportRecv<T: TypeSize>(Vec<T>);

impl<T: TypeSize> TransportRecv<T> {
    pub(crate) const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.0.drain(..)
    }
}

#[derive(Debug, Clone, TypeSize)]
pub(crate) struct Lane {
    frags: FragmentReceiver,
    state: LaneState,
}

impl Lane {
    pub(crate) fn new(kind: LaneKind) -> Self {
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

#[derive(Debug, Error)]
pub enum RecvError {
    #[error("not enough bytes to read header")]
    ReadHeader,
    #[error("not enough bytes to read fragment")]
    ReadFragment,
    #[error("invalid lane {lane:?}")]
    InvalidLane { lane: LaneIndex },
    #[error("failed to reassemble fragment")]
    Reassemble(#[source] ReassembleError),
}

pub(crate) fn poll(
    mut commands: Commands,
    mut sessions: Query<(Entity, &mut Transport, &mut PacketBuffers)>,
) {
    for (session, mut transport, mut packet_bufs) in &mut sessions {
        let span = trace_span!("poll", %session);
        let _span = span.enter();

        for (recv_at, packet) in packet_bufs.recv.drain() {
            // TODO: expose the first packet `recv_at` to expose
            // when this message arrived
            if let Err(err) = recv_on(&mut transport, recv_at, &packet) {
                let err = anyhow::Error::new(err);
                trace!("{session} received invalid packet: {err:#}");
                continue;
            };
        }

        let mem_used = transport.memory_used();
        let mem_max = transport.max_memory_usage;
        if mem_used > mem_max {
            warn!("{session} exceeded memory limit, disconnecting - {mem_used} / {mem_max} bytes");
            commands.trigger_targets(Disconnect::new("memory limit exceeded"), session);
        }
    }
}

fn recv_on(
    transport: &mut Transport,
    recv_at: Instant,
    mut packet: &[u8],
) -> Result<(), RecvError> {
    let header = packet
        .read::<PacketHeader>()
        .map_err(|_| RecvError::ReadHeader)?;

    let span = trace_span!("recv", packet = header.seq.0 .0);
    let _span = span.enter();

    trace!(len = packet.len(), "Received packet");

    transport.recv_acks.0.extend(packet_acks_to_msg_keys(
        &mut transport.flushed_packets,
        &mut transport.send.lanes,
        &mut transport.rtt,
        &mut transport.stats.packet_acks_recv,
        recv_at,
        header.acks.seqs(),
    ));

    while packet.has_remaining() {
        recv_frag(transport, &mut packet)?;
    }

    transport.peer_acks.ack(header.seq);
    Ok(())
}

fn packet_acks_to_msg_keys<'s, const N: usize>(
    flushed_packets: &'s mut SeqBuf<FlushedPacket, N>,
    send_lanes: &'s mut [send::Lane],
    rtt: &'s mut RttEstimator,
    packet_acks_recv: &'s mut Saturating<usize>,
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
            let span = trace_span!("ack", packet = acked_seq.0 .0);
            let _span = span.enter();

            *packet_acks_recv += 1;
            let packet_rtt = recv_at.saturating_duration_since(packet.flushed_at.0);
            rtt.update(packet_rtt);
            let rtt_now = rtt.get();
            trace!(?acked_seq, ?packet_rtt, ?rtt_now, "Got peer ack");

            Box::into_iter(packet.frags)
        })
        .filter_map(|frag_path| {
            // for each of those fragments, we'll mark that fragment as acked
            let lane_index = usize::from(frag_path.lane_index);
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
                Some(MessageKey {
                    lane: frag_path.lane_index,
                    seq: frag_path.msg_seq
                })
            } else {
                None
            }
        })
}

fn recv_frag(transport: &mut Transport, packet: &mut &[u8]) -> Result<(), RecvError> {
    let frag = packet
        .read::<Fragment>()
        .map_err(|_| RecvError::ReadFragment)?;
    let lane_index = frag.header.lane;

    let memory_left = transport.memory_left();
    let lane = transport
        .recv_lanes
        .get_mut(usize::from(lane_index))
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

    if let Some(msg) = msg {
        let msgs_with_lane =
            recv_on_lane(&mut lane.state, msg, frag.header.seq).map(|msg| (lane_index, msg));
        transport.recv_msgs.0.extend(msgs_with_lane);
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
                // pending_seq: 40, recv_seq_buf: [41, 45]
                recv_buf.insert(msg_seq);
                // pending_seq: 40, recv_seq_buf: [40, 41, 45]
                while recv_buf.remove(pending) {
                    *pending += MessageSeq::new(1);
                    // iter 1: pending_seq: 41, recv_seq_buf: [41, 45]
                    // iter 2: pending_seq: 42, recv_seq_buf: [45]
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
