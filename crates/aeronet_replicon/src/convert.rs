//! Allows converting between [`bevy_replicon`] and [`crate`] types.

use {
    aeronet_transport::{
        lane::{LaneIndex, LaneKind},
        min_size::MinSize,
    },
    bevy_replicon::prelude::*,
};

// ChannelKind -> LaneKind

/// Converts a [`ChannelKind`] into a [`LaneKind`].
///
/// This is a lossless conversion, since [`LaneKind`] has more variants than
/// [`ChannelKind`]. However, converting a [`LaneKind`] into a [`ChannelKind`]
/// may be lossy, so is not defined.
#[must_use]
pub const fn to_lane_kind(channel_kind: ChannelKind) -> LaneKind {
    match channel_kind {
        ChannelKind::Unreliable => LaneKind::UnreliableUnordered,
        ChannelKind::Unordered => LaneKind::ReliableUnordered,
        ChannelKind::Ordered => LaneKind::ReliableOrdered,
    }
}

// LaneIndex <-> channel ID

/// Attempts to convert a [`LaneIndex`] to a channel ID.
///
/// [`aeronet_transport`]'s lanes and [`bevy_replicon`]'s channels serve a very
/// similar purpose, so we can map between lane indices and channel IDs
/// directly - as long as the [`LaneIndex`] is equal to or below [`u8::MAX`].
///
/// Returns [`None`] if this [`LaneIndex`] cannot fit into a [`u8`].
///
/// See [`to_lane_index`].
#[must_use]
pub fn to_channel_id(lane_index: LaneIndex) -> Option<u8> {
    u8::try_from(lane_index.0.0).ok()
}

/// Converts a channel ID into a [`LaneIndex`].
///
/// See [`to_channel_id`].
#[must_use]
pub fn to_lane_index(channel_id: u8) -> LaneIndex {
    LaneIndex(MinSize::from(channel_id))
}
