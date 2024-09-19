#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub use {anyhow, bytes, ringbuf};

pub mod io;
pub mod message;
pub mod session;
// pub mod stats;
pub mod log;
pub mod naive_transport;
pub mod rtt;
pub mod transport;
// #[cfg(feature = "visualizer")]
// pub mod visualizer;

use bevy_app::{prelude::*, PluginGroupBuilder};

/// Default plugin group for the core `aeronet` logic.
///
/// # Plugins
///
/// - (internal plugins)
/// - [`log::SessionLogPlugin`]
#[derive(Debug)]
pub struct AeronetPlugins;

impl PluginGroup for AeronetPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(session::SessionPlugin)
            .add(io::IoPlugin)
            .add(transport::TransportPlugin)
            .add(log::SessionLogPlugin)
    }
}
