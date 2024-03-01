use std::collections::BinaryHeap;

use crate::{
    ack::AckHeader,
    bytes::prelude::*,
    frag::{Fragment, FragmentError, Fragmentation, ReassembleError},
    seq::Seq,
};

#[derive(Debug)]
pub struct Connection {
    frag: Fragmentation,
    next_msg_seq: Seq,
    next_packet_seq: Seq,
    send_buf: BinaryHeap<Fragment>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConnectionError {
    #[error("failed to fragment message")]
    Fragment(#[source] FragmentError),
    #[error("failed to read ack header")]
    ReadAckHeader(#[source] ReadError),
    #[error("failed to read fragment")]
    ReadFrag(#[source] ReadError),
    #[error("failed to reassemble fragment")]
    Reassemble(#[source] ReassembleError),
}

impl Connection {
    pub fn buffer_send(&mut self, lane_index: usize, msg: Bytes) -> Result<(), ConnectionError> {
        let msg_seq = self.next_msg_seq.get_inc();
        self.send_buf.extend(
            self.frag
                .fragment(msg_seq, msg)
                .map_err(ConnectionError::Fragment)?,
        );
        Ok(())
    }

    pub fn flush(&mut self, available_bytes: usize) -> Flush<'_> {
        Flush {
            send_buf: &mut self.send_buf,
            available_bytes,
        }
    }

    pub fn recv(&mut self, mut packet: Bytes) -> Result<Recv<'_>, ConnectionError> {
        let ack_header = AckHeader::decode(&mut packet).map_err(ConnectionError::ReadAckHeader)?;
        Ok(Recv {
            frag: &mut self.frag,
            packet,
        })
    }
}

pub struct Flush<'c> {
    send_buf: &'c mut BinaryHeap<Fragment>,
    available_bytes: usize,
}

impl Iterator for Flush<'_> {
    type Item = ();

    fn next(&mut self) -> Option<Self::Item> {
        let frag = self.send_buf.peek()?;
        if self.available_bytes < frag.payload.len() {
            return None;
        }
        let frag = self.send_buf.pop().unwrap();
        self.available_bytes -= frag.payload.len();
        Some(todo!())
    }
}

impl Flush<'_> {
    pub fn available_bytes(&self) -> usize {
        self.available_bytes
    }
}

#[derive(Debug)]
pub struct Recv<'c> {
    frag: &'c mut Fragmentation,
    packet: Bytes,
}

impl Iterator for Recv<'_> {
    type Item = Result<Vec<u8>, ConnectionError>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.packet.remaining() > 0 {
            match self.try_next() {
                Ok(Some(msg)) => return Some(Ok(msg)),
                Ok(None) => continue,
                Err(err) => return Some(Err(err)),
            }
        }
        None
    }
}

impl Recv<'_> {
    fn try_next(&mut self) -> Result<Option<Vec<u8>>, ConnectionError> {
        let lane_index = self
            .packet
            .try_get_varint()
            .map_err(ConnectionError::ReadFrag)? as usize;
        let frag = Fragment::decode(&mut self.packet).map_err(ConnectionError::ReadFrag)?;
        self.frag
            .reassemble(&frag.header, &frag.payload)
            .map_err(ConnectionError::Reassemble)
    }
}
