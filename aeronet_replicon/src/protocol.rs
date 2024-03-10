use std::convert::Infallible;

use aeronet::{
    bytes::Bytes,
    lane::LaneIndex,
    message::{Message, TryFromBytes, TryIntoBytes},
};

#[derive(Debug, Clone, Message)]
pub struct RepliconMessage {
    pub channel_id: u8,
    pub payload: Bytes,
}

impl TryIntoBytes for RepliconMessage {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        Ok(self.payload)
    }
}

impl TryFromBytes for RepliconMessage {
    // TODO!! how?
}

impl LaneIndex for RepliconMessage {
    fn lane_index(&self) -> usize {
        usize::from(self.channel_id)
    }
}
