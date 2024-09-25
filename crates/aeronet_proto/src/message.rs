use {
    bevy_app::prelude::*,
    bevy_derive::{Deref, DerefMut},
    bevy_ecs::prelude::*,
    bevy_reflect::prelude::*,
    derive_more::{Add, AddAssign, Sub, SubAssign},
    octs::Bytes,
    std::{num::Saturating, time::Duration},
};

#[derive(Debug)]
pub(crate) struct MessagePlugin;

impl Plugin for MessagePlugin {
    fn build(&self, app: &mut App) {}
}

#[derive(Debug, Clone, Default, Component)]
pub struct MessageBuffers {
    pub recv: Vec<Bytes>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, Component, Reflect,
)]
#[reflect(Component)]
pub struct MessageMtu(pub usize);

#[derive(Debug, Clone, Copy)]
#[doc(alias = "ping")]
#[doc(alias = "latency")]
pub struct MessageRtt {}

#[derive(Debug, Clone, Copy, Default, Component, Reflect, Add, AddAssign, Sub, SubAssign)]
#[reflect(Component)]
pub struct MessageStats {
    pub msgs_recv: Saturating<usize>,
    pub msgs_sent: Saturating<usize>,
    pub acks_recv: Saturating<usize>,
}
