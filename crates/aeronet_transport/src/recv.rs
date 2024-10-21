use ahash::{HashMap, HashSet};
use typesize::derive::TypeSize;

use crate::{frag::FragmentReceiver, packet::MessageSeq, rtt::RttEstimator};

#[derive(Debug, TypeSize)]
pub(crate) struct Receiver {
    lanes: Box<[RecvLane]>,
    rtt: RttEstimator,
}

#[derive(Debug, Clone, TypeSize)]
struct RecvLane {
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
