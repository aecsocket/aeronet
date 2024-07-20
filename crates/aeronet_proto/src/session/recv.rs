use std::convert::Infallible;

use aeronet::lane::LaneIndex;
use either::Either;
use octs::{Buf, BufTooShortOr, Bytes, Read};
use web_time::Instant;

use crate::{
    msg::{FragmentDecodeError, ReassembleError},
    rtt::RttEstimator,
    seq::SeqBuf,
    ty::{Fragment, MessageSeq, PacketHeader, PacketSeq},
};

use super::{FlushedPacket, RecvLane, RecvLaneKind, SendLane, Session};

/// Failed to [`Session::recv`] a packet.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RecvError {
    /// Failed to decode packet header.
    #[error("failed to decode header")]
    DecodeHeader(#[source] BufTooShortOr<Infallible>),
    /// Failed to decode fragment.
    #[error("failed to decode fragment")]
    DecodeFragment(#[source] BufTooShortOr<FragmentDecodeError>),
    /// Decoded a lane index which we are not tracking.
    #[error("invalid lane index {}", lane.into_raw())]
    InvalidLaneIndex {
        /// Index of the invalid lane.
        lane: LaneIndex,
    },
    /// Failed to reassemble a fragment into a message.
    #[error("failed to reassemble message")]
    Reassemble(#[source] ReassembleError),
}

impl Session {
    /// Starts receiving a packet.
    ///
    /// If this is successful, this returns:
    /// - an iterator over all of *our* sent messages which have been
    ///   acknowledged by the peer, along with the lane on which the message
    ///   was sent on
    /// - a [`RecvMessages`], used to read the fragments (actual payload) of
    ///   this packet
    ///
    /// Generally, you should use this like:
    ///
    /// ```
    /// # use aeronet::lane::LaneIndex;
    /// # use aeronet_proto::{session::Session, ty::MessageSeq};
    /// # use octs::Bytes;
    /// # fn recv(mut session: Session, packet: Bytes) -> Result<(), Box<dyn std::error::Error>> {
    /// let (acks, msgs) = session.recv(packet)?;
    /// for (lane_index, msg_seq) in acks {
    ///     do_something_with_ack(lane_index, msg_seq);
    /// }
    /// msgs.for_each_msg(|result| {
    ///     match result {
    ///         Ok((msg, lane_index)) => {
    ///             do_something_with_msg(lane_index, msg);
    ///         }
    ///         Err(err) => {
    ///             eprintln!("{err:?}");
    ///         }
    ///     }
    /// });
    ///
    /// fn do_something_with_ack(lane_index: LaneIndex, msg_seq: MessageSeq) { unimplemented!() }
    ///
    /// fn do_something_with_msg(msg: Bytes, lane_index: LaneIndex) { unimplemented!() }
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Errors if the packet header is invalid.
    pub fn recv(
        &mut self,
        now: Instant,
        mut packet: Bytes,
    ) -> Result<
        (
            impl Iterator<Item = (LaneIndex, MessageSeq)> + '_,
            RecvMessages<'_>,
        ),
        RecvError,
    > {
        self.bytes_recv = self.bytes_recv.saturating_add(packet.len());

        let header = packet
            .read::<PacketHeader>()
            .map_err(RecvError::DecodeHeader)?;
        self.acks.ack(header.seq);

        if packet.has_remaining() {
            // this packet actually has some frags!
            // we should send back an ack ASAP, even if we have no frags to send
            // otherwise, if we don't have any frags queued to send,
            // we would only send the ack on the next keep-alive,
            // and our peer would resend the same frag like a billion more times
            // because it thinks we haven't received it yet
            self.next_keep_alive_at = now;
        }

        let acks = Self::recv_acks(
            &mut self.flushed_packets,
            &mut self.send_lanes,
            &mut self.rtt,
            now,
            header.acks.seqs(),
        );

        Ok((
            acks,
            RecvMessages {
                recv_lanes: &mut self.recv_lanes,
                now,
                packet,
            },
        ))
    }

    fn recv_acks<'session, const N: usize>(
        flushed_packets: &'session mut SeqBuf<FlushedPacket, N>,
        send_lanes: &'session mut [SendLane],
        rtt: &'session mut RttEstimator,
        now: Instant,
        acked_seqs: impl Iterator<Item = PacketSeq> + 'session,
    ) -> impl Iterator<Item = (LaneIndex, MessageSeq)> + 'session {
        acked_seqs
            // we now know that our packet with sequence `seq` was acked by the peer
            // let's find what fragments that packet contained when we flushed it out
            .filter_map(move |seq| flushed_packets.remove_with(seq.0 .0, FlushedPacket::new(now)))
            .flat_map(move |packet| {
                let packet_rtt = now - packet.flushed_at;
                rtt.update(packet_rtt);
                // TODO Rust 1.80: Box::into_iter - https://github.com/rust-lang/rust/issues/59878
                packet.frags.into_vec().into_iter()
            })
            .filter_map(|frag_path| {
                // for each of those fragments, we'll mark that fragment as acked
                let lane_index = usize::try_from(frag_path.lane_index.into_raw())
                    .expect("lane index should fit into a usize");
                let lane = send_lanes
                    .get_mut(lane_index)
                    .expect("frag path should point into a valid lane index");
                // TODO: could these panic instead of returning None silently? is it valid?
                let msg = lane.sent_msgs.get_mut(&frag_path.msg_seq)?;
                let frag_opt = msg.frags.get_mut(usize::from(frag_path.frag_index))?;
                // take this fragment out so it stops being resent
                *frag_opt = None;

                // if all the fragments are now acked, then we report that
                // the entire message is now acked
                if msg.frags.iter().all(Option::is_none) {
                    Some((frag_path.lane_index, frag_path.msg_seq))
                } else {
                    None
                }
            })
    }
}

