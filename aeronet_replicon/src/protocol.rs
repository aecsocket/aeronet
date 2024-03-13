use std::convert::Infallible;

use aeronet::{
    bytes::Bytes,
    lane::{LaneIndex, TryFromBytesAndLane},
    message::{Message, TryIntoBytes},
};

#[derive(Debug, Clone, Message)]
pub struct RepliconMessage {
    pub channel_id: u8,
    pub payload: Bytes,
}

impl RepliconMessage {
    pub fn lane_index(&self) -> LaneIndex {
        LaneIndex::from_raw(usize::from(self.channel_id))
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RepliconMessageError {
    #[error("lane index `{lane_index}` too large")]
    LaneIndexTooLarge { lane_index: usize },
}

impl TryIntoBytes for RepliconMessage {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        Ok(self.payload)
    }
}

impl TryFromBytesAndLane for RepliconMessage {
    type Error = RepliconMessageError;

    fn try_from_bytes_and_lane(payload: Bytes, lane_index: LaneIndex) -> Result<Self, Self::Error> {
        let lane_index = lane_index.into_raw();
        let channel_id = u8::try_from(lane_index)
            .map_err(|_| RepliconMessageError::LaneIndexTooLarge { lane_index })?;
        Ok(Self {
            channel_id,
            payload,
        })
    }
}
