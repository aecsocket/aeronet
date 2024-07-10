mod recv;
mod send;

pub use {recv::*, send::*};

use aeronet::lane::LaneKind;
use ahash::AHashMap;
use web_time::Duration;

use crate::{ack::Acknowledge, frag::FragmentReceiver, seq::Seq};

#[derive(Debug)]
pub struct Session {
    acks: Acknowledge,
    flushed_packets: AHashMap<Seq, ()>,
    // send
    send_lanes: Box<[SendLane]>,
    // recv
    recv_lanes: Box<[RecvLane]>,
    recv_frags: FragmentReceiver,
    max_memory_usage: usize,
    bytes_recv: usize,
}

#[derive(Debug, Clone)]
pub struct LaneConfig {
    pub kind: LaneKind,
    pub bytes_per_sec: usize,
    pub resend_after: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self {
            kind: LaneKind::UnreliableUnordered,
            bytes_per_sec: usize::MAX,
            resend_after: Duration::from_millis(100),
        }
    }
}

impl LaneConfig {
    pub fn new(kind: LaneKind) -> Self {
        Self {
            kind,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SendError {
    #[error("invalid lane index")]
    InvalidLane,
}
