use aeronet::octs::ReadBytes;
use aeronet::{
    lane::OnLane,
    message::{TryFromBytes, TryIntoBytes},
};
use ahash::AHashMap;
use bytes::{Buf, Bytes};

use crate::{ack::Acknowledge, frag::Fragment, seq::Seq};

use super::{FragIndex, MessageError, Messages, SentMessage};

impl<S: TryIntoBytes, R: TryFromBytes + OnLane> Messages<S, R> {
    pub fn read_acks(
        &mut self,
        packet: &mut Bytes,
    ) -> Result<impl Iterator<Item = Seq> + '_, MessageError<S, R>> {
        // mark this packet as acked;
        // this ack will later be sent out to the peer in `flush`
        let packet_seq = packet.read::<Seq>().map_err(MessageError::ReadPacketSeq)?;
        self.acks.ack(packet_seq);

        // read packet seqs the peer has reported they've acked..
        // ..turn those into message seqs via our mappings..
        // ..perform our internal bookkeeping..
        // ..and return those message seqs to the caller
        let acks = packet
            .read::<Acknowledge>()
            .map_err(MessageError::ReadAcks)?;
        let iter =
            Self::packet_to_msg_acks(&self.flushed_packets, &mut self.sent_msgs, acks.seqs());
        Ok(iter)
    }

    fn packet_to_msg_acks<'a>(
        flushed_packets: &'a AHashMap<Seq, Box<[FragIndex]>>,
        sent_msgs: &'a mut AHashMap<Seq, SentMessage>,
        acked_packet_seqs: impl Iterator<Item = Seq> + 'a,
    ) -> impl Iterator<Item = Seq> + 'a {
        acked_packet_seqs
            .filter_map(|acked_packet_seq| flushed_packets.get(&acked_packet_seq).map(|x| x.iter()))
            .flatten()
            .filter_map(|acked_frag| {
                let msg_seq = acked_frag.msg_seq;
                let unacked_msg = sent_msgs.get_mut(&msg_seq)?;
                if let Some(frag_slot) = unacked_msg.frags.get_mut(usize::from(acked_frag.frag_id))
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

    pub fn read_frags(
        &mut self,
        mut packet: Bytes,
    ) -> impl Iterator<Item = Result<R, MessageError<S, R>>> + '_ {
        enum State<I> {
            ReadFrags,
            RecvIter { iter: I },
        }

        let frags = &mut self.frags;
        let lanes = &mut self.lanes;
        let mut state = State::ReadFrags;

        // what the fuck
        std::iter::from_fn(move || 'iter: loop {
            match state {
                State::ReadFrags => {
                    // read in all remaining fragments in this packet
                    'frags: while packet.remaining() > 0 {
                        let frag = match packet
                            .read::<Fragment<Bytes>>()
                            .map_err(MessageError::ReadFragment)
                        {
                            Ok(frag) => frag,
                            Err(err) => return Some(Err(err)),
                        };

                        // reassemble this fragment into a message
                        let msg_bytes = match frags
                            .reassemble(&frag.header, &frag.payload)
                            .map_err(MessageError::Reassemble)
                        {
                            Ok(Some(x)) => x,
                            Ok(None) => continue 'frags,
                            Err(err) => return Some(Err(err)),
                        };
                        let msg = match R::try_from_bytes(Bytes::from(msg_bytes)) {
                            Ok(x) => x,
                            Err(err) => return Some(Err(MessageError::FromBytes(err))),
                        };

                        // get what lane this message is received on
                        let lane_index = msg.lane_index();
                        let lane = match lanes.get_mut(lane_index.into_raw()) {
                            Some(lane) => lane,
                            None => {
                                return Some(Err(MessageError::InvalidLaneIndex { lane_index }))
                            }
                        };

                        // ask the lane what messages it wants to give us - it could:
                        // * just give us the same message back
                        // * give us nothing and drop the message if it's too old (sequenced)
                        // * give us this message plus a bunch of older buffered ones (ordered)
                        let iter = lane.recv(msg, frag.header.msg_seq);
                        state = State::RecvIter { iter };
                        // then get the `State::RecvIter` logic to take over
                        continue 'iter;
                    }
                    return None;
                }
                State::RecvIter { ref mut iter } => match iter.next() {
                    Some(msg) => return Some(Ok(msg)),
                    None => {
                        state = State::ReadFrags;
                    }
                },
            }
        })
    }
}
