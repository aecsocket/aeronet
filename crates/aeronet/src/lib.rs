#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

use bevy_app::{PluginGroupBuilder, prelude::*};
pub use {aeronet_io as io, aeronet_transport as transport};

/// Adds the default networking plugins.
///
/// # Plugins
///
/// - [`io::AeronetIoPlugin`]
/// - [`transport::AeronetTransportPlugin`]
#[derive(Debug)]
pub struct AeronetPlugins;

impl PluginGroup for AeronetPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(io::AeronetIoPlugin)
            .add(transport::AeronetTransportPlugin)
    }
}
