//! Implementation of a reliable packet sender and receiver, ensuring that
//! packets are delivered to the peer, resending if necessary.
//!
//! # Reliability
//!
//!

use std::{
    collections::BTreeMap,
    marker::PhantomData,
    time::{Duration, Instant},
};

use aeronet::LaneConfig;
use arbitrary::Arbitrary;
use bytes::Bytes;
use derivative::Derivative;
use octets::Octets;

use crate::{FragmentHeader, Fragmentation, Seq};

use super::{
    ord::{Ordered, Ordering, OrderingKind, Unordered},
    LaneError, LaneFlush, LanePacket, LaneRecv, LaneState, Sequenced, VARINT_MAX_SIZE,
};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Reliable<S> {
    /// See [`LaneConfig::resend_after`].
    pub resend_after: Duration,
    /// See [`LaneConfig::ack_timeout`].
    pub ack_timeout: Duration,
    next_send_seq: Seq,
    last_recv_seq: Seq,
    send_buf: BTreeMap<Seq, BufferedMessage>,
    frag: Fragmentation,
    #[derivative(Debug(bound = ""))]
    _phantom: PhantomData<S>,
}

#[derive(Debug, Clone, PartialEq, Eq, Arbitrary)]
pub struct AcknowledgeHeader {
    pub last_ack: Seq,
    pub ack_bits: u32,
}

impl AcknowledgeHeader {
    /// [Encoded](AcknowledgeHeader::encode) size of this value in bytes.
    pub const ENCODE_SIZE: usize = Seq::ENCODE_SIZE + 4;

    pub fn encode(&self, buf: &mut octets::OctetsMut<'_>) -> octets::Result<()> {
        self.last_ack.encode(buf)?;
        buf.put_u32(self.ack_bits)?;
        Ok(())
    }

    pub fn decode(buf: &mut octets::Octets<'_>) -> octets::Result<Self> {
        let last_ack = Seq::decode(buf)?;
        let ack_bits = buf.get_u32()?;
        Ok(Self { last_ack, ack_bits })
    }
}

#[derive(Debug)]
struct BufferedMessage {
    frags: Vec<BufferedFragment>,
    // either when the message was created, or the last time that one of its
    // fragments got acked
    last_ack_at: Instant,
}

#[derive(Debug)]
struct BufferedFragment {
    frag_header: FragmentHeader,
    payload: Box<[u8]>,
    last_sent_at: Option<Instant>,
}

const LEN_ENCODE_MAX_SIZE: usize = VARINT_MAX_SIZE;

// allows encoding at least one fragment in a packet
const MIN_PACKET_LEN: usize = LEN_ENCODE_MAX_SIZE
    + AcknowledgeHeader::ENCODE_SIZE
    + Seq::ENCODE_SIZE
    + FragmentHeader::ENCODE_SIZE;

impl<O: Ordering> Reliable<O> {
    pub fn new(max_packet_len: usize, config: &LaneConfig) -> Self {
        assert!(max_packet_len > MIN_PACKET_LEN);
        let payload_len = max_packet_len - MIN_PACKET_LEN;
        Self {
            resend_after: config.resend_after,
            ack_timeout: config.ack_timeout,
            next_send_seq: Seq(0),
            last_recv_seq: Seq(0),
            send_buf: BTreeMap::new(),
            frag: Fragmentation::new(payload_len),
            _phantom: PhantomData,
        }
    }

    fn do_buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError> {
        let seq = self.next_send_seq.get_inc();
        let now = Instant::now();
        todo!();
        /*
        let frags = self
            .frag
            .fragment(msg)
            .map_err(LaneError::Fragment)?
            .map(|data| BufferedFragment {
                frag_header: data.header,
                payload: Box::from(data.payload),
                last_sent_at: None,
            })
            .collect();
        self.send_buf.insert(
            seq,
            BufferedMessage {
                frags,
                last_ack_at: now,
            },
        );
        Ok(seq)*/
    }

    fn do_poll(&mut self) -> Result<(), LaneError> {
        if self.frag.clean_up(self.ack_timeout) > 0 {
            // at least one of our buffered receiving messages reached the ack
            // timeout
            // we can't guarantee reliability anymore
            return Err(LaneError::RecvTimeout);
        }

        let now = Instant::now();
        let send_len_before = self.send_buf.len();
        self.send_buf
            .retain(|_, msg| now - msg.last_ack_at < self.ack_timeout);
        if self.send_buf.len() < send_len_before {
            // at least one of our buffered send messages took too long to get
            // any fragment acked
            // we can't guarantee reliability anymore
            return Err(LaneError::SendTimeout);
        }

        Ok(())
    }

