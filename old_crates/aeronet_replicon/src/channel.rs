//! Utilities for working with Replicon channels.

use {
    aeronet::lane::LaneKind,
    bevy_replicon::prelude::{ChannelKind, RepliconChannel},
    std::borrow::Borrow,
};

/// Converts this into an iterator of [`LaneKind`]s.
///
/// You can use this to convert any `impl IntoIterator<Item = RepliconChannel>`
/// into an `impl Iterator<Item = LaneKind>`.
pub trait IntoLanes {
    /// Performs the conversion.
    fn into_lanes(self) -> impl Iterator<Item = LaneKind>;
}

impl<C: Borrow<RepliconChannel>, T: IntoIterator<Item = C>> IntoLanes for T {
    fn into_lanes(self) -> impl Iterator<Item = LaneKind> {
        self.into_iter().map(|channel| match channel.borrow().kind {
            ChannelKind::Unreliable => LaneKind::UnreliableUnordered,
            ChannelKind::Unordered => LaneKind::ReliableUnordered,
            ChannelKind::Ordered => LaneKind::ReliableOrdered,
        })
    }
}
