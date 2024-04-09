use aeronet::lane::LaneKind;
use bevy_replicon::prelude::*;

use super::LaneConfig;

impl LaneConfig {
    pub fn from_channel(channel: &RepliconChannel, default_bandwidth: usize) -> Self {
        Self {
            kind: LaneKind::from(channel.kind),
            bandwidth: channel.max_bytes.unwrap_or(default_bandwidth),
            resend_after: channel.resend_time,
        }
    }
}

pub trait RepliconChannelsExt {
    fn client_lanes(&self) -> Vec<LaneConfig>;

    fn server_lanes(&self) -> Vec<LaneConfig>;

    fn client_send_lanes(&self) -> Vec<LaneConfig> {
        self.client_lanes()
    }

    fn client_recv_lanes(&self) -> Vec<LaneConfig> {
        self.server_lanes()
    }

    fn server_send_lanes(&self) -> Vec<LaneConfig> {
        self.server_lanes()
    }

    fn server_recv_lanes(&self) -> Vec<LaneConfig> {
        self.client_lanes()
    }
}

impl RepliconChannelsExt for RepliconChannels {
    fn client_lanes(&self) -> Vec<LaneConfig> {
        self.client_channels()
            .iter()
            .map(|channel| LaneConfig::from_channel(channel, self.default_max_bytes))
            .collect()
    }

    fn server_lanes(&self) -> Vec<LaneConfig> {
        self.server_channels()
            .iter()
            .map(|channel| LaneConfig::from_channel(channel, self.default_max_bytes))
            .collect()
    }
}
