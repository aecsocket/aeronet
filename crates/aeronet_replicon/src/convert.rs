use aeronet_proto::lane::LaneIndex;
use bevy_ecs::prelude::*;
use bevy_replicon::prelude::*;

// ClientId <-> Entity

pub trait IntoClientId {
    #[must_use]
    fn into_client_id(self) -> ClientId;
}

impl IntoClientId for Entity {
    fn into_client_id(self) -> ClientId {
        ClientId::new(self.to_bits())
    }
}

pub trait TryIntoEntity {
    #[must_use]
    fn try_into_entity(self) -> Option<Entity>;
}

impl TryIntoEntity for ClientId {
    fn try_into_entity(self) -> Option<Entity> {
        Entity::try_from_bits(self.get()).ok()
    }
}

// LaneIndex <-> channel ID

pub trait IntoLaneIndex {
    #[must_use]
    fn into_lane_index(self) -> LaneIndex;
}

impl IntoLaneIndex for u8 {
    fn into_lane_index(self) -> LaneIndex {
        LaneIndex::from_raw(u64::from(self))
    }
}

pub trait TryIntoChannelId {
    #[must_use]
    fn try_into_channel_id(self) -> Option<u8>;
}

impl TryIntoChannelId for LaneIndex {
    fn try_into_channel_id(self) -> Option<u8> {
        u8::try_from(self.into_raw()).ok()
    }
}
