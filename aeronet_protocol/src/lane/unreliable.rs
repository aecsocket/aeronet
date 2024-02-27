use bytes::Bytes;

use crate::{Fragmentation, Seq};

use super::{LaneSendError, LaneState};

#[derive(Debug)]
pub struct UnreliableUnsequenced {
    frag: Fragmentation,
    next_send_seq: Seq,
    to_send: Vec<Bytes>,
}

impl UnreliableUnsequenced {
    pub fn new(frag: Fragmentation) -> Self {
        Self {
            frag,
            next_send_seq: Seq(0),
            to_send: Vec::new(),
        }
    }

    pub fn packets_to_send(&mut self) -> impl Iterator<Item = Bytes> {}
}

impl LaneState for UnreliableUnsequenced {
    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        let seq = self.next_send_seq.get_inc();
        self.to_send.extend(
            self.frag
                .fragment(msg)
                .map_err(LaneSendError::Fragment)?
                .map(|data| {}),
        );
        Ok(seq)
    }
}
