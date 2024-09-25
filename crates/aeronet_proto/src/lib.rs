#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod lane;
pub mod message;

use bevy_app::prelude::*;
use bevy_ecs::schedule::SystemSet;
pub use {aeronet_io, octs};

#[derive(Debug)]
pub struct AeronetProtoPlugin;

impl Plugin for AeronetProtoPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, ProtoSet::Poll)
            .configure_sets(PostUpdate, ProtoSet::Flush)
            .add_plugins(message::MessagePlugin);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ProtoSet {
    Poll,
    Flush,
}
