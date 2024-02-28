use std::{
    marker::PhantomData,
    time::{Duration, Instant},
};

use aeronet::LaneConfig;
use bytes::Bytes;
use derivative::Derivative;
use octets::Octets;

use crate::{AcknowledgeHeader, FragmentHeader, Fragmentation, Seq};

use super::{
    ord::{Ordered, Ordering, Unordered},
    LaneRecv, LaneRecvError, LaneSendError, LaneState, LaneUpdateError, Sequenced, VARINT_MAX_SIZE,
};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Reliable<O> {
    /// See [`LaneConfig::resend_after`].
    pub resend_after: Duration,
    /// See [`LaneConfig::ack_timeout`].
    pub ack_timeout: Duration,
    next_send_seq: Seq,
    send_buf: Vec<BufferedMessage>,
    frag: Fragmentation,
    #[derivative(Debug(bound = ""))]
    _phantom: PhantomData<O>,
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
    last_alive_at: Instant,
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
            send_buf: Vec::new(),
            frag: Fragmentation::new(payload_len),
            _phantom: PhantomData,
        }
    }

    fn _buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        let seq = self.next_send_seq.get_inc();
        let now = Instant::now();
        let frags = self
            .frag
            .fragment(msg)
            .map_err(LaneSendError::Fragment)?
            .map(|data| BufferedFragment {
                frag_header: data.header,
                payload: Box::from(data.payload),
                last_alive_at: now,
                last_sent_at: None,
            })
            .collect();
        self.send_buf.push(BufferedMessage { seq, frags });
        Ok(seq)
    }

    fn _poll(&mut self) -> Result<(), LaneUpdateError> {
        if self.frag.clean_up(self.ack_timeout) > 0 {
            // at least one of our buffered receiving messages reached the ack
            // timeout, we can't guarantee reliability anymore
            return Err(LaneUpdateError::RecvTimeout);
        }
        Ok(())
    }
}

impl Reliable<Unordered> {
    pub fn unordered(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Reliable<Unordered> {
    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        self._buffer_send(msg)
    }

    fn recv<'p>(&mut self, packet: &'p [u8]) -> LaneRecv<'_, 'p> {
        LaneRecv::ReliableUnordered(Recv {
            lane: self,
            packet,
            off: 0,
        })
    }

    fn poll(&mut self) -> Result<(), LaneUpdateError> {
        self._poll()
    }
}

impl Reliable<Sequenced> {
    pub fn sequenced(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Reliable<Sequenced> {
    fn poll(&mut self) -> Result<(), LaneUpdateError> {
        self._poll()
    }

    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        self._buffer_send(msg)
    }

    fn recv<'p>(&mut self, packet: &'p [u8]) -> LaneRecv<'_, 'p> {
        LaneRecv::ReliableSequenced(Recv {
            lane: self,
            packet,
            off: 0,
        })
    }
}

impl Reliable<Ordered> {
    pub fn ordered(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Reliable<Ordered> {
    fn poll(&mut self) -> Result<(), LaneUpdateError> {
        self._poll()
    }

    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        self._buffer_send(msg)
    }

    fn recv<'p>(&mut self, packet: &'p [u8]) -> LaneRecv<'_, 'p> {
        LaneRecv::ReliableOrdered(Recv {
            lane: self,
            packet,
            off: 0,
        })
    }
}

pub struct Recv<'l, 'p, O> {
    lane: &'l mut Reliable<O>,
    packet: &'p [u8],
    off: usize,
}

impl<O: Ordering> Iterator for Recv<'_, '_, O> {
    type Item = Result<Bytes, LaneRecvError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut packet = Octets::with_slice(self.packet);
        packet.skip(self.off).unwrap();

        while let Ok(len) = packet.get_varint() {
            self.off = packet.off();
            let len = len as usize;
            let frag = match packet.slice(len).map_err(|_| LaneRecvError::TooLong {
                len,
                cap: packet.cap(),
            }) {
                Ok(frag) => frag,
                Err(err) => return Some(Err(err)),
            };

            let mut frag = Octets::with_slice(frag);
            let result = self.decode(&mut frag);
            // make sure we've consumed the entire fragment
            let _ = frag.skip(frag.cap());
            self.off = packet.off();
            match result {
                Ok(Some(msg)) => return Some(Ok(msg)),
                Ok(None) => continue,
                Err(err) => return Some(Err(err)),
            }
        }
        None
    }
}

impl<O: Ordering> Recv<'_, '_, O> {
    fn decode(&mut self, frag: &mut Octets<'_>) -> Result<Option<Bytes>, LaneRecvError> {
        let seq = Seq::decode(frag).map_err(|_| LaneRecvError::NoSeq)?;
        match S::KIND {
            SequencingKind::Sequenced => {
                if seq < self.lane.last_recv_seq {
                    return Ok(None);
                }
            }
            SequencingKind::Unsequenced => {}
        }
        self.lane.last_recv_seq = seq;

        let header = FragmentHeader::decode(frag)
            .map_err(|_| LaneRecvError::NoHeader)?
            .ok_or(LaneRecvError::InvalidHeader)?;
        let payload = frag.as_ref();
        let msg = self
            .lane
            .frag
            .reassemble(seq, &FragmentData { header, payload })
            .map_err(LaneRecvError::Reassemble)?;
        Ok(msg.map(Bytes::from))
    }
}
