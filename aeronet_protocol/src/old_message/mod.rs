use std::marker::PhantomData;

use aeronet::{
    lane::{LaneConfig, LaneIndex, LaneKind, LaneOrdering, LaneReliability, OnLane},
    message::{TryFromBytes, TryIntoBytes},
};
use ahash::AHashMap;
use bytes::{Buf, Bytes, BytesMut};
use derivative::Derivative;
use enum_dispatch::enum_dispatch;

use crate::{
    ack::Acknowledge,
    frag::{FragHeader, Fragment, FragmentError, Fragmentation, Fragments, ReassembleError},
    octs::{self, ConstEncodeSize, ReadBytes, WriteBytes},
    seq::Seq,
};

#[derive(Debug)]
enum OrderingState<R> {
    Unordered,
    Sequenced { last_recv_msg_seq: Seq },
    Ordered { buf: Vec<R> },
}

impl<R> OrderingState<R> {
    fn new(ordering: LaneOrdering) -> Self {
        match ordering {
            LaneOrdering::Unordered => Self::Unordered,
            LaneOrdering::Sequenced => Self::Sequenced {
                last_recv_msg_seq: Seq(u16::MAX),
            },
            LaneOrdering::Ordered => Self::Ordered { buf: Vec::new() },
        }
    }

    fn recv(&mut self, msg: R, msg_seq: Seq) -> impl Iterator<Item = R> {
        match self {
            Self::Unordered => Some(msg),
            Self::Sequenced { last_recv_msg_seq } => {
                if msg_seq > *last_recv_msg_seq {
                    *last_recv_msg_seq = msg_seq;
                    Some(msg)
                } else {
                    None
                }
            }
            Self::Ordered { buf } => todo!(),
        }
        .into_iter()
    }
}

#[derive(Debug)]
struct LaneState<R> {
    ordering: OrderingState<R>,
}

/*

problem:
* when sending a frag, we need to add it to a vec of outgoing frags
* we need this vec to be sorted before `flush`
  * on insertion?
  * right before the `flush` logic?
* in `flush`, we need to send out all frags which haven't been acked yet
  * if the frag is sent unreliably, no problem, just remove it immediately after
  * if the frag is sent reliably, we keep it in the send buffer, but how/when do
    we remove it?
* when receiving a packet ack, we map it to a (msg_seq, frag_id) -
  we need to then somehow stop `flush` from sending out this frag anymore
  * removing from a map or something?

solution 1:
* fields:
  * send_buf: Vec<SendFrag>
  * acked_frags: AHashSet<(msg_seq, frag_id)>
* on `recv_acks`, add all acked (msg_seq, frag_id) pairs to `acked_frags`
* on `flush`, when we iterate through all `SendFrag`s;
  * if the frag is in `recv_acks`, remove it from both the `send_buf` and
    `acked_frags`, and don't send it
  * PROBLEM: a frag might have been already removed (unreliable frag), so flush
    will never find it, and it will never be removed from `acked_frags`,
    leaking memory
  * MAYBE: clear old entries in `acked_frags`? but that feels hacky

solution 2:
* fields
  * send_buf: AHashMap<Seq, Vec<SentFrag>>
* on `buffer_send`, add the frag to `send_buf`
* on `flush`, iterate thru `send_buf`, get refs to all the frags, and sort it
  by the biggest frags
* on `recv_acks`, remove the (msg_seq, frag_id) pair from `send_buf`
* I like this solution right now

*/

#[derive(Debug)]
pub struct Messages<S, R> {
    // stores current state of lanes, allowing them to influence packet sending
    // and receiving
    lanes: Vec<LaneState<R>>,
    // maximum byte length of a single packet produced by `flush`
    max_packet_len: usize,
    //
    frags: Fragmentation,
    // tracks which packet seqs have been received
    acks: Acknowledge,
    // seq number of the next packet sent out in `flush`
    next_send_packet_seq: Seq,
    // seq number of the next message buffered in `buffer_send`
    next_send_msg_seq: Seq,
    //
    sent_msgs: AHashMap<Seq, SentMessage>,
    // tracks which packets have been sent out, and what frags they contained
    // so that when we receive an ack for that packet, we know what frags have
    // been acked, and therefore what messages have been acked
    flushed_packets: AHashMap<Seq, Vec<FragPath>>,
    _phantom: PhantomData<(S, R)>,
}

#[derive(Debug)]
struct SentMessage {
    lane_index: LaneIndex,
    num_frags: u8,
    num_unacked: u8,
    frags: Box<[Option<Bytes>]>,
}

