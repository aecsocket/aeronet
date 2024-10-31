use ahash::{HashMap, HashSet};
use typesize::derive::TypeSize;

use crate::{frag::FragmentReceiver, lane::LaneIndex, packet::MessageSeq};

#[derive(Debug, TypeSize)]
pub struct TransportRecv {
    pub(crate) msgs: Vec<(LaneIndex, Vec<u8>)>,
}

impl TransportRecv {
    pub(crate) const fn new() -> Self {
        Self { msgs: Vec::new() }
    }

    pub fn drain(&mut self) -> impl Iterator<Item = (LaneIndex, Vec<u8>)> + '_ {
        self.msgs.drain(..)
    }
}

#[derive(Debug, Clone, TypeSize)]
pub(crate) struct Lane {
    frags: FragmentReceiver,
}

#[derive(Debug, Clone, TypeSize)]
enum RecvLaneKind {
    UnreliableUnordered,
    UnreliableSequenced {
        pending: MessageSeq,
    },
    ReliableUnordered {
        pending: MessageSeq,
        recv_buf: HashSet<MessageSeq>,
    },
    ReliableOrdered {
        pending: MessageSeq,
        recv_buf: HashMap<MessageSeq, Vec<u8>>,
    },
}
