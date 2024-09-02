//! Client/server-independent items.

use std::{fmt::Debug, hash::Hash};

use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use bytes::Bytes;

#[derive(Debug)]
pub struct TransportPlugin;

impl Plugin for TransportPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Connected>()
            .register_type::<Disconnect>()
            .register_type::<RecvBuffer>()
            .register_type::<SendBuffer>()
            .configure_sets(PreUpdate, TransportSet::Recv)
            .configure_sets(PostUpdate, TransportSet::Send);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Reflect)]
pub enum SendMode {
    UnreliableUnordered,
    UnreliableSequenced,
    ReliableUnordered,
    ReliableOrdered(usize),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
pub struct Connected;

#[derive(Debug, Clone, Default, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
pub struct RecvBuffer(#[reflect(ignore)] pub Vec<Bytes>);

#[derive(Debug, Clone, Default, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
pub struct AckBuffer<M>(pub Vec<M>);

#[derive(Debug, Clone, Default, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
pub struct SendBuffer(#[reflect(ignore)] pub Vec<(SendMode, Bytes)>);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    Recv,
    Send,
}

#[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
pub struct Disconnect {
    pub reason: String,
}

#[derive(Debug)]
pub enum DisconnectReason {
    Local(String),
    Remote(String),
    Error(anyhow::Error),
}

pub trait DisconnectExt {
    fn disconnect(&mut self, client: Entity, reason: impl Into<String>);
}

impl DisconnectExt for Commands<'_, '_> {
    fn disconnect(&mut self, client: Entity, reason: impl Into<String>) {
        self.entity(client).insert(Disconnect {
            reason: reason.into(),
        });
    }
}

/// Disconnect reason that may be used when a client or server is dropped.
///
/// When a client is dropped, it must disconnect itself from its server.
/// Similarly, when a server is dropped, it must disconnect all of its currently
/// connected clients. For both of these operations, a string reason is
/// required. Implementations may use this string as a default disconnect
/// reason.
pub const DROP_DISCONNECT_REASON: &str = "dropped";