#[derive(Debug, Clone)]
struct FragPath {
    msg_seq: Seq,
    frag_id: u8,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum MessageError<S: TryIntoBytes, R: TryFromBytes> {
    #[error("failed to convert message to bytes")]
    IntoBytes(#[source] S::Error),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),

    #[error("failed to read packet seq")]
    ReadPacketSeq(#[source] octs::BytesError),
    #[error("failed to read ack header")]
    ReadAckHeader(#[source] octs::BytesError),
    #[error("failed to read fragment")]
    ReadFragment(#[source] octs::BytesError),
    #[error("failed to reassemble fragment")]
    Reassemble(#[source] ReassembleError),
    #[error("failed to create message from bytes")]
    FromBytes(#[source] R::Error),
    #[error("invalid lane index {lane_index:?}")]
    InvalidLaneIndex { lane_index: LaneIndex },
}

#[derive(Derivative, Debug)]
#[derivative(PartialEq, Eq, PartialOrd, Ord)]
struct FlushingFrag {
    payload_len: usize,
    #[derivative(PartialEq = "ignore", PartialOrd = "ignore", Ord = "ignore")]
    num_frags: u8,
    #[derivative(PartialEq = "ignore", PartialOrd = "ignore", Ord = "ignore")]
    path: FragPath,
}

const MIN_PACKET_LEN: usize = Seq::ENCODE_SIZE + Acknowledge::ENCODE_SIZE;

impl<S: TryIntoBytes + OnLane, R: TryFromBytes + OnLane> Messages<S, R> {
    pub fn new(max_packet_len: usize, lanes: impl IntoIterator<Item = LaneConfig>) -> Self {
        assert!(max_packet_len > MIN_PACKET_LEN);
        Self {
            lanes: lanes
                .into_iter()
                .map(|config| LaneState {
                    ordering: OrderingState::new(config.kind.ordering()),
                })
                .collect(),
            max_packet_len,
            frags: Fragmentation::new(max_packet_len - MIN_PACKET_LEN),
            acks: Acknowledge::new(),
            next_send_msg_seq: Seq(0),
            next_send_packet_seq: Seq(0),
            sent_msgs: AHashMap::new(),
            flushed_packets: AHashMap::new(),
            _phantom: PhantomData,
        }
    }

    pub fn buffer_send(&mut self, msg: S) -> Result<Seq, MessageError<S, R>> {
        let lane_index = msg.lane_index();
        let msg_bytes = msg.try_into_bytes().map_err(MessageError::IntoBytes)?;
        let msg_seq = self.next_send_msg_seq.get_inc();
        let frags = self
            .frags
            .fragment(msg_seq, msg_bytes)
            .map_err(MessageError::Fragment)?;
        self.sent_msgs.insert(
            msg_seq,
            SentMessage {
                lane_index,
                num_frags: frags.num_frags(),
                num_unacked: frags.num_frags(),
                frags: frags.map(|frag| Some(frag.payload)).collect(),
            },
        );
        Ok(msg_seq)
    }

    /*
    frags to send:
    * AAAA AAAA AAAA
    * BBBB
    * CCCC CCCC CC
    * DD
    * EEEE EE
    * FFFF FF
    packets sent:
    index  [ .... .... .... .... ]
       #1  [ AAAA AAAA AAAA BBBB ]
       #2  [ CCCC CCCC CCDD .... ]
       #3  [ EEEE EEFF FFFF .... ]

    so basically, pack the biggest fragments we can in first,
    then try to pack as many small fragments in as we can
    on the next packet, again try to pack the biggest ones that we can

    general algo overview:
    * setup
      * collect all fragments in `sent_msgs`
      * sort them by their encoded length into a Vec<Option<_>>
    * iterator
      * if there are no more fragments to consume, return None
      * start building a packet (headers)
      * iterate over all the collected fragments
      * if this fragment can't be put into the packet, skip it
      *

     */
    pub fn flush<'a>(
        &'a mut self,
        available_bytes: &'a mut usize,
    ) -> impl Iterator<Item = Bytes> + 'a {
        std::iter::empty()
        // // collect all fragments to send
        // let mut frags = Self::frags_to_send(&self.sent_msgs)
        //     .map(Some)
        //     .collect::<Vec<_>>();
        // // sort by payload length, largest to smallest
        // frags.sort_unstable_by(|a, b| b.cmp(a));

        // std::iter::from_fn(move || {
        //     if *available_bytes < PACKET_HEADER_LEN {
        //         return None;
        //     }

        //     let packet_seq = self.next_send_packet_seq.get_inc();
        //     let mut packet = BytesMut::with_capacity(self.max_packet_len);
        //     // PANIC SAFETY: `max_packet_len > PACKET_HEADER_LEN` is asserted on construction
        //     // and encoding these values takes `PACKET_HEADER_LEN` bytes
        //     packet.write(&packet_seq).unwrap();
        //     packet.write(&self.acks).unwrap();
        //     *available_bytes -= PACKET_HEADER_LEN;

        //     for frag in Self::next_frags_in_packet(&mut frags, &mut usize::MAX) {}

        //     Some(packet.freeze())
        // })

        // /*
        // // collect all frags to be flushed and wrap them in an Option
        // // when we remove frags from this, we just take the Option
        // // don't remove items to retain order; just skip over Nones
        // // when we find which fragments to send
        // let mut frags = Self::frags_to_send(&self.sent_msgs, &self.lanes, max_frags_len)
        //     .map(Some)
        //     .collect::<Box<_>>();
        // // sort largest to smallest
        // frags.sort_unstable_by(|a, b| b.cmp(a));

        // std::iter::from_fn(move || {
        //     if *available_bytes < PACKET_HEADER_LEN {
        //         return None;
        //     }

        //     let packet_seq = self.next_send_packet_seq.get_inc();
        //     let mut packet = BytesMut::with_capacity(self.max_packet_len);
        //     // PANIC SAFETY: `max_packet_len > PACKET_HEADER_LEN` is asserted on construction
        //     // and encoding these values takes `PACKET_HEADER_LEN` bytes
        //     packet_seq.encode(&mut packet).unwrap();
        //     self.ack.header().encode(&mut packet).unwrap();
        //     *available_bytes -= PACKET_HEADER_LEN;

        //     let available_bytes_for_frags = (*available_bytes).min(self.max_packet_len);
        //     let mut available_bytes_for_frags_after = available_bytes_for_frags;
        //     let mut sent_frags = Vec::new();
        //     for frag in Self::next_frags_in_packet(&mut frags, &mut available_bytes_for_frags_after)
        //     {
        //         frag.encode(&mut packet).unwrap();
        //         sent_frags.push(FlushedFrag {
        //             msg_seq: frag.header.msg_seq,
        //             frag_id: frag.header.frag_id,
        //         });
        //     }
        //     *available_bytes -= available_bytes_for_frags - available_bytes_for_frags_after;

        //     if sent_frags.is_empty() {
        //         // if we can't send any more fragments,
        //         // then there must be no more buffered fragments for sending
        //         debug_assert!(self.sent_msgs)
        //         return None;
        //     }

        //     // we've fully built the packet that we're about to send out;
        //     // track its packet sequence, and what frags it contained
        //     // so that when we receive an ack for this packet, we know what frags
        //     // have been acked, and therefore what messages have been acked
        //     self.flushed_packets.insert(packet_seq, sent_frags);

        //     Some(packet.freeze())
        // })*/
    }

    fn frags_to_send<'a>(
        sent_msgs: &'a AHashMap<Seq, SentMessage>,
    ) -> impl Iterator<Item = FlushingFrag> + 'a {
        sent_msgs.iter().flat_map(|(msg_seq, msg)| {
            msg.frags.iter().filter_map(Option::as_ref).enumerate().map(
                move |(frag_id, payload)| FlushingFrag {
                    payload_len: payload.len(),
                    num_frags: msg.num_frags,
                    path: FragPath {
                        msg_seq: *msg_seq,
                        frag_id: u8::try_from(frag_id).unwrap(),
                    },
                },
            )
        })
    }

    fn next_frags_in_packet<'a>(
        frags: &'a mut [Option<FlushingFrag>],
        available_bytes: &'a mut usize,
    ) -> impl Iterator<Item = Fragment> + 'a {
        frags.iter_mut().filter_map(|frag_opt| {
            let frag = frag_opt.take()?;
            if frag.payload_len < *available_bytes {
                *frag_opt = Some(frag);
                return None;
            }

            todo!()
            // match frag.lane_reliability {
            //     LaneReliability::Unreliable => {
            //         // consume this fragment; we won't ever send it again
            //         Some(frag.frag)
            //     }
            //     LaneReliability::Reliable => {
            //         // keep this fragment around, we might need to resend it later
            //         let frag_clone = frag.frag.clone();
            //         *frag_opt = Some(frag);
            //         Some(frag_clone)
            //     }
            // }
        })
    }

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
            .map_err(MessageError::ReadAckHeader)?;
        let iter =
            Self::packet_to_msg_acks(&self.flushed_packets, &mut self.sent_msgs, acks.seqs());
        Ok(iter)
    }

    fn packet_to_msg_acks<'a>(
        sent_packets: &'a AHashMap<Seq, Vec<FragPath>>,
        sent_msgs: &'a mut AHashMap<Seq, SentMessage>,
        acked_packet_seqs: impl Iterator<Item = Seq> + 'a,
    ) -> impl Iterator<Item = Seq> + 'a {
        acked_packet_seqs
            .filter_map(|acked_packet_seq| sent_packets.get(&acked_packet_seq))
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
                            .read::<Fragment>()
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
                        let iter = lane.ordering.recv(msg, frag.header.msg_seq);
                        state = State::RecvIter { iter };
                        // then get the `State::Recv` logic to take over
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
