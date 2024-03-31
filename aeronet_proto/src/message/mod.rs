//! Manages sending and receiving messages on the byte level.
//!
//! This is a high-level utility which most networked aeronet transport
//! implementations will use - the transport implementation handles sending and
//! receiving bytes, while this utility handles the bytes themselves.
//!
//! # Outline
//!
//! ## Sending
//!
//! ### [`buffer_send`]
//!
//! The message that you pass into this function will be enqueued for sending on
//! the next [`flush`]. The message is assigned a new message sequence,
//! converted into bytes, and fragmented into smaller fragments. The new
//! generated message info (such as how many fragments are left to be
//! acknowledged, and which fragments are left to send) are then tracked.
//!
//! **Allocations:**
//! * a single [`Box`] to store the slice of fragments, deallocated after
//!   either:
//!   * the message is on an unreliable lane, and the message has been flushed
//!   * the message is on a reliable lane, and all fragments of the message have
//!     been acknowledged
//!
//! ### [`flush`]
//!
//! The messages previously enqueued in [`buffer_send`] will now be formed into
//! packets, which can be sent to the peer. The function returns an iterator
//! of these flushed packets instead of e.g. a [`Vec`], to avoid as many
//! allocations as possible.
//!
//! Firstly, all individual fragments from the sent messages are collected and
//! sorted based on their payload size, largest to smallest. Then, we try to
//! pack the largest fragments into a packet first, then fill the space with any
//! smaller packets which still fit.
//!
//! After a packet is full (that is, there is no fragment left which is small
//! enough to fit in the remaining space of our current packet), we return the
//! current packet from the iterator, and start work on the next one. This
//! continues until there are no fragments left, at which point the iterator
//! returns [`None`].
//!
//! **Allocations:**
//! * a [`Box`] of indices to all fragments in all sent messages, deallocated
//!   after the iterator is dropped.
//!
//! ## Receiving
//!
//! After the transport receives a [`Bytes`] packet, it will want to pass it
//! here to process it.
//!
//! ### [`read_acks`]
//!
//! Firstly, run [`read_acks`] on the packet to find which message [`Seq`]s the
//! peer has received and acknowledged. This will read the packet seqs, perform
//! bookkeeping, and convert the packet seqs to message seqs.
//!
//! **Allocations:** none
//!
//! ### [`read_next_frag`]
//!
//! Afterwards, run [`read_next_frag`] in a loop on the same packet to read the
//! fragments inside the packet. [`Messages`] will automatically read fragments,
//! reassemble them into full messages, and potentially return the message that
//! it reads.
//!
//! Depending on the lane that the deserialized message is on, the lane may
//! choose to:
//! * immediately return the message (unordered)
//! * return the message if it's strictly newer than the last received message
//!   (sequenced)
//! * buffer the message and return all messages in order up to what it's
//!   already received (ordered)
//!
//! Once the function returns `Ok(None)`, all fragments have been read and the
//! packet has been fully consumed.
//!
//! **Allocations:** none
//!
//! [`buffer_send`]: Messages::buffer_send
//! [`flush`]: Messages::flush
//! [`read_acks`]: Messages::read_acks
//! [`read_frags`]: Messages::read_frags

/*
TODO:
* if we `flush` and we don't produce any packets, then we should produce a
  single packet with only the ack header
  * we should only produce 1 of these packets per X, where X is configurable
* after flushing a reliable fragment, we should start a timer - don't resend
  this same fragment until the timer elapses (e.g. 100ms). So assuming we flush
  each 50ms:
  * t=0: fragment is sent
  * t=50: ...
  * t=100: fragment is sent again
  * t=150: fragment is sent again
  * t=200: fragment is sent again
  * repeats until the fragment is acked
    * or the fragment times out and the connection dies!
*/

mod recv;
mod send;

use std::{borrow::Borrow, marker::PhantomData};

