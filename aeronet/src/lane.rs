//! Provides guarantees for message delivery and receiving.
//!
//! Lanes are analogous to channels or streams in other protocols, which allow
//! sending messages across different logical lanes with different guarantees,
//! but over the same connection.
//! Each lane is independent of any other lane, so e.g. one lane does not block
//! the head of another lane (head-of-line blocking).
//!
//! The name "lanes" was chosen in order to reduce ambiguity:
//! * *streams* may be confused with TCP or WebTransport streams
//! * *channels* may be confused with MPSC channels
//!
//! # Guarantees
//!
//! Lanes mainly offer guarantees about:
//! * [reliability](LaneReliability) - ensuring that a message reaches the other
//!   side without being lost; and if it is lost, it is resent
//! * [ordering](LaneOrdering) - ensuring that messages are received in the same
//!   order that they are sent
//!
//! Although it is not a part of the guarantees laid out by the lane kinds,
//! *head-of-line blocking* is also an important factor to consider when
//! choosing which kind of lane to use. A lane kind with head-of-line
//! blocking may block when it is awaiting a message sent earlier, in order to
//! maintain ordering; others may not.
//!
//! Note that lane kinds provide a *minimum* guarantee of reliability and
//! ordering - a transport may provide some guarantees even if using a less
//! reliable lane kind.
//!
//! | [`LaneKind`]              | Fragmentation | Reliability | Ordering |
//! |---------------------------|---------------|-------------|----------|
//! | [`UnreliableUnordered`] | ✅            |              |          |
//! | [`UnreliableSequenced`]   | ✅            |              | (1)      |
//! | [`ReliableUnordered`]     | ✅            | ✅            |          |
//! | [`ReliableSequenced`]     | ✅            | ✅            | (1)      |
//! | [`ReliableOrdered`]       | ✅            | ✅            | (2)      |
//!
//! 1. If delivery of a single chunk fails, delivery of the whole packet fails
//!    (unreliable). If the message arrives later than a message sent and
//!    received previously, the message is discarded (sequenced, not ordered).
//! 2. If delivery of a single chunk fails, delivery of all messages halts until
//!    that single chunk is received (reliable ordered)..
//!
//! [`UnreliableUnordered`]: LaneKind::UnreliableUnordered
//! [`UnreliableSequenced`]: LaneKind::UnreliableSequenced
//! [`ReliableUnordered`]: LaneKind::ReliableUnordered
//! [`ReliableSequenced`]: LaneKind::ReliableSequenced
//! [`ReliableOrdered`]: LaneKind::ReliableOrdered
//!
//! # Transports
//!
//! Not all transport implementations may offer lanes. If they do, they will
//! usually have an [`OnLane`] bound on the outgoing message type

pub use aeronet_derive::{LaneKey, OnLane};

/// Kind of lane which can provide guarantees about the manner of message
/// delivery.
///
/// See [`lane`](crate::lane).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneKind {
    /// No guarantees given on *reliability* or *ordering*.
    ///
    /// This is useful for messages which should be sent in a fire-and-forget
    /// manner: that is, you don't expect to get a response for this message,
    /// and it is OK if a few messages are lost in transit.
    ///
    /// This lane kind typically has the best performance, as it does not
    /// require any sort of handshaking to ensure that messages have arrived
    /// from one side to the other.
    ///
    /// For example, spawning particle effects could be sent as an unreliable
    /// unordered message, as it is a low-priority message which we don't
    /// really care much about.
    UnreliableUnordered,
    /// Messages are *unreliable* but only messages newer than the last
    /// message will be received.
    ///
    /// Similar to [`LaneKind::UnreliableUnordered`], but any messages which
    /// are received and are older than an already-received message will be
    /// instantly dropped.
    ///
    /// This lane kind has the same performance as
    /// [`LaneKind::UnreliableUnordered`].
    ///
    /// An example of a message using this lane kind is a player positional
    /// update, sent to the server whenever a client moves in a game world.
    /// Since the game client will constantly be sending positional update
    /// messages at a high rate, it is OK if a few are lost in transit, as the
    /// server will hopefully catch the next messages. However, positional
    /// updates should not make the player go back in time - so any messages
    /// older than the most recent ones are dropped.
    UnreliableSequenced,
    /// Messages are sent *reliably* but the *ordering* is not guaranteed.
    ///
    /// This is useful for important one-off events where you need a guarantee
    /// that the message will be delivered, but the order in which it is
    /// delivered is not important.
    ///
    /// This lane kind is typically slower to send and receive than an
    /// unreliable message, but is still faster than an ordered lane because
    /// the implementation may be able to avoid head-of-line blocking.
    ///
    /// An example of a message using this lane kind is sending level data
    /// from a server to a client. It is not important what order the different
    /// parts of the level are received in, but it is important that they are
    /// all received.
    ReliableUnordered,
    /// Messages are sent *reliably* but only messages newer than the last
    /// message will be received.
    ///
    /// All messages are guaranteed to go through, but any messages which arrive
    /// out of order (a message sent earlier arrives at the peer later than
    /// another message) will be dropped.
    ///
    /// This lane kind has the same performance as a reliable unordered lane,
    /// and avoids head-of-line blocking.
    ///
    /// Honestly I couldn't come up with an example for this. This is not always
    /// a useful lane kind, as even though it's a reliable lane, it may also
    /// drop messages intentionally. This is mostly here for completeness' sake.
    ReliableSequenced,
    /// Messages are sent *reliably* and *ordered*.
    ///
    /// This is useful for important one-off events where you need a guarantee
    /// that the message will be delivered, and the order in which it's
    /// delivered is important.
    ///
    /// This lane kind offers the most guarantees, but is typically slower to
    /// send and receive than other lane kinds. Most notably, implementations
    /// may suffer from head-of-line blocking.
    ///
    /// Implementations may suffer from head-of-line blocking if a reliable
    /// lane is used, where messages cannot be received because they are
    /// being held up by a message sent earlier. To avoid this, you may use
    /// multiple different instances of this kind of lane, all of which hold
    /// their own message queues.
    ///
    /// An example of a message using this lane kind is sending chat messages
    /// from the server to the client. Since the server aggregates chat messages
    /// from different sources (system, other players, etc.) in a specific
    /// order, it must then tell its clients about the chat messages in that
    /// specific order as well.
    ReliableOrdered,
}

