#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod lane;
pub mod message;

pub use {aeronet_io, octs};
use {
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet},
    bevy_reflect::prelude::*,
    message::MessageBuffers,
};

#[derive(Debug)]
pub struct AeronetProtoPlugin;

impl Plugin for AeronetProtoPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, ProtoSet::Poll)
            .configure_sets(PostUpdate, ProtoSet::Flush)
            .add_plugins(message::MessagePlugin)
            .observe(on_transport_added);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ProtoSet {
    Poll,
    Flush,
}

#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct ProtoTransport;

// TODO: required components
fn on_transport_added(trigger: Trigger<OnAdd, ProtoTransport>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert(MessageBuffers::default());
}
