use std::{cmp, mem, array};

use bitcode::{Decode, Encode};
use derivative::Derivative;

use crate::util::SparseBuffer;

/// Sequence number of a message.
///
/// This value uniquely identifies this message on both sides of the connection
/// (as long as the sequence counter hasn't wrapped around yet, since this is
/// just a `u16` after all).
/// 
/// Note that this identifies the **message**, not the **chunk** - for a message
/// which has been split into multiple chunks, all of its packets will have the
/// same sequence number.
///
/// See https://gafferongames.com/post/packet_fragmentation_and_reassembly/, *Fragment Packet Structure*.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Ord, Hash, Encode, Decode)]
pub struct Seq(pub u16);

impl PartialOrd for Seq {
    /// Gets if the sequence number `self` is logically greater than the
    /// sequence number `other`.
    ///
    /// See https://gafferongames.com/post/reliability_ordering_and_congestion_avoidance_over_udp/,
    /// *Handling Sequence Number Wrap-Around*.
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        const HALF: u16 = u16::MAX / 2;

        let s1 = self.0;
        let s2 = other.0;

        if s1 == s2 {
            return Some(cmp::Ordering::Equal);
        }

        if ((s1 > s2) && (s1 - s2 <= HALF)) || ((s1 < s2) && (s2 - s1 > HALF)) {
            Some(cmp::Ordering::Greater)
        } else {
            Some(cmp::Ordering::Less)
        }
    }
}

// packets

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct PacketHeader {
    pub seq: Seq,
    pub chunk_idx: u8,
    pub num_chunks: u8,
}

/// Maximum size of a packet which can be sent as a single datagram over a
/// network connection.
///
/// The maximum transmission unit (MTU) of a connection determines how big a
/// single packet can be before it gets dropped by a computer in the connection.
/// If you want to transmit data larger than the MTU, you must break the data up
/// into smaller packets and transmit those individually, then reassemble them
/// on the other end - this is called fragmentation.
///
/// This constant provides a reasonable packet size which should work for most
/// connections without dropping packets, while being large enough to transmit
/// a fair amount of data. Although there are methods to find this value, such
/// as path MTU discovery, this is too complicated for this crate.
///
/// Note that this value is not a hard limit - transports may send packets which
/// are larger, e.g. if they want to add their own frames and metadata, but this
/// crate's protocol implementation will limit the maximum packet size to this
/// value. It is best to stay as close to this limit as possible.
///
/// See https://gafferongames.com/post/packet_fragmentation_and_reassembly/, *MTU in the real world*.
#[doc(alias = "mtu")]
#[doc(alias = "default_mtu")]
pub const MAX_PACKET_SIZE: usize = 1024;

/// Encoded size of a [`PacketHeader`].
///
/// This must be a constant size for all values of [`PacketHeader`], as it is
/// used to determine the rest of the layout of the packet.
pub const PACKET_HEADER_SIZE: usize = mem::size_of::<PacketHeader>();

/// Maximum size of a user-specified payload which can fit into the
/// [`MAX_PACKET_SIZE`].
pub const MAX_PAYLOAD_SIZE: usize = MAX_PACKET_SIZE - PACKET_HEADER_SIZE;

/// Maximum number of chunks that a single message can be split into.
///
/// The packet header stores the metadata for chunking, and the header is
/// limited in size, so the number of chunks is naturally also limited.
pub const MAX_NUM_CHUNKS: u8 = u8::MAX;

