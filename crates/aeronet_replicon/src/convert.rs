//! Allows converting between [`bevy_replicon`] and [`crate`] types.

use {
    aeronet_transport::lane::{LaneIndex, LaneKind},
    bevy_ecs::prelude::*,
    bevy_replicon::prelude::*,
};

/// Converts an [`Entity`] into a [`ClientId`].
///
/// [`bevy_replicon`] requires a [`ClientId`] to identify clients connected to a
/// server. To satisfy this, we use the raw [`u64`] of the [`Entity`] session as
/// the [`ClientId`]. This means that once a client session [`Entity`] has been
/// created, only that entity alone must refer to that client for the rest of
/// the client's lifetime.
///
/// See [`to_entity`].
#[must_use]
pub const fn to_client_id(entity: Entity) -> ClientId {
    ClientId::new(entity.to_bits())
}

/// Attempts to convert a [`ClientId`] into an [`Entity`].
///
/// Returns [`None`] if the client ID does not represent a valid entity.
/// Any value created from [`to_client_id`] will be valid for this function.
///
/// See [`to_client_id`].
#[must_use]
pub fn to_entity(client_id: ClientId) -> Option<Entity> {
    Entity::try_from_bits(client_id.get()).ok()
}

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
    u8::try_from(lane_index.into_raw()).ok()
}

/// Converts a channel ID into a [`LaneIndex`].
///
/// See [`to_channel_id`].
#[must_use]
pub fn to_lane_index(channel_id: u8) -> LaneIndex {
    LaneIndex::from_raw(u32::from(channel_id))
}
