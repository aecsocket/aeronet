//! Allows losslessly converting between [`bevy_replicon`] types.
//!
//! # [`Entity`] ⇔ [`ClientId`]
//!
//! [`bevy_replicon`] requires a [`ClientId`] to identify clients connected to a
//! server. To satisfy this, we use the raw [`u64`] of the [`Entity`] session as
//! the [`ClientId`]. This means that once a client session [`Entity`] has been
//! created, only that entity alone must refer to that client for the rest of
//! the client's lifetime.
//!
//! # [`LaneIndex`] ⇔ channel ID
//!
//! [`aeronet_transport`]'s lanes and [`bevy_replicon`]'s channels serve a very
//! similar purpose, so we can map between lane indices and channel IDs
//! directly - as long as the [`LaneIndex`] is equal to or below [`u8::MAX`].

use {aeronet_transport::lane::LaneIndex, bevy_ecs::prelude::*, bevy_replicon::prelude::*};

// ClientId <-> Entity

/// Allows converting an [`Entity`] into a [`ClientId`].
pub trait IntoClientId {
    /// Performs the conversion.
    #[must_use]
    fn into_client_id(self) -> ClientId;
}

impl IntoClientId for Entity {
    fn into_client_id(self) -> ClientId {
        ClientId::new(self.to_bits())
    }
}

/// Allows converting a [`ClientId`] into an [`Entity`].
pub trait TryIntoEntity {
    /// Attempts the conversion.
    ///
    /// Returns [`None`] if this is not a valid [`Entity`].
    #[must_use]
    fn try_into_entity(self) -> Option<Entity>;
}

impl TryIntoEntity for ClientId {
    fn try_into_entity(self) -> Option<Entity> {
        Entity::try_from_bits(self.get()).ok()
    }
}

// LaneIndex <-> channel ID

/// Attempts to convert a [`LaneIndex`] into a channel ID.
pub trait TryIntoChannelId {
    /// Attempts the conversion.
    ///
    /// Returns [`None`] if this [`LaneIndex`] cannot fit into a [`u8`].
    #[must_use]
    fn try_into_channel_id(self) -> Option<u8>;
}

impl TryIntoChannelId for LaneIndex {
    fn try_into_channel_id(self) -> Option<u8> {
        u8::try_from(self.into_raw()).ok()
    }
}

/// Converts a channel ID into a [`LaneIndex`].
pub trait IntoLaneIndex {
    /// Performs the conversion.
    #[must_use]
    fn into_lane_index(self) -> LaneIndex;
}

impl IntoLaneIndex for u8 {
    fn into_lane_index(self) -> LaneIndex {
        LaneIndex::from_raw(u64::from(self))
    }
}
