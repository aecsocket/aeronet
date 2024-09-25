#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

use bevy_app::{prelude::*, PluginGroupBuilder};
pub use {
    aeronet_io::{self as io, connection, packet, server, AeronetIoPlugin, IoSet},
    aeronet_proto::{self as proto, message, AeronetProtoPlugin, ProtoSet},
    anyhow, bytes, ringbuf,
};

/// Adds the default networking plugins.
///
/// # Plugins
///
/// - [`AeronetIoPlugin`]
/// - [`AeronetProtoPlugin`]
#[derive(Debug)]
pub struct AeronetPlugins;

impl PluginGroup for AeronetPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(AeronetIoPlugin)
            .add(AeronetProtoPlugin)
    }
}
