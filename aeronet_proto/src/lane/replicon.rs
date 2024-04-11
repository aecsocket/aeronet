use aeronet::lane::LaneKind;
use bevy_replicon::prelude::*;

use super::LaneConfig;

impl LaneConfig {
    /// Creates a [`LaneConfig`] from a [`RepliconChannel`].
    pub fn from_channel(channel: &RepliconChannel) -> Self {
        Self {
            kind: LaneKind::from(channel.kind),
            resend_after: channel.resend_time,
        }
    }
}

/// Extension trait for converting [`RepliconChannels`] into [`LaneConfig`]
/// values.
pub trait RepliconChannelsExt {
    /// Converts [`RepliconChannels::client_channels`] into [`LaneConfig`]s.
    fn client_lanes(&self) -> impl Iterator<Item = LaneConfig>;

    /// Converts [`RepliconChannels::server_channels`] into [`LaneConfig`]s.
    fn server_lanes(&self) -> impl Iterator<Item = LaneConfig>;

    /// Converts these channels into the **client-side outgoing lane**
    /// configurations.
    fn client_send_lanes(&self) -> impl Iterator<Item = LaneConfig> {
        self.client_lanes()
    }

    /// Converts these channels into the **client-side incoming lane**
    /// configurations.
    fn client_recv_lanes(&self) -> impl Iterator<Item = LaneConfig> {
        self.server_lanes()
    }

    /// Converts these channels into the **server-side outgoing lane**
    /// configurations.
    fn server_send_lanes(&self) -> impl Iterator<Item = LaneConfig> {
        self.server_lanes()
    }

    /// Converts these channels into the **server-side incoming lane**
    /// configurations.
    fn server_recv_lanes(&self) -> impl Iterator<Item = LaneConfig> {
        self.client_lanes()
    }
}

impl RepliconChannelsExt for RepliconChannels {
    fn client_lanes(&self) -> impl Iterator<Item = LaneConfig> {
        self.client_channels().iter().map(LaneConfig::from_channel)
    }

    fn server_lanes(&self) -> impl Iterator<Item = LaneConfig> {
        self.server_channels().iter().map(LaneConfig::from_channel)
    }
}
