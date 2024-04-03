mod error;
mod stats;

use aeronet_proto::seq::Seq;
pub use {error::*, stats::*};

pub const DEFAULT_MTU: usize = 1200;

pub const DEFAULT_BANDWIDTH: usize = usize::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageKey {
    msg_seq: Seq,
}

impl MessageKey {
    pub fn from_raw(msg_seq: Seq) -> Self {
        Self { msg_seq }
    }

    pub fn into_raw(self) -> Seq {
        self.msg_seq
    }
}
