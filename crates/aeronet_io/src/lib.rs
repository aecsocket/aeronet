#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![doc = include_str!("../README.md")]

mod connection;
mod packet;
mod server;
mod util;

pub use {
    self::{connection::*, packet::*},
    anyhow, bytes, ringbuf,
};
use {bevy_app::prelude::*, bevy_ecs::prelude::*, bevy_reflect::prelude::*};

/// Sets up the IO layer functionality.
#[derive(Debug)]
pub struct AeronetIoPlugin;

impl Plugin for AeronetIoPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Session>()
            .configure_sets(PreUpdate, IoSet::Poll)
            .configure_sets(PostUpdate, IoSet::Flush)
            .add_plugins((connection::ConnectionPlugin, packet::PacketPlugin));
    }
}

/// Marker component for an [`Entity`] used to transfer sequences of bytes over
/// a connection, potentially over a network.
///
/// A session can send data over to the other side of its connection - to its
/// peer. The peer may be located on a different machine, on the same machine as
/// this session, or even within the same app.
///
/// The session API is agnostic to the networking model used: it can be used to
/// represent a client-server, peer-to-peer, or any other kind of network
/// topology. The only constraint is that one session talks to one and only one
/// peer for its lifetime, however you can have multiple sessions within the
/// same world. These different sessions may even be communicating over
/// different protocols, such as raw UDP datagrams alongside Steam networking
/// sockets, so that you can e.g. support crossplay between different platforms.
///
/// You should not add this component to entities yourself - your chosen IO
/// layer implementation is responsible for this. Once added, the session is
/// considered connecting, but it may not be connected yet, and therefore you
/// cannot send data across this session. Once [`Connected`] is added, you can
/// start sending and receiving data.
///
/// If the session fails to connect, or loses connection after successfully
/// connecting (this may be a graceful disconnect or a connection error),
/// [`Disconnected`] is [triggered][trigger] on the session entity, and the
/// session is despawned immediately afterwards. You may also [trigger] your own
/// disconnection with a string reason by triggering [`Disconnect`].
///
/// [trigger]: Trigger
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Session;

/// Set for scheduling IO layer systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum IoSet {
    /// Progressing the connection, handling disconnects, and receiving packets.
    Poll,
    /// Sending buffered packets.
    Flush,
}
