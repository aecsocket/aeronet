use std::{fmt::Debug, time::Duration};

use crate::LaneKind;

/// Stores configuration options for a lane behaves.
///
/// These map directly to the lane types in `aeronet_protocol`, but are defined
/// here so that users can use these types without having to import the protocol
/// directly.
#[derive(Debug, Clone)]
pub enum LaneConfig {
    /// See [`LaneKind::UnreliableUnsequenced`].
    UnreliableUnsequenced {
        ///
        drop_after: Duration,
    },
    /// See [`LaneKind::UnreliableSequenced`].
    UnreliableSequenced { drop_after: Duration },
    /// See [`LaneKind::ReliableUnordered`].
    ReliableUnordered {},
    /// See [`LaneKind::ReliableSequenced`].
    ReliableSequenced {},
    /// See [`LaneKind::ReliableOrdered`].
    ReliableOrdered {},
}

impl LaneConfig {
    pub fn kind(&self) -> LaneKind {
        match self {
            Self::UnreliableUnsequenced { .. } => LaneKind::UnreliableUnsequenced,
            Self::UnreliableSequenced { .. } => LaneKind::UnreliableSequenced,
            Self::ReliableUnordered { .. } => LaneKind::ReliableUnordered,
            Self::ReliableSequenced { .. } => LaneKind::ReliableSequenced,
            Self::ReliableOrdered { .. } => LaneKind::ReliableOrdered,
        }
    }
}

/// Defines what lane index a lane item represents.
///
/// This trait should be derived indirectly, by deriving [`LaneKey`].
///
/// This trait is deliberately left vague in order to allow users of lanes to
/// manage their own lanes and lane indices - but this comes at the risk of
/// panicking the transport!
///
/// # Panic safety
///
/// See [`LaneIndex::index`].
pub trait LaneIndex {
    /// Index of this lane in the initial transport lane configuration.
    ///
    /// When creating a transport which uses lanes, you will pass in a list of
    /// lanes that the transport uses. How this is done exactly is left up to
    /// the implementation. Each lane in that list has its own index, and this
    /// function maps to one of those lanes.
    ///
    /// # Panic safety
    ///
    /// The value returned by this function must be an in-bounds index into the
    /// list of lanes defined on transport creation.
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
    /// Gets the configuration for this lane.
    fn config(&self) -> LaneConfig;

    /// Gets a list of all lane configurations associated with this lane key.
    ///
    /// This can be passed to transports which accept lanes to configure which
    /// lanes it will use.
    fn configs() -> Box<[LaneConfig]>;
}
