/// Index of a lane as specified in a transport constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LaneIndex(usize);

impl LaneIndex {
    /// Creates a new lane index from a raw index.
    ///
    /// # Correctness
    ///
    /// When creating a transport, you pass a set of [`LaneKind`]s in to define
    /// which lanes are available for it to use.
    /// Functions which accept a [`LaneIndex`] expect to be given a valid index
    /// into this list. If this index is for a different configuration, then the
    /// transport will most likely panic.
    ///
    /// [`LaneKind`]: crate::lane::LaneKind
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

/// Defines what [lane] a [`Message`] is either sent or received on.
///
/// This trait can be derived - see [`aeronet_derive::OnLane`].
///
/// If you are unable to implement this trait manually because you lack some
/// important context for getting the lane of this message, take a look at
/// [`LaneMapper`].
///
/// [lane]: crate::lane
/// [`Message`]: crate::message::Message
pub trait OnLane {
    /// Gets the index of the lane that this is sent out on.
    fn lane_index(&self) -> LaneIndex;
}

/// Allows reading the lane index of a message.
///
/// Transports may include a value implementing this trait as a field, and use
/// it to map their messages to a lane index.
///
/// # How do I make one?
///
/// If your message type already implements [`OnLane`] you don't need to make
/// your own. Just use `()` as the mapper value - it implements this trait.
///
/// # Why use this over [`OnLane`]?
///
/// In some cases, you may not have all the context you need in the message
/// itself in order to be able to read its lane index. You can instead store
/// this state in a type implementing this trait. When the transport attempts to
/// get a message's lane, it will call this value's function, letting you use
/// your existing context for the conversion.
pub trait LaneMapper<T> {
    /// Gets the lane index of the given message.
    fn lane_index(&self, msg: &T) -> LaneIndex;
}

impl<T: OnLane> LaneMapper<T> for () {
    fn lane_index(&self, msg: &T) -> LaneIndex {
        msg.lane_index()
    }
}
