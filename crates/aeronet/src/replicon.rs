use bevy_replicon::prelude::{ChannelKind, RepliconChannel};

use crate::lane::LaneKind;

impl From<&ChannelKind> for LaneKind {
    fn from(value: &ChannelKind) -> Self {
        match value {
            ChannelKind::Unreliable => Self::UnreliableUnordered,
            ChannelKind::Unordered => Self::ReliableUnordered,
            ChannelKind::Ordered => Self::ReliableOrdered,
        }
    }
}

impl From<ChannelKind> for LaneKind {
    fn from(value: ChannelKind) -> Self {
        Self::from(&value)
    }
}

impl From<&RepliconChannel> for LaneKind {
    fn from(value: &RepliconChannel) -> Self {
        Self::from(value.kind)
    }
}

impl From<RepliconChannel> for LaneKind {
    fn from(value: RepliconChannel) -> Self {
        Self::from(value.kind)
    }
}
