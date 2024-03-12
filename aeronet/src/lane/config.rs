use std::{fmt::Debug, time::Duration};

use super::{LaneIndex, LaneKind};

/// Stores configuration options for how a [lane](crate::lane) behaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneConfig {
    /// Kind of lane that this configuration will create.
    pub kind: LaneKind,
    /// For [unreliable](LaneKind::is_reliable) lanes: if a message does not receive a new fragment in
    /// this duration of time, it will be automatically dropped, and the message
    /// will be considered lost.
    pub drop_after: Duration,
    /// For [reliable] lanes: how long to wait until resending a fragment which
    /// was not acknowledged by the peer.
    ///
    /// The initial send is always instant.
    ///
    /// [reliable]: LaneKind::is_reliable
    pub resend_after: Duration,
    /// For [reliable] lanes: if:
    /// * any incoming message does not receive all its fragments, or
    /// * any outgoing message does not receive acknowledgements for all its
    ///   fragments
    ///
    /// after the given duration, the lane is considered timed out, and the
    /// connection must be closed.
    ///
    /// [reliable]: LaneKind::is_reliable
    pub ack_timeout: Duration,
}

impl Default for LaneConfig {
    fn default() -> Self {
        Self {
            kind: LaneKind::UnreliableUnordered,
            drop_after: Duration::from_secs(3),
            resend_after: Duration::from_millis(100),
            ack_timeout: Duration::from_secs(30),
        }
    }
}

/// App-defined type listing a set of [lanes](crate::lane) which a transport can
/// use to send app messages along.
///
/// This trait should be derived - see [`aeronet_derive::LaneKey`]. Otherwise,
/// you will have to make sure to follow the contract regarding panics.
///
/// There isn't much point to implementing this yourself - if you need
/// fine-grained control over lanes, use [`LaneIndex`].
///
/// # Panic safety
///
/// This trait must be implemented correctly, otherwise transport
/// implementations may panic.
pub trait LaneKey {
    /// Gets which lane index this variant represents.
    fn lane_index(&self) -> LaneIndex;

    /// Gets the configurations used for representing lanes of this type.
    fn configs() -> &'static [LaneConfig];
}

impl<T: LaneKey> From<T> for LaneIndex {
    fn from(value: T) -> Self {
        value.lane_index()
    }
}
