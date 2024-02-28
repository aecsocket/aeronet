use std::{
    marker::PhantomData,
    time::{Duration, Instant},
};

use bytes::Bytes;
use derivative::Derivative;

use crate::{AcknowledgeHeader, FragmentHeader, Fragmentation, Seq};

use super::{
    ord::{Ordered, Ordering, Unordered},
    LaneRecvError, LaneSendError, LaneState, LaneUpdateError, Sequenced, VARINT_MAX_SIZE,
};

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Reliable<O: Ordering> {
    /// How long to wait until resending a fragment which was not acknowledged
    /// by the peer.
    ///
    /// The initial send is always instant.
    pub resend_after: Duration,
    /// If any message is not fully received by this duration, the lane is
    /// considered "timed out", and the connection must be terminated.
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
    pub fn new(max_packet_len: usize, resend_after: Duration, ack_timeout: Duration) -> Self {
        assert!(max_packet_len > MIN_PACKET_LEN);
        let payload_len = max_packet_len - MIN_PACKET_LEN;
        Self {
            resend_after,
            ack_timeout,
            next_send_seq: Seq(0),
            send_buf: Vec::new(),
            frag: Fragmentation::new(payload_len),
            _phantom: PhantomData,
        }
    }
}

impl Reliable<Unordered> {
    pub fn unordered(max_packet_len: usize, resend_after: Duration, ack_timeout: Duration) -> Self {
        Self::new(max_packet_len, resend_after, ack_timeout)
    }
}

impl Reliable<Sequenced> {
    pub fn sequenced(max_packet_len: usize, resend_after: Duration, ack_timeout: Duration) -> Self {
        Self::new(max_packet_len, resend_after, ack_timeout)
    }
}

impl Reliable<Ordered> {
    pub fn ordered(max_packet_len: usize, resend_after: Duration, ack_timeout: Duration) -> Self {
        Self::new(max_packet_len, resend_after, ack_timeout)
    }
}

impl<O: Ordering> LaneState for Reliable<O> {
    fn update(&mut self) -> Result<(), LaneUpdateError> {
        if self.frag.clean_up(self.ack_timeout) > 0 {
            // at least one of our buffered receiving messages reached the ack
            // timeout, we can't guarantee reliability anymore
            return Err(LaneUpdateError::RecvTimeout);
        }
        Ok(())
    }

    fn buffer_send(&mut self, msg: &[u8]) -> Result<Seq, LaneSendError> {
        let seq = self.next_send_seq.get_inc();
        todo!();
        Ok(seq)
    }

    fn recv(&mut self, packet: &[u8]) -> (Vec<Bytes>, Result<(), LaneRecvError>) {
        todo!()
    }
}
