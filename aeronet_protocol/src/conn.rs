use bytes::{Buf, Bytes, BytesMut};
use octets::Octets;

use crate::{ack::AckHeader, frag::Fragment, seq::Seq};

#[derive(Debug)]
pub struct Connection {}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ConnectionError {
    #[error("no ack header")]
    NoAckHeader,
}

impl Connection {
    pub fn recv(&mut self, packet: Bytes) -> Result<(), ConnectionError> {
        if packet.remaining() < AckHeader::ENCODE_SIZE {
            return Err(ConnectionError::NoAckHeader);
        }
        let ack_header = AckHeader::decode(&mut packet);

        while packet.remaining() > 0 {
            let frag = Fragment::decode(buf)
        }
    }
}
