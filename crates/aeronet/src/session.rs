use std::{fmt::Debug, hash::Hash};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;

#[derive(Debug)]
pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(PreUpdate, SessionSet::Recv)
            .configure_sets(PostUpdate, SessionSet::Send)
            .register_type::<crate::io::PacketBuffers>()
            .register_type::<Disconnect>();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum SessionSet {
    Recv,
    Send,
}

/// Why a session was disconnected from its peer.
#[derive(Debug)]
pub enum DisconnectReason {
    /// Session was disconnected by the user on our side, with a provided
    /// reason.
    ///
    /// On the peer, this will be interpreted as a [`DisconnectReason::Peer`]
    /// with the same reason.
    User(String),
    /// Session was disconnected by the peer on the other side, with a provided
    /// reason.
    ///
    /// On the peer, this will be interpreted as a [`DisconnectReason::User`]
    /// with the same reason.
    Peer(String),
    /// Session encountered a fatal connection error, and communication between
    /// this session and the peer is no longer possible.
    ///
    /// This may be caused by:
    /// - a bad network condition for a prolonged period of time
    /// - a malicious or malfunctioning peer
    /// - failing to send a message [reliably]
    /// - the peer pretending like there are network errors to discreetly
    ///   force us to disconnect
    /// - ..and more
    ///
    /// If you need the actual typed error value, use
    /// [`anyhow::Error::downcast`].
    ///
    /// [reliably]: crate::message::SendReliability::Reliable
    Error(anyhow::Error),
}

/// Component for sessions which should be disconnected from their peer the next
/// time (TODO what?) runs.
///
/// On our side, a disconnection due to this component being present will be
/// interpreted as a [`DisconnectReason::User`].
///
/// On the peer's side, this disconnection will be interpreted as a
/// [`DisconnectReason::Peer`].
///
/// You can disconnect a session even if it doesn't have [`ConnectedSession`],
/// if you need to e.g. disconnect a session that's still in the process of
/// connecting.
#[derive(Debug, Clone, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
#[component(storage = "SparseSet")]
pub struct Disconnect {
    /// User-specified message on why this session should be disconnected.
    ///
    /// This will be available in the [`DisconnectReason`].
    pub reason: String,
}

/// Extension trait on [`Commands`] providing [`DisconnectExt::disconnect`].
pub trait DisconnectExt {
    fn disconnect(&mut self, session: Entity, reason: impl Into<String>);
}

impl DisconnectExt for Commands<'_, '_> {
    fn disconnect(&mut self, session: Entity, reason: impl Into<String>) {
        disconnect(self, session, reason.into());
    }
}

fn disconnect(this: &mut Commands, session: Entity, reason: String) {
    this.entity(session).insert(Disconnect { reason });
}
