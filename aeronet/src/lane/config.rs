use std::{fmt::Debug, time::Duration};

use crate::LaneKind;

/// Stores configuration options for a lane behaves.
///
/// These map directly to the lane types in `aeronet_protocol`, but are defined
/// here so that users can use these types without having to import the protocol
/// directly.
#[derive(Debug, Clone)]
pub struct LaneConfig {
    /// Kind of lane that this configuration will create.
    pub kind: LaneKind,
    /// For [unreliable] lanes: if a message does not receive a new fragment in
    /// this duration of time, it will be automatically dropped, and the message
    /// will be considered lost.
    ///
    /// [unreliable](LaneKind::is_reliable)
    pub drop_after: Duration,
    /// For [reliable] lanes: how long to wait until resending a fragment which
    /// was not acknowledged by the peer.
    ///
    /// The initial send is always instant.
    ///
    /// [reliable](LaneKind::is_reliable)
    pub resend_after: Duration,
    /// For [reliable] lanes: if:
    /// * any incoming message does not receive all its fragments, or
    /// * any outgoing message does not receive acknowledgements for all its
    ///   fragments
    ///
    /// after the given duration, the lane is considered timed out, and the
    /// connection must be closed.
    ///
    /// [reliable](LaneKind::is_reliable)
    pub ack_timeout: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self {
            kind: LaneKind::UnreliableUnsequenced,
            drop_after: Duration::from_secs(3),
            resend_after: Duration::from_millis(100),
            ack_timeout: Duration::from_secs(30),
        }
    }
}

impl LaneConfig {
    pub fn with_defaults(kind: LaneKind) -> Self {
        Self {
            kind,
            ..Default::default()
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
    /// List of all variants for this type.
    const VARIANTS: &'static [Self];

    /// Gets the configuration for this lane.
    fn config(&self) -> LaneConfig;

    /// Gets a list of all lane configurations associated with this lane key.
    ///
    /// This can be passed to transports which accept lanes to configure which
    /// lanes it will use.
    fn configs() -> Box<[LaneConfig]> {
        Self::VARIANTS.iter().map(Self::config).collect()
    }
}
