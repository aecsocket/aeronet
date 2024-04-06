//! Manages sending and receiving messages as packets composed of bytes.
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
//! sorted based on their payload length, largest to smallest. Then, we try to
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

mod lane;
mod recv;
mod send;

use std::{fmt::Debug, marker::PhantomData};

use aeronet::{
    lane::{LaneIndex, LaneKind, LaneMapper},
    message::BytesMapper,
    octs::{BytesError, ConstEncodeLen},
};
use ahash::AHashMap;
use bytes::Bytes;
use derivative::Derivative;

use crate::{
    ack::Acknowledge,
    frag::{FragmentError, Fragmentation, ReassembleError},
    seq::Seq,
};

use self::lane::{LaneRecv, LaneSend};

// todo docs
/// See the [module-level documentation](self).
#[derive(Derivative)]
#[derivative(Debug(bound = "M: Debug"))]
pub struct Packets<S, R, M> {
    lanes_in: Box<[LaneRecv<R>]>,
    lanes_out: Box<[LaneSend]>,
    max_packet_len: usize,
    default_packet_cap: usize,
    frags: Fragmentation,
    acks: Acknowledge,
    next_send_packet_seq: Seq,
    next_send_msg_seq: Seq,
    sent_msgs: AHashMap<Seq, SentMessage>,
    flushed_packets: AHashMap<Seq, Box<[FragIndex]>>,
    mapper: M,
    _phantom: PhantomData<S>,
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

#[derive(Debug, thiserror::Error)]
pub enum SendError<E> {
    #[error("failed to convert message into bytes")]
    IntoBytes(#[source] E),
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
}

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

const PACKET_HEADER_LEN: usize = Seq::ENCODE_LEN + Acknowledge::ENCODE_LEN;

impl<S, R, M> Packets<S, R, M>
where
    M: BytesMapper<S> + BytesMapper<R> + LaneMapper<S> + LaneMapper<R>,
{
    #[must_use]
    pub fn new(
        max_packet_len: usize,
        default_packet_cap: usize,
        lanes_in: &[LaneKind],
        lanes_out: &[LaneKind],
        mapper: M,
    ) -> Self {
        assert!(max_packet_len > PACKET_HEADER_LEN);
        Self {
            lanes_in: lanes_in.iter().map(|kind| LaneRecv::new(*kind)).collect(),
            lanes_out: lanes_out.iter().map(|kind| LaneSend::new(*kind)).collect(),
            max_packet_len,
            default_packet_cap,
            frags: Fragmentation::new(max_packet_len - PACKET_HEADER_LEN),
            acks: Acknowledge::new(),
            next_send_msg_seq: Seq(0),
            next_send_packet_seq: Seq(0),
            sent_msgs: AHashMap::new(),
            flushed_packets: AHashMap::new(),
            mapper,
            _phantom: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, string::FromUtf8Error};

    use aeronet::{
        lane::{LaneKey, OnLane},
        message::{Message, TryFromBytes, TryIntoBytes},
    };

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

    const MAX_PACKET_LEN: usize = 1024;

    #[test]
    fn test() {
        let mut msgs = Packets::<MyMsg, MyMsg, ()>::new(
            MAX_PACKET_LEN,
            MAX_PACKET_LEN,
            MyLane::KINDS,
            MyLane::KINDS,
            (),
        );
        msgs.buffer_send(MyMsg::from("1")).unwrap();
        msgs.buffer_send(MyMsg::from("2")).unwrap();

        let mut bytes_left = 45;
        let packets1 = msgs.flush(&mut bytes_left).collect::<Vec<_>>();

        msgs.buffer_send(MyMsg::from("3")).unwrap();
        let packets2 = msgs.flush(&mut bytes_left).collect::<Vec<_>>();
        dbg!(packets1[0].len(), packets2[0].len(), bytes_left);

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
