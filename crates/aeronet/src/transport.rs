use bevy_app::prelude::*;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;
use bytes::Bytes;

use crate::{
    io::IoSet,
    message::{MessageKey, SendMode},
};

#[derive(Debug)]
pub struct TransportPlugin;

impl Plugin for TransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, TransportSet::Recv.after(IoSet::Recv))
            .configure_sets(PostUpdate, TransportSet::Send.before(IoSet::Send))
            .register_type::<MessageBuffers>()
            .register_type::<MessageMtu>();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    /// Decoding packets received from the IO layer into messages, draining
    /// [`PacketBuffers::recv`] and filling up [`MessageBuffers::recv`].
    ///
    /// [`PacketBuffers::recv`]: crate::io::PacketBuffers::recv
    Recv,
    /// Encoding messages into packets, draining [`MessageBuffers::send`] and
    /// filling up [`PacketBuffers::send`].
    ///
    /// [`PacketBuffers::send`]: crate::io::PacketBuffers::send
    Send,
}

#[derive(Debug, Clone, Default, Component, Reflect)]
#[reflect(Component)]
pub struct MessageBuffers {
    #[reflect(ignore)]
    pub recv: Vec<Bytes>,
    #[reflect(ignore)]
    pub send: Vec<(SendMode, Bytes)>,
    #[reflect(ignore)]
    pub acks: Vec<MessageKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deref, Component, Reflect)]
#[reflect(Component)]
pub struct MessageMtu(pub usize);
