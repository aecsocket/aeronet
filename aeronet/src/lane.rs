use std::fmt::Debug;

/// Kind of lane which can provide guarantees about the manner of message
/// delivery.
///
/// This is analogous to channels or streams in other protocols, which allow
/// sending messages across different logical lanes with different guarantees.
/// Each lane is independent of any other lane, so e.g. one lane does not block
/// the head of another lane (head-of-line blocking).
///
/// The name "lanes" was chosen in order to reduce ambiguity:
/// * *streams* may be confused with TCP or WebTransport streams
/// * *channels* may be confused with MPSC channels
///
/// # Guarantees
///
/// Lanes mainly offer guarantees about:
/// * *reliability* - ensuring that a message reaches the other side without
///   being lost; and if it is lost, it is resent
/// * *ordering* - ensuring that messages are received in the same order that
///   they are sent
///
/// Although it is not a part of the guarantees laid out by the channel kinds,
/// *head-of-line blocking* is also an important factor to consider when
/// choosing which kind of channel to use. A channel kind with head-of-line
/// blocking may block when it is awaiting a message sent earlier, in order to
/// maintain ordering; others may not.
///
/// Note that channel kinds provide a *minimum* guarantee of reliability and
/// ordering - a transport may provide some guarantees even if using a less
/// reliable channel kind.
///
/// | [`LaneKind`]              | Fragmentation | Reliability | Ordering |
/// |---------------------------|---------------|-------------|----------|
/// | [`UnreliableUnsequenced`] | ✅            |              |          |
/// | [`UnreliableSequenced`]   | ✅            |              | (1)      |
/// | [`ReliableUnordered`]     | ✅            | ✅            |          |
/// | [`ReliableOrdered`]       | ✅            | ✅            | (2)      |
///
/// 1. If delivery of a single chunk fails, delivery of the whole packet fails
///    (unreliable). If the message arrives later than a message sent and
///    received previously, the message is discarded (sequenced, not ordered).
/// 2. If delivery of a single chunk fails, delivery of all messages halts until
///    that single chunk is received (reliable ordered).
///
/// # Transports
///
/// Not all transport implementations may offer lanes. If they do, they will
/// usually have an [`OnLane`] bound on the outgoing message type.
///
/// [`UnreliableUnsequenced`]: LaneKind::UnreliableUnsequenced
/// [`UnreliableSequenced`]: LaneKind::UnreliableSequenced
/// [`ReliableUnordered`]: LaneKind::ReliableUnordered
/// [`ReliableOrdered`]: LaneKind::ReliableOrdered
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
    UnreliableUnsequenced,
    /// Messages are *unreliable*, but if they are received, then only the
    /// latest messages are received.
    ///
    /// Similar to [`LaneKind::UnreliableUnsequenced`], but any messages which
    /// are received and are older than an already-received message will be
    /// instantly dropped.
    ///
    /// This lane kind has the same performance as
    /// [`LaneKind::UnreliableUnsequenced`].
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
    /// Messages are sent *reliably* and **ordered**.
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

#[derive(Debug, Clone)]
pub struct LaneConfig {
    pub kind: LaneKind,
}

// TODO docs
pub trait LaneIndex {
    /// Index of this lane in the [`LaneConfig`]
    // TODO
    ///
    /// # Panic safety
    ///
    /// This must be a valid index in the variants array, meaning:
    /// * it is not out of the bounds of the array
    /// * the value in the variants array at this index is identical to `self`
    fn index(&self) -> usize;
}

/// App-defined type listing a set of lanes which a transport can use to send
/// app messages along.
///
/// See [`LaneKind`] for an explanation of lanes.
///
/// This trait should be derived - see [`aeronet_derive::LaneKey`]. Otherwise,
/// you will have to make sure to follow the contract regarding panics.
///
/// # Panic safety
///
/// This trait must be implemented correctly, otherwise transport
/// implementations may panic.
pub trait LaneKey: Send + Sync + Debug + Clone + Copy + LaneIndex + 'static {
    /// All variants of this type that may exist.
    ///
    /// # Panic safety
    ///
    /// This must contain every possible value that may exist, otherwise
    /// transport implementations may panic.
    const VARIANTS: &'static [Self];

    fn config() -> Vec<LaneConfig> {
        Self::VARIANTS
            .iter()
            .map(|variant| LaneConfig {
                kind: variant.kind(),
            })
            .collect()
    }

    /// What kind of lane this value represents.
    fn kind(&self) -> LaneKind;
}

/// Defines what lane a [`Message`] is sent on.
///
/// See [`LaneKind`] for an explanation of lanes.
///
/// This trait can be derived - see [`aeronet_derive::OnLane`].
///
/// Note that this only affects what lane an *outgoing* message is *sent out*
/// on - it has no effect on incoming messages.
///
/// [`Message`]: crate::Message
pub trait OnLane {
    /// User-defined type of lane, output by [`OnLane::lane`].
    type Lane: LaneIndex;

    /// What lane this value is sent out on.
    fn lane(&self) -> Self::Lane;
}
