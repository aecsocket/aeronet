use std::{marker::PhantomData, time::Duration};

use bytes::Bytes;
use derivative::Derivative;
use octets::{Octets, OctetsMut};

use crate::{FragmentData, FragmentHeader, Fragmentation, Seq};

use super::{
    ord::{Sequencing, SequencingKind},
    LaneRecvError, LaneSendError, LaneState, LaneUpdateError, Sequenced, Unsequenced,
    VARINT_MAX_SIZE,
};

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub struct Unreliable<S> {
    frag: Fragmentation,
    // incrementing counter for the seq of the next buffered message to send
    next_send_seq: Seq,
    // seq number of the last message identified (not fully received) - at least
    // 1 frag has been received for this message
    last_recv_seq: Seq,
    drop_after: Duration,
    send_buf: Vec<Box<[u8]>>,
    #[derivative(Debug(bound = ""))]
    _phantom: PhantomData<S>,
}

const LEN_ENCODE_MAX_SIZE: usize = VARINT_MAX_SIZE;

// allows encoding at least one fragment in a packet
const MIN_PACKET_LEN: usize = LEN_ENCODE_MAX_SIZE + Seq::ENCODE_SIZE + FragmentHeader::ENCODE_SIZE;

impl<S: Sequencing> Unreliable<S> {
    pub fn new(max_packet_len: usize, drop_after: Duration) -> Self {
        assert!(max_packet_len > MIN_PACKET_LEN);
        let packet_len = max_packet_len - MIN_PACKET_LEN;
        Self {
            frag: Fragmentation::new(packet_len),
            next_send_seq: Seq(0),
            last_recv_seq: Seq(0),
            drop_after,
            send_buf: Vec::new(),
            _phantom: PhantomData,
        }
    }
}

impl Unreliable<Unsequenced> {
    pub fn unsequenced(max_packet_len: usize, drop_after: Duration) -> Self {
        Self::new(max_packet_len, drop_after)
    }
}

impl Unreliable<Sequenced> {
    pub fn sequenced(max_packet_len: usize, drop_after: Duration) -> Self {
        Self::new(max_packet_len, drop_after)
    }
}

impl<S: Sequencing> LaneState for Unreliable<S> {
    fn update(&mut self) -> Result<(), LaneUpdateError> {
        self.frag.clean_up(self.drop_after);
        Ok(())
    }

    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
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
                    buf
                }),
        );
        Ok(seq)
    }

    fn recv(&mut self, packet: &[u8]) -> (Vec<Bytes>, Result<(), LaneRecvError>) {
        let mut msgs = Vec::new();
        let mut packet = Octets::with_slice(packet);

        while let Ok(len) = packet.get_varint() {
            let len = len as usize;
            match self.start_decode_frag(len, &mut packet) {
                Ok(Some(msg)) => msgs.push(msg),
                Ok(None) => {}
                Err(err) => return (msgs, Err(err)),
            }
        }
        (msgs, Ok(()))
    }
}

impl<S: Sequencing> Unreliable<S> {
    fn start_decode_frag(
        &mut self,
        len: usize,
        packet: &mut Octets<'_>,
    ) -> Result<Option<Bytes>, LaneRecvError> {
        let frag = packet.slice(len).map_err(|_| LaneRecvError::TooLong {
            len,
            cap: packet.cap(),
        })?;
        let mut frag = Octets::with_slice(frag);
        let res = self.decode_frag(&mut frag);
        // make sure we've consumed the entire fragment
        let _ = frag.skip(frag.cap());
        res
    }

    fn decode_frag(&mut self, frag: &mut Octets<'_>) -> Result<Option<Bytes>, LaneRecvError> {
        let seq = Seq::decode(frag).map_err(|_| LaneRecvError::NoSeq)?;
        match S::KIND {
            SequencingKind::Sequenced => {
                if seq < self.last_recv_seq {
                    return Ok(None);
                }
            }
            SequencingKind::Unsequenced => {}
        }
        self.last_recv_seq = seq;

        let header = FragmentHeader::decode(frag)
            .map_err(|_| LaneRecvError::NoHeader)?
            .ok_or(LaneRecvError::InvalidHeader)?;
        let payload = frag.as_ref();
        let msg = self
            .frag
            .reassemble(seq, &FragmentData { header, payload })
            .map_err(LaneRecvError::Reassemble)?;
        Ok(msg.map(Bytes::from))
    }
}