/// Used to read the fragments in a packet and receive the messages reassembled
/// from those fragments.
///
/// See [`Session::recv`].
#[derive(Debug)]
pub struct RecvMessages<'session> {
    recv_lanes: &'session mut [RecvLane],
    now: Instant,
    packet: Bytes,
}

impl RecvMessages<'_> {
    /// Reads all messages reassembled from this packet and passes them to the
    /// callback provided.
    ///
    /// [`RecvError`]s may be safely ignored.
    pub fn for_each_msg(&mut self, mut f: impl FnMut(Result<(Bytes, LaneIndex), RecvError>)) {
        while self.packet.has_remaining() {
            match self.recv_next_frag() {
                Ok(iter) => iter.map(Ok).for_each(&mut f),
                Err(err) => f(Err(err)),
            }
        }
    }

    fn recv_next_frag(
        &mut self,
    ) -> Result<impl Iterator<Item = (Bytes, LaneIndex)> + '_, RecvError> {
        let frag = self
            .packet
            .read::<Fragment>()
            .map_err(RecvError::DecodeFragment)?;
        let lane_index = frag.header.lane_index;
        let lane_index_u = usize::try_from(lane_index.into_raw())
            .map_err(|_| RecvError::InvalidLaneIndex { lane: lane_index })?;
        let lane = self
            .recv_lanes
            .get_mut(lane_index_u)
            .ok_or(RecvError::InvalidLaneIndex { lane: lane_index })?;
        Ok(lane
            .frags
            .reassemble(
                self.now,
                frag.header.msg_seq,
                frag.header.marker,
                frag.payload,
            )
            .map_err(RecvError::Reassemble)?
            .map(|msg| {
                Self::recv_on_lane(lane, msg, frag.header.msg_seq).map(move |msg| (msg, lane_index))
            })
            .into_iter()
            .flatten())
    }

    fn recv_on_lane(
        lane: &mut RecvLane,
        msg: Bytes,
        msg_seq: MessageSeq,
    ) -> impl Iterator<Item = Bytes> + '_ {
        match &mut lane.kind {
            RecvLaneKind::UnreliableUnordered => {
                // always just return the message
                Either::Left(Some(msg))
            }
            RecvLaneKind::UnreliableSequenced { pending_seq } => {
                if msg_seq < *pending_seq {
                    // msg is older than the message we're expecting to get next, drop it
                    Either::Left(None)
                } else {
                    // msg is the one we're expecting to get or newer, return it
                    *pending_seq = msg_seq + MessageSeq::ONE;
                    Either::Left(Some(msg))
                }
            }
            RecvLaneKind::ReliableUnordered {
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
                        *pending_seq += MessageSeq::ONE;
                        // iter 1: pending_seq: 41, recv_seq_buf: [41, 45]
                        // iter 2: pending_seq: 42, recv_seq_buf: [45]
                    }
                    Either::Left(Some(msg))
                }
            }
            RecvLaneKind::ReliableOrdered {
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
                        *pending_seq += MessageSeq::ONE;
                        Some(msg)
                    }))
                }
            }
        }
        .into_iter()
    }
}
