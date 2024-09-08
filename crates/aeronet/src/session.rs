//! Core items for sessions and session connections.
//!
//! # Session
//!
//! A session is an [`Entity`] which can be used to transfer sequences of bytes
//! over to the other side of a connection - to the session's peer. The peer may
//! be located on a different machine and connected over a network such as the
//! internet, and may use any protocol for communication. The peer can also be
//! located on the same machine, or even within the same app - these details are
//! deliberately left unspecified and abstracted away.
//!
//! The session API is agnostic to the networking model used: it can be used to
//! represent a client-server, peer-to-peer, or any other kind of network
//! topology. The only constraint is that one session talks to one and only one
//! peer for its lifetime, however you can have multiple sessions within the
//! same world. These different sessions may even be communicating over
//! different protocols, such as raw UDP datagrams alongside Steam networking
//! sockets, so that you can e.g. support crossplay between different platforms.
//!
//! At the lowest level, sessions operate on packets. Sessions do not provide
//! any guarantees of packet delivery, so they may be delayed, lost, or even
//! duplicated. This is because when working with a network such as the internet
//! we have effectively zero guarantees - network conditions are constantly
//! changing, paths may change, or the computer might suddenly be disconnected
//! from the network. Sessions pass down packets to, and receive packets from,
//! the [IO layer].
//!
//! However, you will typically want guarantees when working with networking
//! code, such as reliability or ordering (see [`SendMode`]). This is handled
//! by the [transport layer].
//!
//! [IO layer]: crate::io
//! [`SendMode`]: crate::message::SendMode
//! [transport layer]: crate::transport
//!
//! # Lifecycle
//!
//! Your IO layer implementation of choice (i.e. [`aeronet_channel`],
//! [`aeronet_webtransport`]) is responsible for spawning and despawning
//! sessions. See the implementation's documentation on how to configure and
//! spawn a session.
//!
//! Once a session has been spawned, [`SessionConnecting`] is emitted. The
//! session may not be ready for transmitting packets immediately, and may go
//! through a connection process. If this is successful, [`SessionConnected`]
//! is emitted and the session is initialized with components for data transport
//! such as [`PacketBuffers`]. If the session fails to connect, or is
//! disconnected after establishing a successful connection,
//! [`SessionDisconnected`] is emitted, and the session is despawned on the
//! update afterwards.
//!
//! [`aeronet_channel`]: https://docs.rs/aeronet_channel
//! [`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport
//! [`PacketBuffers`]: crate::io::PacketBuffers

use std::{any::type_name, fmt::Debug};

use bevy_app::prelude::*;
use bevy_core::Name;
use bevy_derive::Deref;
use bevy_ecs::prelude::*;
use bevy_hierarchy::DespawnRecursiveExt;
use bevy_reflect::prelude::*;
use tracing::{error, info, warn};
use web_time::Instant;

use crate::{stats::ConnectedAt, util::display_name};

#[derive(Debug)]
pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Session>()
            .register_type::<Connected>()
            .observe(connecting)
            .observe(connected)
            .observe(disconnect)
            .observe(on_disconnected);
    }
}

/// Marker component for an [`Entity`] which is a [session].
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Session;

#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Connected;

#[derive(Debug, Deref, Component)]
#[component(storage = "SparseSet")]
pub struct Disconnected(pub DisconnectReason<anyhow::Error>);

#[derive(Debug, Clone, PartialEq, Eq, Component, Reflect)]
#[reflect(Component)]
#[component(storage = "SparseSet")]
pub struct Disconnect(pub String);

/// Why a [session] was disconnected from its peer.
///
/// [session]: crate::session
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason<E> {
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
    /// [reliably]: crate::message::SendReliability::Reliable
    Error(E),
}

impl<E> DisconnectReason<E> {
    /// Maps a [`DisconnectReason<E>`] to a [`DisconnectReason<F>`] by mapping
    /// the [`DisconnectReason::Error`] variant.
    pub fn map_err<F>(self, f: impl FnOnce(E) -> F) -> DisconnectReason<F> {
        match self {
            Self::User(reason) => DisconnectReason::User(reason),
            Self::Peer(reason) => DisconnectReason::Peer(reason),
            Self::Error(err) => DisconnectReason::Error(f(err)),
        }
    }
}

impl<E> From<E> for DisconnectReason<E> {
    fn from(value: E) -> Self {
        Self::Error(value)
    }
}

pub const DROP_DISCONNECT_REASON: &str = "dropped";

pub trait DisconnectSessionExt {
    fn disconnect_session(&mut self, session: Entity, reason: impl Into<String>);
}

impl DisconnectSessionExt for Commands<'_, '_> {
    fn disconnect_session(&mut self, session: Entity, reason: impl Into<String>) {
        self.entity(session).insert(Disconnect(reason.into()));
    }
}

fn connecting(trigger: Trigger<OnAdd, Session>, names: Query<Option<&Name>>) {
    let session = trigger.entity();
    let name = names
        .get(session)
        .expect("`session` should exist because we are adding a component to it");

    let display_name = display_name(session, name);
    info!("Session {display_name} connecting");
}

fn connected(
    trigger: Trigger<OnAdd, Connected>,
    mut commands: Commands,
    names: Query<Option<&Name>>,
    with_session: Query<(), With<Session>>,
) {
    let session = trigger.entity();
    let name = names
        .get(session)
        .expect("`session` should exist because we are adding a component to it");

    let display_name = display_name(session, name);
    info!("Session {display_name} connected");

    commands.entity(session).insert(ConnectedAt(Instant::now()));

    if with_session.get(session).is_err() {
        error!(
            "Session {display_name} does not have `{}`",
            type_name::<Session>()
        );
    }
}

fn disconnect(
    trigger: Trigger<OnAdd, Disconnect>,
    disconnects: Query<&Disconnect>,
    mut commands: Commands,
) {
    let session = trigger.entity();
    let Disconnect(reason) = disconnects
        .get(session)
        .expect("`session` should exist because we are adding a component to it");

    commands
        .entity(session)
        .insert(Disconnected(DisconnectReason::User(reason.clone())));
}

fn on_disconnected(
    trigger: Trigger<OnAdd, Disconnected>,
    names: Query<(&Disconnected, Option<&Name>)>,
    with_session: Query<(), With<Session>>,
    mut commands: Commands,
) {
    let session = trigger.entity();
    let (Disconnected(dc_reason), name) = names
        .get(session)
        .expect("`session` should exist because we are adding a component to it");

    let display_name = display_name(session, name);
    match dc_reason {
        DisconnectReason::User(reason) => {
            info!("Session {display_name} disconnected by user: {reason}");
        }
        DisconnectReason::Peer(reason) => {
            info!("Session {display_name} disconnected by peer: {reason}");
        }
        DisconnectReason::Error(err) => {
            warn!("Session {display_name} disconnected due to error: {err:#}");
        }
    }

    if with_session.get(session).is_err() {
        error!(
            "Session {display_name} does not have `{}`",
            type_name::<Session>(),
        );
    }

    commands.entity(session).despawn_recursive();
}
