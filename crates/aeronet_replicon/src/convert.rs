//! Allows converting between [`bevy_replicon`] and [`crate`] types.

use {
    aeronet_transport::{
        lane::{LaneIndex, LaneKind},
        size::MinSize,
    },
    bevy_replicon::prelude::*,
};

// ChannelKind -> LaneKind

/// Converts a [`Channel`] into a [`LaneKind`].
///
/// This is a lossless conversion, since [`LaneKind`] has more variants than
/// [`Channel`]. However, converting a [`LaneKind`] into a [`Channel`] may be
/// lossy, so is not defined.
#[must_use]
pub const fn to_lane_kind(channel: Channel) -> LaneKind {
    match channel {
        Channel::Unreliable => LaneKind::UnreliableUnordered,
        Channel::Unordered => LaneKind::ReliableUnordered,
        Channel::Ordered => LaneKind::ReliableOrdered,
    }
}

// LaneIndex <-> channel ID

/// Converts a [`LaneIndex`] to a channel ID.
///
/// [`aeronet_transport`]'s lanes and [`bevy_replicon`]'s channels serve a very
/// similar purpose, so we can map between lane indices and channel IDs
/// directly - as long as the channel ID fits into the size of a [`LaneIndex`].
///
/// See [`to_lane_index`].
#[must_use]
pub fn to_channel_id(lane_index: LaneIndex) -> usize {
    usize::from(lane_index.0)
}

/// Attempts to convert a channel ID into a [`LaneIndex`].
///
/// Returns [`None`] if this channel ID cannot fit into a [`LaneIndex`].
///
/// See [`to_channel_id`].
#[must_use]
pub fn to_lane_index(channel_id: usize) -> Option<LaneIndex> {
    MinSize::try_from(channel_id).map(LaneIndex).ok()
}
