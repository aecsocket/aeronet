use std::{net::SocketAddr, num::Saturating, time::Duration};

use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct Rtt {
    latest: Duration,
    smoothed: Duration,
    jitter: Duration,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct SessionStats {
    pub msgs_sent: Saturating<usize>,
    pub msgs_recv: Saturating<usize>,
    pub bytes_sent: Saturating<usize>,
    pub bytes_recv: Saturating<usize>,
    pub packets_sent: Saturating<usize>,
    pub packets_recv: Saturating<usize>,
    pub acks_recv: Saturating<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Component)]
pub struct LocalAddr(pub SocketAddr);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Component)]
pub struct RemoteAddr(pub SocketAddr);
