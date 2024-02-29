use std::{marker::PhantomData, time::Duration};

use aeronet::LaneConfig;
use bytes::Bytes;
use derivative::Derivative;
use octets::Octets;

use crate::{FragmentBytesData, FragmentHeader, Fragmentation, Seq};

use super::{
    ord::{Sequencing, SequencingKind},
    LaneError, LaneRecv, LaneSend, LaneState, Sequenced, Unordered, VARINT_MAX_SIZE,
};

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Unreliable<O> {
    /// See [`LaneConfig::drop_after`].
    pub drop_after: Duration,
    frag: Fragmentation,
    max_packet_len: usize,
    // incrementing counter for the seq of the next buffered message to send
    next_send_seq: Seq,
    // seq number of the last message identified (not fully received) - at least
    // 1 frag has been received for this message
    last_recv_seq: Seq,
    send_buf: Vec<FragmentBytesData>,
    #[derivative(Debug(bound = ""))]
    _phantom: PhantomData<O>,
}

#[derive(Debug)]
pub struct BufferedFragment {
    frag_header: FragmentHeader,
    payload: Bytes,
}

const LEN_ENCODE_MAX_SIZE: usize = VARINT_MAX_SIZE;

// allows encoding at least one fragment in a packet
const MIN_PACKET_LEN: usize = LEN_ENCODE_MAX_SIZE + Seq::ENCODE_SIZE + FragmentHeader::ENCODE_SIZE;

impl<O: Sequencing> Unreliable<O> {
    #[must_use]
    pub fn new(max_packet_len: usize, config: &LaneConfig) -> Self {
        assert!(max_packet_len > MIN_PACKET_LEN);
        Self {
            drop_after: config.drop_after,
            frag: Fragmentation::new(max_packet_len - MIN_PACKET_LEN),
            max_packet_len,
            next_send_seq: Seq(0),
            last_recv_seq: Seq(0),
            send_buf: Vec::new(),
            _phantom: PhantomData,
        }
    }

    fn do_buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError> {
        let seq = self.next_send_seq.get_inc();
        self.send_buf
            .extend(self.frag.fragment_bytes(msg).map_err(LaneError::Fragment)?);
        Ok(seq)
    }

    fn update(&mut self) {
        self.frag.clean_up(self.drop_after);
    }
}

impl Unreliable<Unordered> {
    #[must_use]
    pub fn unordered(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Unreliable<Unordered> {
    fn buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError> {
        self.do_buffer_send(msg)
    }

    fn recv(&mut self, packet: Bytes) -> Result<LaneRecv<'_>, LaneError> {
        Ok(LaneRecv::UnreliableUnordered(Recv {
            lane: self,
            packet,
            off: 0,
        }))
    }

    fn send_buffered(&mut self) -> Result<LaneSend<'_>, LaneError> {
        self.update();
        todo!()
    }
}

impl Unreliable<Sequenced> {
    #[must_use]
    pub fn sequenced(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Unreliable<Sequenced> {
    fn buffer_send(&mut self, msg: Bytes) -> Result<Seq, LaneError> {
        self.do_buffer_send(msg)
    }

    fn recv(&mut self, packet: Bytes) -> Result<LaneRecv<'_>, LaneError> {
        Ok(LaneRecv::UnreliableSequenced(Recv {
            lane: self,
            packet,
            off: 0,
        }))
    }

    fn send_buffered(&mut self) -> Result<LaneSend<'_>, LaneError> {
        self.update();
        todo!()
    }
}

#[derive(Debug)]
pub struct Recv<'l, O> {
    lane: &'l mut Unreliable<O>,
    packet: Bytes,
    off: usize,
}

impl<O: Sequencing> Iterator for Recv<'_, O> {
    type Item = Result<Bytes, LaneError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut packet = Octets::with_slice(&self.packet);
        packet.skip(self.off).unwrap();
        let result = self.do_next(&mut packet);
        self.off = packet.off();
        result
    }
}

impl<O: Sequencing> Recv<'_, O> {
    fn do_next(&mut self, packet: &mut Octets<'_>) -> Option<Result<Bytes, LaneError>> {
        while let Ok(len) = packet.get_varint() {
            let len = len as usize;
            let frag = match packet.slice(len).map_err(|_| LaneError::TooLong {
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
            match result {
                Ok(Some(msg)) => return Some(Ok(msg)),
                Ok(None) => continue,
                Err(err) => return Some(Err(err)),
            }
        }
        None
    }

    fn decode(&mut self, frag: &mut Octets<'_>) -> Result<Option<Bytes>, LaneError> {
        let seq = Seq::decode(frag).map_err(|_| LaneError::NoSeq)?;
        match O::KIND {
            SequencingKind::Sequenced => {
                if seq < self.lane.last_recv_seq {
                    return Ok(None);
                }
            }
            SequencingKind::Unordered => {}
        }
        self.lane.last_recv_seq = seq;

        let header = FragmentHeader::decode(frag)
            .map_err(|_| LaneError::NoFragHeader)?
            .ok_or(LaneError::InvalidFragHeader)?;
        let payload = frag.as_ref();
        let msg = self
            .lane
            .frag
            .reassemble(seq, &header, payload)
            .map_err(LaneError::Reassemble)?;
        Ok(msg.map(Bytes::from))
    }
}

#[derive(Debug)]
pub struct Send<'l> {
    max_packet_len: usize,
    send_buf: std::iter::Peekable<std::vec::Drain<'l, BufferedFragment>>,
}

impl Iterator for Send<'_> {
    type Item = Box<[u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut available_bytes = self.max_packet_len;
        while available_bytes > 0 {
            let next_frag = self.send_buf.peek();
        }
        todo!()
    }
}
