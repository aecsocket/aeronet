use aeronet::lane::LaneIndex;
use ahash::AHashMap;
use either::Either;
use octs::{Buf, Bytes, Read, VarInt};
use terrors::OneOf;
use web_time::Instant;

use crate::{
    frag::{Fragment, FragmentReceiver},
    packet::{MessageSeq, PacketHeader, PacketSeq},
};

use super::{FlushedPacket, OutOfMemory, RecvError, RecvLane, SentMessage, Session};

impl Session {
    /// Starts receiving a packet from the peer.
    ///
    /// If the packet header is valid, this will return:
    /// - an iterator over all of our [`MessageSeq`]s that the peer has
    ///   acknowledged
    /// - a [`RecvMessages`], used to actually read the messages that we
    ///   receive
    ///
    /// # Errors
    ///
    /// Errors if the packet has an invalid header.
    ///
    /// Even if this returns [`Ok`], you may still encounter errors when using
    /// [`RecvMessages`].
    pub fn recv(
        &mut self,
        now: Instant,
        mut packet: Bytes,
    ) -> Result<(impl Iterator<Item = MessageSeq> + '_, RecvMessages<'_>), RecvError> {
        self.bytes_recv = self.bytes_recv.saturating_add(packet.len());

        let header = packet
            .read::<PacketHeader>()
            .map_err(RecvError::ReadHeader)?;
        self.acks.ack(header.packet_seq);

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
            &mut self.sent_msgs,
            header.acks.seqs(),
        );

        Ok((
            acks,
            RecvMessages {
                now,
                recv_lanes: &mut self.recv_lanes,
                recv_frags: &mut self.recv_frags,
                recv_frags_cap: self.recv_frags_cap,
                packet,
            },
        ))
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
}

/// Allows reading the messages from a packet given to [`Session::recv`].
///
/// Use [`RecvMessages::for_each_msg`] to iterate through all the messages in
/// the packet.
///
/// In a future version of the crate (when/if generators become stable), this
/// may just become an `Iterator<Item = Bytes>`.
// TODO: ideally this becomes an iterator like `recv_acks`
// but the logic is really hard to make an iterator
// this would be so much easier with coroutines...
#[derive(Debug)]
pub struct RecvMessages<'session> {
    now: Instant,
    recv_lanes: &'session mut [RecvLane],
    recv_frags: &'session mut FragmentReceiver,
    recv_frags_cap: usize,
    packet: Bytes,
}

impl RecvMessages<'_> {
    /// Iterates through all messages in this packet, passing each one to `f`.
    ///
    /// # Errors
    ///
    /// If we fail to read one of the messages for a recoverable reason,
    /// [`RecvError`] is passed to `f`. However, if we fail for a fatal error
    /// e.g. [`OutOfMemory`], this error is returned from this function itself,
    /// and the connection must be closed.
    pub fn for_each_msg(
        &mut self,
        mut f: impl FnMut(Result<(Bytes, LaneIndex), RecvError>),
    ) -> Result<(), OutOfMemory> {
        while self.packet.has_remaining() {
            match Self::recv_next_frag(
                self.now,
                self.recv_lanes,
                self.recv_frags,
                self.recv_frags_cap,
                &mut self.packet,
            ) {
                Ok(iter) => iter.map(Ok).for_each(&mut f),
                Err(err) => match err.narrow::<RecvError, _>() {
                    Ok(err) => f(Err(err)),
                    Err(err) => return Err(err.take()),
                },
            }
        }
        Ok(())
    }

    fn recv_next_frag<'session>(
        now: Instant,
        recv_lanes: &'session mut [RecvLane],
        recv_frags: &'session mut FragmentReceiver,
        recv_frags_cap: usize,
        packet: &mut Bytes,
    ) -> Result<impl Iterator<Item = (Bytes, LaneIndex)> + 'session, OneOf<(RecvError, OutOfMemory)>>
    {
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
            return Ok(Either::Left(std::iter::empty()));
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

        Ok(Either::Right(
            Self::recv_on_lane(lane, msg, msg_seq)
                .map(move |msg| (msg, LaneIndex::from_raw(lane_index))),
        ))
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
