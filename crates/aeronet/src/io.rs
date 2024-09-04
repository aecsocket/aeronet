use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;

#[derive(Debug, Clone, Default, Component, Reflect)]
#[reflect(Component)]
pub struct PacketBuffers {
    /// Buffer of packets that will be received on the next
    /// [`SessionSet::Recv`].
    #[reflect(ignore)]
    pub recv: Vec<Bytes>,
    /// Buffer of packets that will be sent out on the next
    /// [`SessionSet::Send`].
    #[reflect(ignore)]
    pub send: Vec<Bytes>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deref, DerefMut, Component, Reflect,
)]
#[reflect(Component)]
pub struct PacketMtu(pub usize);
