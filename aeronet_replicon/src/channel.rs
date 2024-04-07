use aeronet::lane::LaneKind;
use bevy_replicon::core::replicon_channels::{ChannelKind, RepliconChannel, RepliconChannels};

// TODO resend_time and max_bytes
// BIG TODO!!!

/// Extension functions on [`RepliconChannels`] to convert channels into
/// [lanes](aeronet::lane).
pub trait RepliconChannelsExt {
    /// Converts these channels into client-to-server (client out) lanes.
    fn to_c2s_lanes(&self) -> Box<[LaneKind]>;

    /// Converts these channels into server-to-client (server out) lanes.
    fn to_s2c_lanes(&self) -> Box<[LaneKind]>;
}

impl RepliconChannelsExt for RepliconChannels {
    fn to_c2s_lanes(&self) -> Box<[LaneKind]> {
        self.client_channels().iter().map(to_lane).collect()
    }

    fn to_s2c_lanes(&self) -> Box<[LaneKind]> {
        self.server_channels().iter().map(to_lane).collect()
    }
}

fn to_lane(channel: &RepliconChannel) -> LaneKind {
    match channel.kind {
        ChannelKind::Unreliable => LaneKind::UnreliableUnordered,
        ChannelKind::Unordered => LaneKind::ReliableUnordered,
        ChannelKind::Ordered => LaneKind::ReliableOrdered,
    }
}
