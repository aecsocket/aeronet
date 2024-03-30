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
//! * a single [`Box`] to store the slice of fragments,
//!   deallocated after either:
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
//! continues until there are no fragments left, at which the iterator returns
//! no more elements.
//!
//! **Allocations:**
//! * a [`Box`] of indices to all fragments in all sent messages,
//!   deallocated after the iterator is dropped.
//!
//! ## Receiving
//!
//! [`buffer_send`]: Messages::buffer_send
//! [`flush`]: Messages::flush

mod recv;
mod send;

use std::marker::PhantomData;

use aeronet::{
    lane::{LaneIndex, LaneKind, LaneOrdering, LaneReliability, OnLane},
    message::{TryFromBytes, TryIntoBytes},
    octs::{BytesError, ConstEncodeSize},
};
use ahash::AHashMap;
use bytes::Bytes;

use crate::{
    ack::Acknowledge,
    frag::{FragmentError, Fragmentation, ReassembleError},
    seq::Seq,
};

/// See the [module-level documentation](self).
#[derive(Debug)]
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

#[derive(Debug)]
struct LaneState<R> {
    reliability: ReliabilityState,
    ordering: OrderingState<R>,
}

#[derive(Debug)]
enum ReliabilityState {
    Unreliable,
    Reliable,
}

#[derive(Debug)]
enum OrderingState<R> {
    Unordered,
    Sequenced { last_recv_msg_seq: Seq },
    Ordered { buf: Vec<R> },
}

impl<R> LaneState<R> {
    fn new(kind: LaneKind) -> Self {
        Self {
            reliability: match kind.reliability() {
                LaneReliability::Unreliable => ReliabilityState::Unreliable,
                LaneReliability::Reliable => ReliabilityState::Reliable,
            },
            ordering: match kind.ordering() {
                LaneOrdering::Unordered => OrderingState::Unordered,
                LaneOrdering::Sequenced => OrderingState::Sequenced {
                    last_recv_msg_seq: Seq(0),
                },
                LaneOrdering::Ordered => OrderingState::Ordered { buf: Vec::new() },
            },
        }
    }

    fn drop_on_flush(&self) -> bool {
        match self.reliability {
            ReliabilityState::Unreliable => true,
            ReliabilityState::Reliable => false,
        }
    }

    fn recv(&mut self, msg: R, msg_seq: Seq) -> impl Iterator<Item = R> {
        match &mut self.ordering {
            OrderingState::Unordered => Some(msg),
            OrderingState::Sequenced { last_recv_msg_seq } => {
                if msg_seq > *last_recv_msg_seq {
                    *last_recv_msg_seq = msg_seq;
                    Some(msg)
                } else {
                    None
                }
            }
            OrderingState::Ordered { buf } => todo!(),
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
pub enum MessageError<S: TryIntoBytes, R: TryFromBytes> {
    #[error("failed to convert message into bytes")]
    IntoBytes(#[source] S::Error),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),

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

const PACKET_HEADER_SIZE: usize = Seq::ENCODE_SIZE + Acknowledge::ENCODE_SIZE;

impl<S: TryIntoBytes + OnLane, R: TryFromBytes + OnLane> Messages<S, R> {
    pub fn new(
        max_packet_size: usize,
        default_packet_cap: usize,
        lanes: impl IntoIterator<Item = LaneKind>,
    ) -> Self {
        assert!(max_packet_size > PACKET_HEADER_SIZE);
        Self {
            lanes: lanes.into_iter().map(LaneState::new).collect(),
            max_packet_size,
            default_packet_cap,
            frags: Fragmentation::new(max_packet_size - PACKET_HEADER_SIZE),
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
    enum MyLane {
        #[lane_kind(UnreliableUnordered)]
        LowPrio,
    }

    #[derive(Debug, Clone, Message, OnLane)]
    #[on_lane(MyLane::LowPrio)]
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
        let mut msgs = Messages::<MyMsg, MyMsg>::new(1024, 1024, [LaneKind::UnreliableUnordered]);
        msgs.buffer_send(MyMsg::from("hello.")).unwrap();
        msgs.buffer_send(MyMsg::from("HELLO!!!")).unwrap();
        msgs.buffer_send(MyMsg::from("small")).unwrap();

        let mut bytes_left = usize::MAX;
        let packets = msgs.flush(&mut bytes_left).collect::<Vec<_>>();

        for mut packet in packets {
            for ack in msgs.read_acks(&mut packet).unwrap() {
                println!("ack: {ack:?}");
            }
            println!("read all acks");
            for result in msgs.read_frags(packet) {
                let msg = result.unwrap();
                println!("got {msg:?}");
            }
        }

        msgs.buffer_send(MyMsg::from("omg another one")).unwrap();
        let packets = msgs.flush(&mut bytes_left).collect::<Vec<_>>();

        for mut packet in packets {
            for ack in msgs.read_acks(&mut packet).unwrap() {
                println!("ack: {ack:?}");
            }
            println!("read all acks");
            for result in msgs.read_frags(packet) {
                let msg = result.unwrap();
                println!("got {msg:?}");
            }
        }
    }
}
