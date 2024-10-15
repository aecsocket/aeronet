#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod lane;
pub mod message;

pub use {aeronet_io as io, octs};
use {
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet, system::EntityCommand},
    lane::LaneKind,
    message::MessageBuffers,
};

#[derive(Debug)]
pub struct AeronetTransportPlugin;

impl Plugin for AeronetTransportPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, TransportSet::Poll)
            .configure_sets(PostUpdate, TransportSet::Flush)
            .add_plugins(message::MessagePlugin)
            .observe(on_transport_added);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    Poll,
    Flush,
}

#[derive(Debug, Component)]
pub struct Transport {
    recv_lanes: Box<()>,
    send_lanes: Box<()>,
}

impl Transport {
    #[must_use]
    pub fn new(
        recv_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
        send_lanes: impl IntoIterator<Item = impl Into<LaneKind>>,
    ) -> Self {
        Self {
            recv_lanes: Box::new(()),
            send_lanes: Box::new(()),
        }
    }
}

// TODO: required components
fn on_transport_added(trigger: Trigger<OnAdd, Transport>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert(MessageBuffers::default());
}
