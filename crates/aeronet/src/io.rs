use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;

#[derive(Debug)]
pub struct IoPlugin;

impl Plugin for IoPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, IoSet::Recv)
            .configure_sets(PostUpdate, IoSet::Send)
            .register_type::<PacketBuffers>()
            .register_type::<PacketMtu>();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum IoSet {
    /// Receiving packets from the IO layer, filling up [`PacketBuffers::recv`].
    Recv,
    /// Sending packets to the IO layer, draining [`PacketBuffers::send`].
    Send,
}

#[derive(Debug, Clone, Default, Component, Reflect)]
#[reflect(Component)]
pub struct PacketBuffers {
    /// Buffer of packets received from the IO layer during [`IoSet::Recv`].
    #[reflect(ignore)]
    pub recv: Vec<Bytes>,
    /// Buffer of packets that will be drained and sent out to the IO layer
    /// during [`IoSet::Send`].
    #[reflect(ignore)]
    pub send: Vec<Bytes>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deref, Component, Reflect)]
#[reflect(Component)]
pub struct PacketMtu(pub usize);
