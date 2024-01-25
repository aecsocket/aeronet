use bitcode::{Decode, Encode};

use super::Seq;

/// Metadata for an [`Acks`] packet.
#[derive(Debug, Clone, Encode, Decode)]
pub struct AckHeader {
    pub seq: Seq,
    pub latest_ack: Seq,
    pub prev_acks: u32,
}

const MESSAGES_BUF: usize = 256;

#[derive(Debug)]
pub struct Acks {
    messages: Box<[MessageBuffer; MESSAGES_BUF]>,
}

impl Default for Acks {
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

impl Acks {}
