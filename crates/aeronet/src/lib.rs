#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

use bevy_app::{prelude::*, PluginGroupBuilder};
pub use {
    aeronet_io::{self as io, anyhow, bytes, connection, packet, ringbuf, server, web_time},
    aeronet_transport::{self as transport, message, octs},
};

/// Adds the default networking plugins.
///
/// # Plugins
///
/// - [`AeronetIoPlugin`]
/// - [`AeronetTransportPlugin`]
#[derive(Debug)]
pub struct AeronetPlugins;

impl PluginGroup for AeronetPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(io::AeronetIoPlugin)
            .add(transport::AeronetTransportPlugin)
    }
}
