use std::convert::Infallible;

use aeronet::{
    bytes::{Bytes, BytesMut},
    lane::{LaneIndex, OnLane},
    message::{Message, TryFromBytes, TryIntoBytes},
    octs::{BytesError, ReadBytes, WriteBytes},
};

/// Message type used by [`bevy_replicon`]-compatible [`TransportProtocol`]s.
///
/// [`TransportProtocol`]: aeronet::protocol::TransportProtocol
#[derive(Debug, Clone, Message)]
pub struct RepliconMessage {
    /// Replicon channel ID.
    ///
    /// This maps one-to-one to a [`LaneIndex`].
    pub channel_id: u8,
    /// Message payload.
    pub payload: Bytes,
}

impl OnLane for RepliconMessage {
    fn lane_index(&self) -> LaneIndex {
        LaneIndex::from_raw(usize::from(self.channel_id))
    }
}

/// Error when reading a [`RepliconMessage`] from bytes.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RepliconMessageError {
    /// Failed to read the channel ID.
    #[error("failed to read channel id")]
    ReadChannelId(BytesError),
}

impl TryIntoBytes for RepliconMessage {
    type Error = Infallible;

    fn try_into_bytes(self) -> Result<Bytes, Self::Error> {
        let mut bytes = BytesMut::with_capacity(self.payload.len() + 1);
        // CORRECTNESS: we just allocated enough capacity for all of this
        bytes.write(&self.channel_id).unwrap();
        bytes.write_slice(&self.payload).unwrap();
        Ok(bytes.freeze())
    }
}

impl TryFromBytes for RepliconMessage {
    type Error = RepliconMessageError;

    fn try_from_bytes(mut buf: Bytes) -> Result<Self, Self::Error> {
        let channel_id = buf
            .read::<u8>()
            .map_err(RepliconMessageError::ReadChannelId)?;
        Ok(Self {
            channel_id,
            payload: buf,
        })
    }
}
