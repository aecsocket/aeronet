use bitcode::{Decode, Encode};

use super::Seq;

/// Metadata for an [`Acknowledge`] packet.
#[derive(Debug, Clone, Encode, Decode)]
struct AckHeader {
    seq: Seq,
    latest_ack: Seq,
    prev_acks: u32,
}

const MESSAGES_BUF: usize = 256;

#[derive(Debug)]
pub struct Acknowledge {
    messages: Box<[MessageBuffer; MESSAGES_BUF]>,
}

impl Default for Acknowledge {
    fn default() -> Self {
        Self {
            messages: Box::new(std::array::from_fn(|_| MessageBuffer::default())),
        }
    }
}

#[derive(Debug, Default)]
struct MessageBuffer {
    acked: bool,
}

impl Acknowledge {}
