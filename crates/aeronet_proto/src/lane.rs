use bevy_reflect::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub struct LaneIndex(u64);

impl LaneIndex {
    #[must_use]
    pub const fn from_raw(index: u64) -> Self {
        Self(index)
    }

    #[must_use]
    pub const fn into_raw(self) -> u64 {
        self.0
    }
}
