//! Provides guarantees for message delivery and receiving.
//!
//! Lanes are analogous to channels or streams in other protocols, which allow
//! sending messages across different logical lanes with different guarantees,
//! but over the same connection. Each lane is independent of any other lane, so
//! e.g. one lane does not block the head of another lane (head-of-line
//! blocking).
//!
//! The name "lanes" was chosen in order to reduce ambiguity:
//! * *streams* may be confused with TCP, QUIC, or WebTransport streams
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
//! choosing which kind of lane to use. A lane kind with head-of-line blocking
//! may block when it is awaiting a message sent earlier, in order to maintain
//! ordering; others may not.
//!
//! Note that lane kinds provide a *minimum* guarantee of reliability and
//! ordering - a transport may provide some guarantees even if using a less
//! reliable lane kind. If a transport does not support lanes, then it
//! guarantees that all messages are sent with the strictest guarantees
//! (reliable-ordered).
//!
//! | [`LaneKind`]              | Reliability | Ordering |
//! |---------------------------|-------------|----------|
//! | [`UnreliableUnordered`]   |             | (1)      |
//! | [`UnreliableSequenced`]   |             | (1,2)    |
//! | [`ReliableUnordered`]     | ✅           |          |
//! | [`ReliableOrdered`]       | ✅           | (3)      |
//!
//! 1. If delivery of a single chunk fails, delivery of the whole packet fails.
//! 2. If messages X and Y are sent, and X arrives after Y, then X is discarded.
//! 3. If delivery of a single chunk fails, delivery of all messages halts until
//!    that single chunk is received.
//!
//! Note: Although reliable-sequenced is possible in theory, this crate does not
//! support this kind of lane. "Reliable-sequenced" is not actually reliable, as
//! messages *may* be dropped if they are older than the last received message.
//! You should probably use [`UnreliableSequenced`] instead.
//!
//! [`UnreliableUnordered`]: LaneKind::UnreliableUnordered
//! [`UnreliableSequenced`]: LaneKind::UnreliableSequenced
//! [`ReliableUnordered`]: LaneKind::ReliableUnordered
//! [`ReliableOrdered`]: LaneKind::ReliableOrdered

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
    /// Implementations may suffer from head-of-line blocking if new messages
    /// cannot be received because our peer is stuck waiting to receive the
    /// final parts of a previous message.
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
    pub const fn reliability(&self) -> LaneReliability {
        match self {
            Self::UnreliableUnordered | Self::UnreliableSequenced => LaneReliability::Unreliable,
            Self::ReliableUnordered | Self::ReliableOrdered => LaneReliability::Reliable,
        }
    }

    /// Gets the ordering of this lane kind.
    #[must_use]
    pub const fn ordering(&self) -> LaneOrdering {
        match self {
            Self::UnreliableUnordered | Self::ReliableUnordered => LaneOrdering::Unordered,
            Self::UnreliableSequenced => LaneOrdering::Sequenced,
            Self::ReliableOrdered => LaneOrdering::Ordered,
        }
    }
}

/// Index of a lane.
///
/// # Correctness
///
/// When creating a transport, you pass a list of [`LaneKind`]s in to define
/// which lanes are available for it to send and receive on. Functions which
/// work with a transport and a lane index (e.g. [`ClientTransport::send`])
/// expect to be given a [`LaneIndex`] which is valid for that instance of the
/// transport. If this index is for a different instance, then the transport may
/// panic.
///
/// [`ClientTransport::send`]: crate::client::ClientTransport::send
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, arbitrary::Arbitrary)]
pub struct LaneIndex(u64);

impl LaneIndex {
    /// Creates a new lane index from a raw index.
    ///
    /// # Correctness
    ///
    /// See [`LaneIndex`].
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Gets the raw index of this lane.
    #[must_use]
    pub const fn into_raw(self) -> u64 {
        self.0
    }
}
