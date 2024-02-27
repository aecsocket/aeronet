use std::time::Duration;

use bytes::Bytes;
use octets::OctetsMut;

use crate::{FragmentHeader, Fragmentation, Seq};

use super::{LaneSendError, LaneState, LANE_INDEX_SIZE};

#[derive(Debug)]
pub struct UnreliableUnsequenced {
    frag: Fragmentation,
    next_send_seq: Seq,
    drop_after: Duration,
    send_buf: Vec<Box<[u8]>>,
}

impl UnreliableUnsequenced {
    pub fn new(max_packet_len: usize, drop_after: Duration) -> Self {
        const MIN_PACKET_LEN: usize =
            LANE_INDEX_SIZE + Seq::ENCODE_SIZE + FragmentHeader::ENCODE_SIZE;
        assert!(max_packet_len > MIN_PACKET_LEN);
        let packet_len = max_packet_len - MIN_PACKET_LEN;
        Self {
            frag: Fragmentation::new(packet_len),
            next_send_seq: Seq(0),
            drop_after,
            send_buf: Vec::new(),
        }
    }
}

impl LaneState for UnreliableUnsequenced {
    // allocates here
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

    fn update(&mut self) {
        self.frag.clean_up(self.drop_after);
    }
}
