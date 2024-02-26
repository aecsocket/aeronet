use std::time::{Duration, Instant};

use arbitrary::Arbitrary;
use bitcode::{Decode, Encode};
use bytes::Bytes;
use static_assertions::const_assert_eq;

use crate::{FragmentError, FragmentHeader, Fragmentation, ReassembleError, Seq};

#[derive(Debug, Clone, Encode, Decode, Arbitrary)]
pub struct AcknowledgeHeader {
    pub last_ack: Seq,
    pub ack_bits: u32,
}

const_assert_eq!(AcknowledgeHeader::ENCODE_MIN, AcknowledgeHeader::ENCODE_MAX);

impl AcknowledgeHeader {
    /// Encoded size of a header in bytes.
    pub const SIZE: usize = Self::ENCODE_MIN / 8;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ReliableSendError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
    #[error("send buffer is full")]
    BufferFull,
}

#[derive(Debug)]
pub struct Reliable {
    pub resend_after: Duration,
    send_buf: Vec<Option<BufferedMessage>>,
    frag: Fragmentation,
}

#[derive(Debug)]
struct BufferedMessage {
    seq: Seq,
    frags: Vec<BufferedFragment>,
}

#[derive(Debug)]
struct BufferedFragment {
    bytes: Bytes,
    last_sent_at: Option<Instant>,
}

impl Reliable {
    pub fn buffer_send(&mut self, seq: Seq, msg: &[u8]) -> Result<(), ReliableSendError> {
        let buf_index = usize::from(seq.0) % self.send_buf.len();
        let buf_opt = &mut self.send_buf[buf_index];
        if buf_opt.is_some() {
            // either there are too few slots for buffered messages,
            // or we are sending too many messages,
            // or the receiver is not acking any of our messages
            return Err(ReliableSendError::BufferFull);
        }

        let frags = self
            .frag
            .fragment(msg)
            .map_err(ReliableSendError::Fragment)?
            .map(|(frag_header, payload)| {
                let mut bytes = vec![0; FragmentHeader::SIZE + payload.len()].into_boxed_slice();
                let frag_header = bitcode::encode(&frag_header)
                    .expect("does not use #[bitcode(with_serde)], so should not fail");

                bytes[..FragmentHeader::SIZE].copy_from_slice(&frag_header);
                bytes[FragmentHeader::SIZE..].copy_from_slice(payload);
                BufferedFragment {
                    bytes: Bytes::from(bytes.into_vec()),
                    last_sent_at: None,
                }
            })
            .collect();
        *buf_opt = Some(BufferedMessage { seq, frags });
        Ok(())
    }

    pub fn collect_to_send(&mut self, now: Instant, bytes_available: &mut usize) -> Vec<Bytes> {
        // each fragment is its own individual packet
        // there is no packing done, maybe TODO?
        let mut packets = Vec::new();
        for buf_opt in &mut self.send_buf {
            let Some(opt) = buf_opt else { continue };
            for frag in &mut opt.frags {
                if *bytes_available < frag.bytes.len() {
                    break;
                }

                let due_for_sending = match frag.last_sent_at {
                    Some(last_sent) => now - last_sent > self.resend_after,
                    None => true,
                };
                if !due_for_sending {
                    continue;
                }

                frag.last_sent_at = Some(now);
                packets.push(frag.bytes.clone()); // cheap Bytes clone
                *bytes_available -= frag.bytes.len();
            }
        }
        packets
    }

    pub fn recv(
        &mut self,
        seq: Seq,
        ack_header: &AcknowledgeHeader,
        frag_header: &FragmentHeader,
        payload: &[u8],
    ) -> Result<Option<Vec<u8>>, ReassembleError> {
        if let Some(msg) = self.frag.reassemble(seq, frag_header, payload)? {
            todo!()
        }
        todo!()
    }
}
