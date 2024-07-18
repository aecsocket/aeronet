use aeronet::lane::LaneKind;
use bevy_replicon::prelude::{ChannelKind, RepliconChannel};

use crate::session::LaneConfig;

impl From<&RepliconChannel> for LaneConfig {
    fn from(value: &RepliconChannel) -> Self {
        Self {
            kind: match value.kind {
                ChannelKind::Unreliable => LaneKind::UnreliableUnordered,
                ChannelKind::Unordered => LaneKind::ReliableUnordered,
                ChannelKind::Ordered => LaneKind::ReliableOrdered,
            },
            send_bytes_per_sec: usize::MAX,
            resend_after: value.resend_time,
        }
    }
}

impl From<RepliconChannel> for LaneConfig {
    fn from(value: RepliconChannel) -> Self {
        Self::from(&value)
    }
}
