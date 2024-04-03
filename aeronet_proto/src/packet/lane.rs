use aeronet::lane::LaneKind;
use ahash::{AHashMap, AHashSet};
use derivative::Derivative;
use either::Either;

use crate::seq::Seq;

#[derive(Derivative)]
#[derivative(Debug(bound = ""))]
pub enum LaneState<R> {
    UnreliableUnordered,
    UnreliableSequenced {
        /// Sequence number of the last message received.
        last_recv_seq: Seq,
    },
    ReliableUnordered {
        /// All seqs under this value are considered guaranteed to already be
        /// received.
        pending_seq: Seq,
        /// Tracks message seqs **after `pending_seq`** which have already been
        /// received.
        ///
        /// Once `pending_seq` increases, all entries in this buffer older than
        /// `pending_seq` are removed.
        recv_seq_buf: AHashSet<Seq>,
    },
    ReliableSequenced {
        /// Sequence number of the last message received.
        last_recv_seq: Seq,
    },
    ReliableOrdered {
        /// All seqs under this value are considered guaranteed to already be
        /// received.
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

impl<R> LaneState<R> {
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
            LaneKind::ReliableSequenced => Self::ReliableSequenced {
                last_recv_seq: Seq::MAX,
            },
            LaneKind::ReliableOrdered => Self::ReliableOrdered {
                pending_seq: Seq(0),
                recv_buf: AHashMap::new(),
            },
        }
    }

    pub fn drop_on_flush(&self) -> bool {
        match self {
            Self::UnreliableUnordered | Self::UnreliableSequenced { .. } => true,
            Self::ReliableUnordered { .. }
            | Self::ReliableSequenced { .. }
            | Self::ReliableOrdered { .. } => false,
        }
    }

    // TODO coroutines
    pub fn recv(&mut self, msg: R, msg_seq: Seq) -> impl Iterator<Item = R> + '_ {
        match self {
            Self::UnreliableUnordered => Either::Left(Some(msg)),
            Self::UnreliableSequenced { last_recv_seq }
            | Self::ReliableSequenced { last_recv_seq } => Either::Left({
                // if msg is older than the last received msg, just drop it
                if msg_seq > *last_recv_seq {
                    *last_recv_seq = msg_seq;
                    Some(msg)
                } else {
                    None
                }
            }),
            Self::ReliableUnordered {
                pending_seq,
                recv_seq_buf,
            } => {
                if msg_seq < *pending_seq {
                    // msg is guaranteed to already be received
                    Either::Left(None)
                } else {
                    // here's an example to visualize what this does:
                    // msg_seq: 41
                    // pending_seq: 40, recv_seq_buf: [45]
                    recv_seq_buf.insert(msg_seq);
                    // pending_seq: 40, recv_seq_buf: [41, 45]
                    while recv_seq_buf.contains(pending_seq) {
                        recv_seq_buf.remove(pending_seq);
                        *pending_seq += Seq(1);
                        // pending_seq: 41, recv_seq_buf: [45]
                    }
                    Either::Left(Some(msg))
                }
            }
            Self::ReliableOrdered {
                pending_seq,
                recv_buf,
            } => {
                if msg_seq < *pending_seq {
                    // msg is guaranteed to already be received
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
