#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]
#![allow(missing_docs, dead_code)] // TODO

pub mod lane;
pub mod message;
pub mod packet;

#[cfg(feature = "stats")]
pub mod stats;

#[cfg(feature = "visualizer")]
pub mod visualizer;

pub use {aeronet_io as io, octs};
use {
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, schedule::SystemSet},
    lane::LaneKind,
    message::{MessageBuffers, MessageRtt, MessageStats},
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
    pub max_memory_usage: usize,
    pub send_bytes_per_sec: usize,
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
            max_memory_usage: 4 * 1024 * 1024,
            send_bytes_per_sec: usize::MAX,
        }
    }
}

// TODO: required components
fn on_transport_added(trigger: Trigger<OnAdd, Transport>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert((
        MessageBuffers::default(),
        MessageRtt {},
        MessageStats::default(),
    ));
}