use aeronet::{
    lane::{LaneIndex, LaneKind, LaneOrdering, LaneReliability, OnLane},
    message::{TryFromBytes, TryIntoBytes},
    octs::{BytesError, ConstEncodeLen},
};
use ahash::{AHashMap, AHashSet};
use bytes::Bytes;
use derivative::Derivative;
use either::Either;

use crate::{
    ack::Acknowledge,
    frag::{FragmentError, Fragmentation, ReassembleError},
    seq::Seq,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessagesConfig {
    pub max_packet_size: usize,
    pub default_packet_cap: usize,
}

// todo docs
/// See the [module-level documentation](self).
#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Messages<S, R> {
    lanes: Box<[LaneState<R>]>,
    max_packet_size: usize,
    default_packet_cap: usize,
    frags: Fragmentation,
    acks: Acknowledge,
    next_send_packet_seq: Seq,
    next_send_msg_seq: Seq,
    sent_msgs: AHashMap<Seq, SentMessage>,
    flushed_packets: AHashMap<Seq, Box<[FragIndex]>>,
    _phantom: PhantomData<(S, R)>,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
struct LaneState<R> {
    reliability: ReliabilityState,
    ordering: OrderingState<R>,
}

#[derive(Debug)]
enum ReliabilityState {
    Unreliable,
    Reliable,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
enum OrderingState<R> {
    Unordered {
        next_pending_msg_seq: Seq,
        recv_seq_buf: AHashSet<Seq>,
    },
    Sequenced {
        last_recv_msg_seq: Seq,
    },
    Ordered {
        next_pending_msg_seq: Seq,
        #[derivative(Debug = "ignore")]
        recv_buf: AHashMap<Seq, R>,
    },
}

impl<R> LaneState<R> {
    fn new(kind: LaneKind) -> Self {
        Self {
            reliability: match kind.reliability() {
                LaneReliability::Unreliable => ReliabilityState::Unreliable,
                LaneReliability::Reliable => ReliabilityState::Reliable,
            },
            ordering: match kind.ordering() {
                LaneOrdering::Unordered => OrderingState::Unordered {
                    next_pending_msg_seq: Seq(0),
                    recv_seq_buf: AHashSet::new(),
                },
                LaneOrdering::Sequenced => OrderingState::Sequenced {
                    last_recv_msg_seq: Seq::MAX,
                },
                LaneOrdering::Ordered => OrderingState::Ordered {
                    next_pending_msg_seq: Seq(0),
                    recv_buf: AHashMap::new(),
                },
            },
        }
    }

    fn drop_on_flush(&self) -> bool {
        match self.reliability {
            ReliabilityState::Unreliable => true,
            ReliabilityState::Reliable => false,
        }
    }

    // TODO coroutines
    fn recv(&mut self, msg: R, msg_seq: Seq) -> impl Iterator<Item = R> + '_ {
        // Either::Left: Option::IntoIter
        // Either::Right: vec::Drop
        match &mut self.ordering {
            OrderingState::Unordered { .. } => Either::Left(Some(msg)),
            OrderingState::Sequenced { last_recv_msg_seq } => {
                // purposefully drop message which have the same seq as
                // `last_recv_msg_seq` - they're probably duplicate packets
                if msg_seq > *last_recv_msg_seq {
                    *last_recv_msg_seq = msg_seq;
                    Either::Left(Some(msg))
                } else {
                    Either::Left(None)
                }
            }
            OrderingState::Ordered {
                next_pending_msg_seq,
                recv_buf,
            } => {
                // add the message to the buffer
                // if the message is older than the next expected one,
                // then it's probably a duplicate packet - drop the message
                if msg_seq >= *next_pending_msg_seq {
                    recv_buf.insert(msg_seq, msg);
                }

                // return all the messages we've buffered in order
                // if we get to a message which we don't have, then the iter ends
                // and we have to wait for it to be received
                Either::Right(std::iter::from_fn(move || {
                    let msg = recv_buf.remove(&next_pending_msg_seq)?;
                    *next_pending_msg_seq += Seq(1);
                    Some(msg)
                }))
            }
        }
        .into_iter()
    }
}

#[derive(Debug)]
struct SentMessage {
    lane_index: usize,
    num_frags: u8,
    num_unacked: u8,
    frags: Box<[Option<Bytes>]>,
}

#[derive(Debug, Clone, Copy)]
struct FragIndex {
    msg_seq: Seq,
    frag_id: u8,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SendMessageError<S: TryIntoBytes> {
    #[error("failed to convert message into bytes")]
    IntoBytes(#[source] S::Error),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RecvMessageError<R: TryFromBytes> {
    #[error("failed to read packet sequence")]
    ReadPacketSeq(#[source] BytesError),
    #[error("failed to read acks")]
    ReadAcks(#[source] BytesError),
    #[error("failed to read fragment")]
    ReadFragment(#[source] BytesError),
    #[error("failed to reassemble message")]
    Reassemble(#[source] ReassembleError),
    #[error("failed to create message from bytes")]
    FromBytes(#[source] R::Error),
    #[error("invalid lane index {lane_index:?}")]
    InvalidLaneIndex { lane_index: LaneIndex },
}

const PACKET_HEADER_LEN: usize = Seq::ENCODE_LEN + Acknowledge::ENCODE_LEN;

impl<S: TryIntoBytes + OnLane, R: TryFromBytes + OnLane> Messages<S, R> {
    pub fn new(
        max_packet_size: usize,
        default_packet_cap: usize,
        lanes: impl IntoIterator<Item = impl Borrow<LaneKind>>,
    ) -> Self {
        assert!(max_packet_size > PACKET_HEADER_LEN);
        Self {
            lanes: lanes
                .into_iter()
                .map(|kind| LaneState::new(*kind.borrow()))
                .collect(),
            max_packet_size,
            default_packet_cap,
            frags: Fragmentation::new(max_packet_size - PACKET_HEADER_LEN),
            acks: Acknowledge::new(),
            next_send_msg_seq: Seq(0),
            next_send_packet_seq: Seq(0),
            sent_msgs: AHashMap::new(),
            flushed_packets: AHashMap::new(),
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, string::FromUtf8Error};

    use aeronet::{lane::LaneKey, message::Message};

    use super::*;

    #[derive(Debug, Clone, Copy, LaneKey)]
    #[lane_kind(ReliableUnordered)]
    struct MyLane;

    #[derive(Debug, Clone, Message, OnLane)]
    #[on_lane(MyLane)]
    struct MyMsg(String);

    impl<T: Into<String>> From<T> for MyMsg {
        fn from(value: T) -> Self {
            Self(value.into())
        }
    }

    impl TryIntoBytes for MyMsg {
        type Error = Infallible;

        fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
            Ok(self.0.into())
        }
    }

    impl TryFromBytes for MyMsg {
        type Error = FromUtf8Error;

        fn try_from_bytes(buf: Bytes) -> Result<Self, Self::Error> {
            String::from_utf8(buf.into()).map(MyMsg)
        }
    }

    #[test]
    fn test() {
        let mut msgs = Messages::<MyMsg, MyMsg>::new(1024, 1024, MyLane::KINDS);
        msgs.buffer_send(MyMsg::from("1")).unwrap();
        msgs.buffer_send(MyMsg::from("2")).unwrap();

        let mut bytes_left = usize::MAX;
        let packets1 = msgs.flush(&mut bytes_left).collect::<Vec<_>>();

        msgs.buffer_send(MyMsg::from("3")).unwrap();
        let packets2 = msgs.flush(&mut bytes_left).collect::<Vec<_>>();

        let mut read = |packets| {
            for mut packet in packets {
                for ack in msgs.read_acks(&mut packet).unwrap() {
                    println!("ack: {ack:?}");
                }
                println!("read all acks");
                while let Some(msgs) = msgs.read_next_frag(&mut packet).unwrap() {
                    for msg in msgs {
                        println!("got {msg:?}");
                    }
                }
            }
        };

        read(packets2);
        read(packets1);
    }
}
