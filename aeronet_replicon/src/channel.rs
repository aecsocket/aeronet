use aeronet::lane::LaneKind;
use bevy_replicon::core::replicon_channels::ChannelKind;

pub trait IntoLaneKind {
    fn into_lane_kind(self) -> LaneKind;
}

impl IntoLaneKind for ChannelKind {
    fn into_lane_kind(self) -> LaneKind {
        match self {
            Self::Unreliable => LaneKind::UnreliableUnordered,
            Self::Unordered => LaneKind::ReliableUnordered,
            Self::Ordered => LaneKind::ReliableOrdered,
        }
    }
}
