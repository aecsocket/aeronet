use std::{array, mem};

use bitcode::{Decode, Encode};

use super::Seq;

#[doc(alias = "mtu")]
pub const MAX_PACKET_SIZE: usize = 1024;

#[derive(Debug, Clone, Encode, Decode)]
struct PacketHeader {
    seq: Seq,
    frag_id: u8,
    num_frags: u8,
}

// size of `bitcode::encode` on a value of this type must always be equal to
// this value
pub const HEADER_SIZE: usize = mem::size_of::<PacketHeader>();

pub const PAYLOAD_SIZE: usize = MAX_PACKET_SIZE - HEADER_SIZE;

pub const PACKETS_BUF: usize = 256;

pub const MAX_MESSAGE_SIZE: usize = PAYLOAD_SIZE * u8::MAX as usize;

#[derive(Debug, thiserror::Error)]
pub enum FragmentationError {
    #[error("message too big; {len} / {MAX_MESSAGE_SIZE} bytes")]
    MessageTooBig { len: usize },
    #[error("packet too small; {len} / {HEADER_SIZE} bytes")]
    PacketTooSmall { len: usize },
    #[error("packet too big; {len} / {MAX_PACKET_SIZE} bytes")]
    PacketTooBig { len: usize },
    #[error("failed to decode packet header")]
    DecodeHeader(#[source] bitcode::Error),
    #[error("invalid packet header")]
    InvalidHeader,
}

#[derive(Debug)]
pub struct Fragmentation {
    seq: Seq,
    packets: Box<[PacketBuffer; PACKETS_BUF as usize]>,
}

impl Default for Fragmentation {
    fn default() -> Self {
        Self {
            seq: Seq::default(),
            packets: Box::new(array::from_fn(|_| PacketBuffer::default())),
        }
    }
}

#[derive(Debug)]
struct PacketBuffer {
    seq: Seq,
    num_frags: u8,
    recv_frags: u8,
    frags: Box<[Vec<u8>; u8::MAX as usize + 1]>,
}

impl Default for PacketBuffer {
    fn default() -> Self {
        Self {
            seq: Seq::default(),
            num_frags: u8::default(),
            recv_frags: u8::default(),
            frags: Box::new(array::from_fn(|_| Vec::default())),
        }
    }
}

impl Fragmentation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fragment<'a>(
        &'a mut self,
        bytes: &'a [u8],
    ) -> Result<impl Iterator<Item = Vec<u8>> + 'a, FragmentationError> {
        let seq = self.seq.next();

        let chunks = bytes.chunks(PAYLOAD_SIZE);
        let num_frags = u8::try_from(chunks.len())
            .map_err(|_| FragmentationError::MessageTooBig { len: bytes.len() })?;

        Ok(chunks.enumerate().map(move |(frag_id, chunk)| {
            let frag_id = u8::try_from(frag_id)
                .expect("`num_frags` is a u8, so `frag_id` should be convertible");
            let header = PacketHeader {
                seq,
                frag_id,
                num_frags,
            };
            let mut packet = bitcode::encode(&header)
                .expect("does not use #[bitcode(with_serde)], so should never fail");
            debug_assert_eq!(HEADER_SIZE, packet.len());

            packet.reserve_exact(PAYLOAD_SIZE);
            packet.extend(chunk);
            debug_assert!(packet.len() <= MAX_PACKET_SIZE);

            packet
        }))
    }

    pub fn reassemble(&mut self, packet: &[u8]) -> Result<Option<Vec<u8>>, FragmentationError> {
        if packet.len() < HEADER_SIZE {
            return Err(FragmentationError::PacketTooSmall { len: packet.len() });
        }
        if packet.len() > MAX_PACKET_SIZE {
            return Err(FragmentationError::PacketTooBig { len: packet.len() });
        }

        let header = bitcode::decode::<PacketHeader>(&packet[..HEADER_SIZE])
            .map_err(FragmentationError::DecodeHeader)?;

        let payload = &packet[HEADER_SIZE..];
        match header.num_frags {
            0 => Err(FragmentationError::InvalidHeader),
            1 => Ok(Some(payload.to_vec())),
            _ => Ok(self.reassemble_fragment(header, payload)),
        }
    }

    fn reassemble_fragment(&mut self, header: PacketHeader, payload: &[u8]) -> Option<Vec<u8>> {
        let buf = &mut self.packets[header.seq.0 as usize % PACKETS_BUF];
        if buf.num_frags == 0 {
            // buffer is unpopulated, let's initialize it
            buf.seq = header.seq;
            buf.num_frags = header.num_frags;
        }

        // make sure that `buf` really does point to the same packet that we're
        // meant to be reassembling
        if buf.seq != header.seq {
            return None;
        }
        if buf.num_frags != header.num_frags {
            return None;
        }

        let buf_payload = &mut buf.frags[usize::from(header.frag_id)];
        if !buf_payload.is_empty() {
            // we already got the payload for this fragment; is this a duplicate
            // packet?
            return None;
        }
        // add the payload
        buf_payload.extend(payload);
        buf.recv_frags += 1;

        if buf.recv_frags >= buf.num_frags {
            // we've received all fragments for this packet, collect them and
            // return the packet
            let message = buf.frags[..usize::from(buf.num_frags)]
                .iter()
                .flatten()
                .copied()
                .collect();

            // mark this buffer as unpopulated and clear it
            buf.num_frags = 0;
            buf.recv_frags = 0;
            buf.frags.fill(Vec::new());

            Some(message)
        } else {
            // this packet isn't complete yet, nothing to return
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[test]
    fn small() {
        let mut buf: VecDeque<Vec<u8>> = VecDeque::new();
        let mut frag = Fragmentation::new();

        // send
        let msg = b"Hello world!";
        buf.extend(frag.fragment(msg).unwrap());

        // recv
        assert_eq!(
            msg.as_slice(),
            frag.reassemble(&buf.pop_front().unwrap())
                .unwrap()
                .unwrap()
                .as_slice()
        );
    }

    #[test]
    fn big() {
        let mut buf: VecDeque<Vec<u8>> = VecDeque::new();
        let mut frag = Fragmentation::new();

        // send
        let msg = format!("Hello world! {}", "abcd".repeat(700));
        buf.extend(frag.fragment(msg.as_bytes()).unwrap());

        // recv
        assert!(matches!(
            frag.reassemble(&buf.pop_front().unwrap()),
            Ok(None)
        ));
        assert!(matches!(
            frag.reassemble(&buf.pop_front().unwrap()),
            Ok(None)
        ));
        assert_eq!(
            msg.as_bytes().to_vec(),
            frag.reassemble(&buf.pop_front().unwrap()).unwrap().unwrap(),
        );
    }
}
