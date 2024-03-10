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

impl LaneIndex for RepliconMessage {
    fn lane_index(&self) -> usize {
        usize::from(self.channel_id)
    }
}

impl TryFromBytesAndLane for RepliconMessage {
    type Error = RepliconMessageError;

    fn try_from_bytes_and_lane(payload: Bytes, lane_index: usize) -> Result<Self, Self::Error> {
        let channel_id = u8::try_from(lane_index)
            .map_err(|_| RepliconMessageError::LaneIndexTooLarge { lane_index })?;
        Ok(Self {
            channel_id,
            payload,
        })
    }
}
