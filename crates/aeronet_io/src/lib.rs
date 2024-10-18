#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod connection;
pub mod packet;
pub mod server;
mod util;

pub use {anyhow, bytes, ringbuf, web_time};
use {bevy_app::prelude::*, bevy_ecs::prelude::*};

/// Sets up the IO layer functionality.
#[derive(Debug)]
pub struct AeronetIoPlugin;

impl Plugin for AeronetIoPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, IoSet::Poll)
            .configure_sets(PostUpdate, IoSet::Flush)
            .add_plugins((
                connection::ConnectionPlugin,
                packet::PacketPlugin,
                server::ServerPlugin,
            ));
    }
}

/// Set for scheduling IO layer systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum IoSet {
    /// Progressing the connection, handling disconnects, and receiving packets.
    Poll,
    /// Sending buffered packets.
    Flush,
}