    fn decode_ack(&mut self, packet: &mut Octets<'_>) -> Result<(), LaneError> {
        let ack_header = AcknowledgeHeader::decode(packet).map_err(|_| LaneError::NoAckHeader)?;
        // TODO
        Ok(())
    }
}

impl Reliable<Unordered> {
    pub fn unordered(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Reliable<Unordered> {
    fn buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError> {
        self.do_buffer_send(msg)
    }

    fn recv(&mut self, mut packet: Bytes) -> Result<LaneRecv<'_>, LaneError> {
        //self.decode_ack(&mut packet)?;
        Ok(LaneRecv::ReliableUnordered(Recv { lane: self, packet }))
    }

    fn poll(&mut self) -> Result<(), LaneError> {
        self.do_poll()
    }

    fn flush(&mut self) -> LaneFlush<'_> {
        LaneFlush::Reliable(Flush {})
    }
}

impl Reliable<Sequenced> {
    pub fn sequenced(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Reliable<Sequenced> {
    fn buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError> {
        self.do_buffer_send(msg)
    }

    fn recv(&mut self, mut packet: Bytes) -> Result<LaneRecv<'_>, LaneError> {
        //self.decode_ack(&mut packet)?;
        Ok(LaneRecv::ReliableSequenced(Recv { lane: self, packet }))
    }

    fn poll(&mut self) -> Result<(), LaneError> {
        self.do_poll()
    }

    fn flush(&mut self) -> LaneFlush<'_> {
        Ok(LaneFlush::Reliable(Flush { lane: self }))
    }
}

impl Reliable<Ordered> {
    pub fn ordered(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Reliable<Ordered> {
    fn buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError> {
        self.do_buffer_send(msg)
    }

    fn recv(&mut self, mut packet: Bytes) -> Result<LaneRecv<'_>, LaneError> {
        //self.decode_ack(&mut packet)?;
        Ok(LaneRecv::ReliableOrdered(Recv { lane: self, packet }))
    }

    fn poll(&mut self) -> Result<(), LaneError> {
        self.do_poll()
    }

    fn flush(&mut self) -> LaneFlush<'_> {
        self.do_poll()?;
        Ok(LaneFlush::ReliableOrdered(Flush { lane: self }))
    }
}

#[derive(Debug)]
pub struct Recv<'l, O> {
    lane: &'l mut Reliable<O>,
    packet: Bytes,
}

impl<O: Ordering> Iterator for Recv<'_, O> {
    type Item = Result<Bytes, LaneError>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!();
        /*
        while let Ok(len) = self.packet.get_varint() {
            let len = len as usize;
            let frag = match self.packet.slice(len).map_err(|_| LaneError::TooLong {
                len,
                cap: self.packet.cap(),
            }) {
                Ok(frag) => frag,
                Err(err) => return Some(Err(err)),
            };

            let mut frag = Octets::with_slice(frag);
            let result = self.decode(&mut frag);
            // make sure we've consumed the entire fragment
            let _ = frag.skip(frag.cap());
            match result {
                Ok(Some(msg)) => return Some(Ok(msg)),
                Ok(None) => continue,
                Err(err) => return Some(Err(err)),
            }
        }
        None*/
    }
}

impl<O: Ordering> Recv<'_, O> {
    fn decode(&mut self, frag: &mut Octets<'_>) -> Result<Option<Bytes>, LaneError> {
        let seq = Seq::decode(frag).map_err(|_| LaneError::NoSeq)?;
        match O::KIND {
            OrderingKind::Unordered => {}
            OrderingKind::Sequenced => {
                if seq < self.lane.last_recv_seq {
                    return Ok(None);
                }
            }
            OrderingKind::Ordered => todo!(),
        }
        self.lane.last_recv_seq = seq;

        let header = FragmentHeader::decode(frag)
            .map_err(|_| LaneError::NoFragHeader)?
            .ok_or(LaneError::InvalidFragHeader)?;
        let payload = frag.as_ref();
        todo!()
    }
}

#[derive(Debug)]
pub struct Flush<'l> {}

impl Iterator for Flush<'_> {
    type Item = LanePacket;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_header() {
        let header = AcknowledgeHeader {
            last_ack: Seq(12),
            ack_bits: 34,
        };
        let mut buf = [0; AcknowledgeHeader::ENCODE_SIZE];

        let mut oct = octets::OctetsMut::with_slice(&mut buf);
        header.encode(&mut oct).unwrap();
        oct.peek_bytes(1).unwrap_err();

        let mut oct = octets::Octets::with_slice(&buf);
        assert_eq!(header, AcknowledgeHeader::decode(&mut oct).unwrap());
    }
}
