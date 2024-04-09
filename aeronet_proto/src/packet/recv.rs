use std::marker::PhantomData;

use aeronet::{
    lane::{LaneIndex, LaneMapper},
    message::BytesMapper,
    octs::{BytesError, ReadBytes},
};
use ahash::AHashMap;
use bytes::{Buf, Bytes};
use derivative::Derivative;

use crate::{
    ack::Acknowledge,
    frag::{Fragment, FragmentReceiver, ReassembleError},
    seq::Seq,
};

use super::{lane::LaneReceiver, FlushedPacket, PacketManager, SentMessage};

#[derive(Debug, thiserror::Error)]
pub enum RecvError<E> {
    #[error("failed to read packet sequence")]
    ReadPacketSeq(#[source] BytesError),
    #[error("failed to read acks")]
    ReadAcks(#[source] BytesError),
    #[error("failed to read fragment")]
    ReadFragment(#[source] BytesError),
    #[error("failed to reassemble message")]
    Reassemble(#[source] ReassembleError),
    #[error("failed to create message from bytes")]
    FromBytes(#[source] E),
    #[error("invalid lane index {lane_index:?}")]
    InvalidLaneIndex { lane_index: LaneIndex },
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct PacketReceiver<R, M> {
    recv_lanes: Box<[LaneReceiver<R>]>,
    // insertion policy: on recv
    // removal policy: oh shit TODO
    recv_frags: FragmentReceiver,
    _phantom: PhantomData<M>,
}

impl<S, R, M: BytesMapper<R> + LaneMapper<R>> PacketManager<S, R, M> {
    pub fn recv<'a>(&'a mut self, packet: Bytes) -> ReadAcks<'a, R, M> {
        self.total_bytes_recv = self.total_bytes_recv.saturating_add(packet.len());
        ReadAcks {
            recv_lanes: &mut self.recv_lanes,
            recv_frags: &mut self.recv_frags,
            acks: &mut self.acks,
            msgs_recv: &mut self.msgs_recv,
            msg_bytes_recv: &mut self.msg_bytes_recv,
            flushed_packets: &mut self.flushed_packets,
            sent_msgs: &mut self.sent_msgs,
            mapper: &self.mapper,
            packet,
            _phantom: PhantomData,
        }
    }
}

pub struct ReadAcks<'a, R, M> {
    recv_lanes: &'a mut [LaneReceiver<R>],
    recv_frags: &'a mut FragmentReceiver,
    acks: &'a mut Acknowledge,
    msgs_recv: &'a mut usize,
    msg_bytes_recv: &'a mut usize,
    flushed_packets: &'a mut AHashMap<Seq, FlushedPacket>,
    sent_msgs: &'a mut AHashMap<Seq, SentMessage>,
    mapper: &'a M,
    packet: Bytes,
    _phantom: PhantomData<(R, M)>,
}

impl<'a, R, M: BytesMapper<R>> ReadAcks<'a, R, M> {
    /// Reads the [`Acknowledge`] header of a packet, and returns an iterator of
    /// all acknowledged **mesage** sequence numbers.
    ///
    /// # Errors
    ///
    /// Errors if the packet did not contain a valid acknowledge header.
    pub fn read_acks(
        mut self,
    ) -> Result<(impl Iterator<Item = Seq> + 'a, ReadFrags<'a, R, M>), RecvError<M::FromError>>
    {
        // mark this packet as acked;
        // this ack will later be sent out to the peer in `flush`
        let packet_seq = self
            .packet
            .read::<Seq>()
            .map_err(RecvError::ReadPacketSeq)?;
        self.acks.ack(packet_seq);

        // read packet seqs the peer has reported they've acked..
        // ..turn those into message seqs via our mappings..
        // ..perform our internal bookkeeping..
        // ..and return those message seqs to the caller
        let acks = self
            .packet
            .read::<Acknowledge>()
            .map_err(RecvError::ReadAcks)?;
        self.flushed_packets
            .retain(|_, packet| packet.num_unacked > 0);
        let iter = Self::packet_to_msg_acks(self.flushed_packets, self.sent_msgs, acks.seqs());
        Ok((
            iter,
            ReadFrags {
                recv_lanes: self.recv_lanes,
                recv_frags: self.recv_frags,
                msgs_recv: self.msgs_recv,
                msg_bytes_recv: self.msg_bytes_recv,
                mapper: self.mapper,
                packet: self.packet,
            },
        ))
    }

    fn packet_to_msg_acks(
        flushed_packets: &'a AHashMap<Seq, FlushedPacket>,
        sent_msgs: &'a mut AHashMap<Seq, SentMessage>,
        acked_packet_seqs: impl Iterator<Item = Seq> + 'a,
    ) -> impl Iterator<Item = Seq> + 'a {
        acked_packet_seqs
            .filter_map(|acked_packet_seq| {
                flushed_packets
                    .get(&acked_packet_seq)
                    .map(|packet| packet.frags.iter())
            })
            .flatten()
            .filter_map(|acked_frag| {
                let msg_seq = acked_frag.msg_seq;
                let unacked_msg = sent_msgs.get_mut(&msg_seq)?;
                let frag = unacked_msg
                    .frags
                    .get_mut(usize::from(acked_frag.frag_index))
                    .and_then(|x| x.as_mut());
                // TODO FIX

                // do internal bookkeeping
                if let Some(frag_slot) = unacked_msg
                    .frags
                    .get_mut(usize::from(acked_frag.frag_index))
                {
                    // mark this frag as acked
                    unacked_msg.num_unacked -= 1;

                    *frag_slot = None;
                }

                if unacked_msg.num_unacked == 0 {
                    // message is no longer unacked,
                    // we've just acked all the fragments
                    sent_msgs.remove(&msg_seq);
                    Some(msg_seq)
                } else {
                    None
                }
            })
    }
}

pub struct ReadFrags<'a, R, M> {
    recv_lanes: &'a mut [LaneReceiver<R>],
    recv_frags: &'a mut FragmentReceiver,
    msgs_recv: &'a mut usize,
    msg_bytes_recv: &'a mut usize,
    mapper: &'a M,
    packet: Bytes,
}

impl<'a, R, M: BytesMapper<R> + LaneMapper<R>> ReadFrags<'a, R, M> {
    /// Reads the next message fragment present in the given packet, and returns
    /// the reassembled message(s) that result from reassembling this fragment.
    ///
    /// This must be called in a loop on the same packet until this returns
    /// `Ok(None)` or `Err`.
    ///
    /// # Errors
    ///
    /// Errors if it could not read the next fragment in the packet.
    pub fn read_next_frag(
        &'_ mut self,
    ) -> Result<Option<impl Iterator<Item = R> + '_>, RecvError<M::FromError>> {
        while self.packet.has_remaining() {
            let frag = self
                .packet
                .read::<Fragment<Bytes>>()
                .map_err(RecvError::ReadFragment)?;
            *self.msg_bytes_recv = self.msg_bytes_recv.saturating_add(frag.payload.len());

            // reassemble this fragment into a message
            let Some(msg_bytes) = self
                .recv_frags
                .reassemble(&frag.header, &frag.payload)
                .map_err(RecvError::Reassemble)?
            else {
                continue;
            };

            let msg_bytes = Bytes::from(msg_bytes);
            let msg = self
                .mapper
                .try_from_bytes(msg_bytes)
                .map_err(RecvError::FromBytes)?;
            *self.msgs_recv = self.msgs_recv.saturating_add(1);

            // get what lane this message is received on
            let lane_index = self.mapper.lane_index(&msg);
            let lane = self
                .recv_lanes
                .get_mut(lane_index.into_raw())
                .ok_or(RecvError::InvalidLaneIndex { lane_index })?;

            // ask the lane what messages it wants to give us, in response to
            // receiving this message
            return Ok(Some(lane.recv(frag.header.msg_seq, msg)));
        }
        Ok(None)
    }
}
