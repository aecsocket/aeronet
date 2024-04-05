use aeronet::lane::LaneKind;
use bevy_replicon::core::replicon_channels::{ChannelKind, RepliconChannel, RepliconChannels};

// TODO resend_time and max_bytes
// BIG TODO!!!
pub trait RepliconChannelsExt {
    fn to_client_lanes(&self) -> Box<[LaneKind]>;

    fn to_server_lanes(&self) -> Box<[LaneKind]>;
}

impl RepliconChannelsExt for RepliconChannels {
    fn to_client_lanes(&self) -> Box<[LaneKind]> {
        self.client_channels().iter().map(to_lane).collect()
    }

    fn to_server_lanes(&self) -> Box<[LaneKind]> {
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
