use std::{marker::PhantomData, time::Duration};

use aeronet::LaneConfig;
use bytes::Bytes;
use derivative::Derivative;
use octets::{Octets, OctetsMut};

use crate::{FragmentData, FragmentHeader, Fragmentation, Seq};

use super::{
    ord::{Sequencing, SequencingKind},
    LaneRecv, LaneRecvError, LaneSendError, LaneState, LaneUpdateError, Sequenced, Unsequenced,
    VARINT_MAX_SIZE,
};

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Unreliable<S> {
    /// See [`LaneConfig::drop_after`].
    pub drop_after: Duration,
    frag: Fragmentation,
    // incrementing counter for the seq of the next buffered message to send
    next_send_seq: Seq,
    // seq number of the last message identified (not fully received) - at least
    // 1 frag has been received for this message
    last_recv_seq: Seq,
    send_buf: Vec<Bytes>,
    #[derivative(Debug(bound = ""))]
    _phantom: PhantomData<S>,
}

const LEN_ENCODE_MAX_SIZE: usize = VARINT_MAX_SIZE;

// allows encoding at least one fragment in a packet
const MIN_PACKET_LEN: usize = LEN_ENCODE_MAX_SIZE + Seq::ENCODE_SIZE + FragmentHeader::ENCODE_SIZE;

impl<S: Sequencing> Unreliable<S> {
    #[must_use]
    pub fn new(max_packet_len: usize, config: &LaneConfig) -> Self {
        assert!(max_packet_len > MIN_PACKET_LEN);
        let packet_len = max_packet_len - MIN_PACKET_LEN;
        Self {
            frag: Fragmentation::new(packet_len),
            next_send_seq: Seq(0),
            last_recv_seq: Seq(0),
            drop_after: config.drop_after,
            send_buf: Vec::new(),
            _phantom: PhantomData,
        }
    }

    fn _buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        let seq = self.next_send_seq.get_inc();
        self.send_buf.extend(
            self.frag
                .fragment(msg)
                .map_err(LaneSendError::Fragment)?
                .map(|data| {
                    let mut buf = vec![0; FragmentHeader::ENCODE_SIZE + data.payload.len()]
                        .into_boxed_slice();
                    let mut octs = OctetsMut::with_slice(&mut buf);
                    data.header.encode(&mut octs).unwrap();
                    octs.put_bytes(data.payload).unwrap();
                    Bytes::from(buf)
                }),
        );
        Ok(seq)
    }

    fn _poll(&mut self) -> Result<(), LaneUpdateError> {
        self.frag.clean_up(self.drop_after);
        self.send_buf.drain(..);
        Ok(())
    }
}

impl Unreliable<Unsequenced> {
    #[must_use]
    pub fn unsequenced(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Unreliable<Unsequenced> {
    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        self._buffer_send(msg)
    }

    fn recv<'packet>(&mut self, packet: &'packet [u8]) -> LaneRecv<'_, 'packet> {
        LaneRecv::UnreliableUnsequenced(Recv {
            lane: self,
            packet,
            off: 0,
        })
    }

    fn poll(&mut self) -> Result<(), LaneUpdateError> {
        self._poll()
    }
}

impl Unreliable<Sequenced> {
    #[must_use]
    pub fn sequenced(max_packet_len: usize, config: &LaneConfig) -> Self {
        Self::new(max_packet_len, config)
    }
}

impl LaneState for Unreliable<Sequenced> {
    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        self._buffer_send(msg)
    }

    fn recv<'packet>(&mut self, packet: &'packet [u8]) -> LaneRecv<'_, 'packet> {
        LaneRecv::UnreliableSequenced(Recv {
            lane: self,
            packet,
            off: 0,
        })
    }

    fn poll(&mut self) -> Result<(), LaneUpdateError> {
        self._poll()
    }
}

pub struct Recv<'l, 'p, S> {
    lane: &'l mut Unreliable<S>,
    packet: &'p [u8],
    off: usize,
}

impl<S: Sequencing> Iterator for Recv<'_, '_, S> {
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

impl<S: Sequencing> Recv<'_, '_, S> {
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
