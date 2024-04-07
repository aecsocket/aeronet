use aeronet::lane::LaneKind;
use ahash::{AHashMap, AHashSet};
use derivative::Derivative;
use either::Either;

use crate::seq::Seq;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub enum LaneReceiver<R> {
    UnreliableUnordered,
    UnreliableSequenced {
        /// Sequence number of the last message received.
        last_recv_seq: Seq,
    },
    ReliableUnordered {
        /// Next message sequence that we expect to receive, if transmission was
        /// guaranteed to be in order.
        ///
        /// All message sequences below this value are guaranteed to already
        /// have been received.
        pending_seq: Seq,
        /// Tracks message sequences **after `pending_seq`** which have already been
        /// received.
        ///
        /// Once `pending_seq` increases, all entries in this buffer older than
        /// `pending_seq` are removed.
        recv_seq_buf: AHashSet<Seq>,
    },
    ReliableOrdered {
        /// Next message sequence that we expect to receive, if transmission was
        /// guaranteed to be in order.
        ///
        /// All message sequences below this value are guaranteed to already
        /// have been received.
        pending_seq: Seq,
        /// Tracks messages **after `pending_seq`** which have already been
        /// received.
        ///
        /// Once `pending_seq` increases, all entries in this buffer older than
        /// `pending_seq` are removed.
        #[derivative(Debug = "ignore")]
        recv_buf: AHashMap<Seq, R>,
    },
}

impl<R> LaneReceiver<R> {
    pub fn new(kind: LaneKind) -> Self {
        match kind {
            LaneKind::UnreliableUnordered => Self::UnreliableUnordered,
            LaneKind::UnreliableSequenced => Self::UnreliableSequenced {
                last_recv_seq: Seq::MAX,
            },
            LaneKind::ReliableUnordered => Self::ReliableUnordered {
                pending_seq: Seq(0),
                recv_seq_buf: AHashSet::new(),
            },
            LaneKind::ReliableOrdered => Self::ReliableOrdered {
                pending_seq: Seq(0),
                recv_buf: AHashMap::new(),
            },
        }
    }

    pub fn recv(&mut self, msg: R, msg_seq: Seq) -> impl Iterator<Item = R> + '_ {
        match self {
            Self::UnreliableUnordered => Either::Left(Some(msg)),
            Self::UnreliableSequenced { last_recv_seq } => {
                if msg_seq > *last_recv_seq {
                    // msg is new, return it
                    *last_recv_seq = msg_seq;
                    Either::Left(Some(msg))
                } else {
                    // msg is old, drop it
                    Either::Left(None)
                }
            }
            Self::ReliableUnordered {
                pending_seq,
                recv_seq_buf,
            } => {
                if msg_seq < *pending_seq {
                    // msg is guaranteed to already be received, drop it
                    Either::Left(None)
                } else {
                    // here's an example to visualize what this does:
                    // msg_seq: 40
                    // pending_seq: 40, recv_seq_buf: [41, 45]
                    recv_seq_buf.insert(msg_seq);
                    // pending_seq: 40, recv_seq_buf: [40, 41, 45]
                    while recv_seq_buf.contains(pending_seq) {
                        recv_seq_buf.remove(pending_seq);
                        *pending_seq += Seq(1);
                        // iter 1: pending_seq: 41, recv_seq_buf: [41, 45]
                        // iter 2: pending_seq: 42, recv_seq_buf: [45]
                    }
                    Either::Left(Some(msg))
                }
            }
            Self::ReliableOrdered {
                pending_seq,
                recv_buf,
            } => {
                if msg_seq < *pending_seq {
                    // msg is guaranteed to already be received, drop it
                    Either::Left(None)
                } else {
                    // almost identical to above, but we also return the
                    // messages that we remove
                    recv_buf.insert(msg_seq, msg);
                    Either::Right(std::iter::from_fn(move || {
                        let msg = recv_buf.remove(pending_seq)?;
                        *pending_seq += Seq(1);
                        Some(msg)
                    }))
                }
            }
        }
        .into_iter()
    }
}
