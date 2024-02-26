use std::time::{Duration, Instant};

use arbitrary::Arbitrary;

use crate::{FragmentError, FragmentHeader, Fragmentation, ReassembleError, Seq};

#[derive(Debug, Clone, Arbitrary)]
pub struct AcknowledgeHeader {
    pub last_ack: Seq,
    pub ack_bits: u32,
}

impl AcknowledgeHeader {
    /// Encoded size of this value in bytes.
    pub const SIZE: usize = 2 + 4;

    pub fn encode(&self, buf: &mut octets::OctetsMut<'_>) -> octets::Result<()> {
        self.last_ack.encode(buf)?;
        buf.put_u32(self.ack_bits);
        Ok(())
    }

    pub fn decode(buf: &mut octets::Octets<'_>) -> octets::Result<Self> {
        let last_ack = Seq::decode(buf)?;
        let ack_bits = buf.get_u32()?;
        Ok(Self { last_ack, ack_bits })
    }
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
    /// How long to wait until resending a fragment which was not acknowledged
    /// by the peer.
    ///
    /// The initial send is always instant.
    pub resend_after: Duration,
    /// If any message is not fully received by this duration, the lane is
    /// considered "timed out", and the connection must be terminated.
    pub ack_timeout: Duration,
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
    frag_header: FragmentHeader,
    payload: Box<[u8]>,
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
            .map(|(frag_header, payload)| BufferedFragment {
                frag_header,
                payload: Box::from(payload),
                last_sent_at: None,
            })
            .collect();
        *buf_opt = Some(BufferedMessage { seq, frags });
        Ok(())
    }

    // note: `bytes_available` only counts payload bytes
    pub fn packets_to_send<'a>(
        &'a mut self,
        bytes_available: &mut usize,
    ) -> Vec<(&'a FragmentHeader, &'a [u8])> {
        let now = Instant::now();
        // each fragment is its own individual packet
        // there is no packing done, maybe TODO?
        let mut packets = Vec::new();
        for buf in &mut self.send_buf {
            let Some(opt) = buf else { continue };
            for frag in &mut opt.frags {
                if *bytes_available < frag.payload.len() {
                    continue;
                }
                let due_for_sending = match frag.last_sent_at {
                    Some(last_sent) => now - last_sent >= self.resend_after,
                    None => true,
                };
                if !due_for_sending {
                    continue;
                }

                frag.last_sent_at = Some(now);
                packets.push((&frag.frag_header, &*frag.payload));
                *bytes_available -= frag.payload.len();
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