/// Error that occurs when dealing with packets in a [`LaneState`].
#[derive(Debug, thiserror::Error)]
pub enum PacketError {
    /// Attempted to split a message into chunks, but there were too many chunks
    /// for a single message.
    #[error("requested more than {MAX_NUM_CHUNKS} chunks")]
    TooManyChunks,
    /// Incoming packet was too small to hold any data.
    #[error("too small")]
    TooSmall,
    #[error("failed to decode header")]
    DecodeHeader(#[source] bitcode::Error),
    #[error("invalid chunk")]
    InvalidChunk,
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct LaneState {
    next_seq: Seq,
    msgs_recv: Box<SparseBuffer<MessageData, 256>>,
}

#[derive(Debug, Clone)]
struct MessageData {
    num_chunks: u8,
    chunks_recv: u8,
    chunks: Box<[Vec<u8>; MAX_NUM_CHUNKS as usize]>,
}

impl Default for MessageData {
    fn default() -> Self {
        Self {
            num_chunks: 0,
            chunks_recv: 0,
            chunks: Box::new(array::from_fn(|_| Vec::new())),
        }
    }
}

impl LaneState {
    pub fn new() -> Self {
        assert_eq!(
            PACKET_HEADER_SIZE,
            bitcode::encode(&PacketHeader::default()).unwrap().len()
        );
        Self {
            next_seq: Seq::default(),
            msgs_recv: Box::new(SparseBuffer::default()),
        }
    }

    fn next_seq(&mut self) -> Seq {
        let seq = self.next_seq;
        self.next_seq.0 = seq.0.wrapping_add(1);
        seq
    }

    pub fn chunk(&mut self, buf: &[u8]) -> Result<Vec<Vec<u8>>, PacketError> {
        let chunks = buf.chunks(MAX_PAYLOAD_SIZE);
        let num_chunks = u8::try_from(chunks.len())
            .map_err(|_| PacketError::TooManyChunks)?;

        let seq = self.next_seq();
        let chunks = chunks
            .enumerate()
            .map(|(chunk_idx, chunk)| {
                let chunk_idx = u8::try_from(chunk_idx)
                    .expect("should not be more than `MAX_NUM_CHUNKS` values in this iterator");

                let header = PacketHeader {
                    seq,
                    chunk_idx,
                    num_chunks,
                };

                let mut bytes =
                    bitcode::encode(&header).expect("should be able to serialize header");
                debug_assert_eq!(PACKET_HEADER_SIZE, bytes.len());

                bytes.extend(chunk);
                debug_assert!(bytes.len() <= MAX_PACKET_SIZE);

                bytes
            })
            .collect();
        Ok(chunks)
    }

    pub fn recv(&mut self, buf: &[u8]) -> Result<Option<Vec<u8>>, PacketError> {
        if buf.len() < PACKET_HEADER_SIZE {
            return Err(PacketError::TooSmall);
        }

        let header: PacketHeader = bitcode::decode(&buf[..PACKET_HEADER_SIZE])
            .map_err(PacketError::DecodeHeader)?;
        if header.chunk_idx >= header.num_chunks {
            return Err(PacketError::InvalidChunk);
        }
        let payload = &buf[PACKET_HEADER_SIZE..];
        
        let seq = usize::from(header.seq.0);
        let msg_data = match self.msgs_recv.get_mut(seq) {
            Some(msg_data) => msg_data,
            None => self.msgs_recv.insert(seq, MessageData::default()),
        };
        if header.num_chunks != msg_data.num_chunks {
            // We're about to overwrite a previous message
            msg_data.num_chunks = header.num_chunks;
            msg_data.chunks_recv = 0;
            msg_data.chunks.fill_with(Vec::new);
        }
        
        msg_data.chunks_recv += 1;
        msg_data.chunks[usize::from(header.chunk_idx)] = payload.to_vec();

        if msg_data.chunks_recv >= msg_data.num_chunks {
            // This message has been fully received;
            // clear it and return the message
            let msg_buf = (0..msg_data.num_chunks)
                .map(|i| mem::take(&mut msg_data.chunks[usize::from(i)]))
                .flatten()
                .collect();
            self.msgs_recv.remove(seq);
            Ok(Some(msg_buf))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Encode, Decode)]
    struct Foo {
        x: f32,
        y: u32,
        z: String,
    }
    
    #[test]
    fn test() {
        let mut lane = LaneState::new();

        let foo = Foo {
            x: 12.3,
            y: 456,
            z: "hello".into(),
        };

        let bytes = bitcode::encode(&foo).unwrap();
        let packets = lane.chunk(&bytes).unwrap();
        for packet in packets {
            if let Some(msg) = lane.recv(&packet).unwrap() {
                println!("{msg:?}");
                let msg: Foo = bitcode::decode(&msg).unwrap();
                println!("Received: {msg:?}");
            }
        }

        println!("Done");
    }
}

// // todo docs
// /// See:
// /// * https://gafferongames.com/post/reliable_ordered_messages
// /// * https://gafferongames.com/post/reliability_ordering_and_congestion_avoidance_over_udp/,
// ///   *Reliable Acks*
// #[derive(Encode, Decode)]
// pub struct PacketHeader {
//     /// The sequence number of the current packet.
//     pub seq: Seq,
//     /// The most recent sequence number received.
//     ///
//     /// `ack_bits` works relative to this number.
//     pub ack: Seq,
//     /// A bit set of acked packets.
//     ///
//     /// If bit `n` is set starting from `n = 0`, then `ack - (n + 1)` has
// been     /// acked. For example:
//     /// * if the last acked packet was number 300 (`ack: 300`)
//     /// * and the five packets before that were also acked (294 to 299)
//     /// * but no packets before 294 were acked
//     ///
//     /// this would hold the value `0b00[..]0011111`.
//     pub ack_bits: u32,
// }

// #[derive(Debug, Clone, Copy)]
// pub struct PacketData {
//     pub sent_at: Instant,
//     pub state: PacketState,
// }

// #[derive(Debug, Clone, Copy)]
// pub enum PacketState {
//     Nack,
//     Ack,
// }

// #[derive(Debug, Clone)]
// pub struct LaneWriter<const CAP: usize> {
//     seq: Seq,
//     seq_buf: [Seq; CAP],
//     packets: [Option<PacketData>; CAP],
// }

// impl<const CAP: usize> LaneWriter<CAP> {
//     pub fn new() -> Self {
//         assert!(CAP > 0);

//         Self {
//             seq: Seq(0),
//             seq_buf: [Seq(0); CAP],
//             packets: [None; CAP],
//         }
//     }

//     pub fn get(&self, seq: Seq) -> Option<PacketData> {
//         let index = usize::from(seq.0) % CAP;
//         if self.seq_buf[index] == seq {
//             self.packets[index]
//         } else {
//             None
//         }
//     }

//     pub fn insert(&mut self, seq: Seq, data: PacketData) {
//         let index = usize::from(seq.0) % CAP;
//         self.seq_buf[index] = seq;
//         self.packets[index] = Some(data);
//     }
// }