/// Guarantees that a [lane](crate::lane) provides with regards to delivering a
/// sent message to the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneReliability {
    /// There is no guarantee that a message will be delivered to the receiver.
    Unreliable,
    /// The message is guaranteed to be delivered to the receiver.
    Reliable,
}

/// Guarantees that a [lane](crate::lane) provides with regards to in what order
/// messages will be received.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneOrdering {
    /// There is no guarantee on the order that messages are received in.
    Unordered,
    /// Messages are guaranteed to be received in the order they were sent, but
    /// a message may be dropped if it arrives later than a message sent later
    /// but received earlier.
    ///
    /// For example, if messages A and B are sent in that order, and the
    /// receiver receives B then A in that order, it will discard A as it was
    /// sent earlier but received later than B.
    Sequenced,
    /// Messages are guaranteed to be received in the order that they were sent,
    /// with no dropped messages.
    ///
    /// This only makes sense if reliability is also
    /// [`LaneReliability::Reliable`].
    Ordered,
}

impl LaneKind {
    /// Gets the reliability of this lane kind.
    #[must_use]
    pub fn reliability(&self) -> LaneReliability {
        match self {
            Self::UnreliableUnordered | Self::UnreliableSequenced => LaneReliability::Unreliable,
            Self::ReliableUnordered | Self::ReliableSequenced | Self::ReliableOrdered => {
                LaneReliability::Reliable
            }
        }
    }

    /// Gets the ordering of this lane kind.
    #[must_use]
    pub fn ordering(&self) -> LaneOrdering {
        match self {
            Self::UnreliableUnordered | Self::ReliableUnordered => LaneOrdering::Unordered,
            Self::UnreliableSequenced | Self::ReliableSequenced => LaneOrdering::Sequenced,
            Self::ReliableOrdered => LaneOrdering::Ordered,
        }
    }
}

/// Index of a lane as specified in a transport constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LaneIndex(usize);

impl LaneIndex {
    /// Creates a new lane index from a raw index.
    ///
    /// # Panic safety
    ///
    /// When creating a transport, you pass a set of [`LaneKind`]s in to define
    /// which lanes are available for it to use.
    /// Functions which accept a [`LaneIndex`] expect to be given a valid index
    /// into this list. If this index is for a different configuration, then the
    /// transport will most likely panic.
    #[must_use]
    pub const fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    /// Gets the raw index of this value.
    #[must_use]
    pub const fn into_raw(self) -> usize {
        self.0
    }
}

/// Defines what [lane] a [`Message`] is sent on.
///
/// This trait can be derived - see [`aeronet_derive::OnLane`].
///
/// [lane]: crate::lane
/// [`Message`]: crate::message::Message
pub trait OnLane {
    /// Gets the index of the lane that this is sent out on.
    fn lane_index(&self) -> LaneIndex;
}

/// App-defined type listing a set of [lanes](crate::lane) which a transport can
/// use to send app messages along.
///
/// This trait should be derived - see [`aeronet_derive::LaneKey`]. Otherwise,
/// you will have to make sure to follow the contract regarding panics.
///
/// There isn't much point to implementing this yourself - if you need
/// fine-grained control over lanes, use [`LaneIndex`] manually.
///
/// # Panic safety
///
/// This trait must be implemented correctly, otherwise transport
/// implementations may panic.
pub trait LaneKey {
    /// Slice of all lane kinds under this key.
    ///
    /// Pass this into the constructor for your transport.
    const KINDS: &'static [LaneKind];

    /// Gets which lane index this variant represents.
    ///
    /// # Panic safety
    ///
    /// See [`LaneIndex`] for the guarantees you must uphold when implementing
    /// this.
    fn lane_index(&self) -> LaneIndex;
}

impl<T: LaneKey> From<T> for LaneIndex {
    fn from(value: T) -> Self {
        value.lane_index()
    }
}
