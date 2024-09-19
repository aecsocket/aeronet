//! Core items for sessions and connection.
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
//! Connection and data transmission is handled by the [IO layer], however you
//! will typically be working with a higher-level layer such as the
//! [transport layer], or potentially some even more high-level networking
//! features.
//!
//! [IO layer]: crate::io
//! [transport layer]: crate::transport
//!
//! # Lifecycle
//!
//! Your IO layer implementation of choice (i.e. [`aeronet_channel`],
//! [`aeronet_webtransport`]) is responsible for spawning and despawning
//! sessions. See the implementation's documentation on how to configure and
//! spawn a session.
//!
//! Once [`Session`] is added to an entity, it is considered *connecting*, where
//! it is not ready to transmit packets yet. Once [`Connected`] is added to this
//! session, it is considered *connected*, and data transfer should be possible.
//! If the session fails to connect, or loses connection after successfully
//! connecting, [`Disconnected`] is emitted, and the session is despawned
//! immediately afterwards.
//!
//! [`aeronet_channel`]: https://docs.rs/aeronet_channel
//! [`aeronet_webtransport`]: https://docs.rs/aeronet_webtransport

use std::{fmt::Debug, net::SocketAddr};

use bevy_app::prelude::*;
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::{observer::TriggerTargets, prelude::*};
use bevy_hierarchy::DespawnRecursiveExt;
use bevy_reflect::prelude::*;
use web_time::Instant;

#[derive(Debug)]
pub(crate) struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Session>()
            .register_type::<Connected>()
            .observe(on_connected)
            .observe(on_disconnect)
            .observe(on_disconnected);
    }
}

/// Marker component for a [session].
///
/// If this component is present without [`Connected`], then this session is
/// still connecting.
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Session;

/// Bundle for spawning a [session].
///
/// In Bevy 0.15, this will be removed in favour of required components.
///
/// [session]: crate::session
#[derive(Default, Bundle)]
#[allow(missing_docs)]
pub struct SessionBundle {
    pub session: Session,
    pub packet_buffers: crate::io::PacketBuffers,
    pub packet_mtu: crate::io::PacketMtu,
    pub io_stats: crate::io::IoStats,
    pub message_buffers: crate::transport::MessageBuffers,
    pub message_mtu: crate::transport::MessageMtu,
    pub transport_stats: crate::transport::TransportStats,
}

/// Marker component for a [session] which is connected to its peer, and data
/// transmission should be possible.
///
/// Note that this is not a *guarantee* that the session is connected, since
/// networking operations such as working with OS sockets may fail at any time.
/// Packets may also be delayed or lost in transit.
///
/// To listen for when a session is connected, use [`Trigger<OnAdd, Connected>`].
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Connected;

/// Triggered when a user requests a [session] to gracefully disconnect from its
/// peer with a given reason.
///
/// The string provided is used as the disconnection reason in
/// [`DisconnectReason::User`].
///
/// Triggering this will guarantee that the session is disconnected and
/// despawned immediately, however the disconnection reason will be transmitted
/// to the peer as a best-effort attempt. If the IO layer does not support
/// disconnection reasons, or it cannot send your given reason (if e.g. it is
/// too long), the peer may not receive this disconnect reason.
///
/// If you have access to [`Commands`], consider using [`disconnect_sessions`]
/// as a convenient alternative to manually triggering an event.
///
/// [session]: crate::session
/// [`disconnect_sessions`]: DisconnectSessionsExt::disconnect_sessions
#[derive(Debug, Clone, PartialEq, Eq, Deref, DerefMut, Event)]
pub struct Disconnect(pub String);

/// Triggered when a [session] loses connection for any reason.
///
/// Immediately after this, the session will be despawned.
///
/// This must only be triggered by the IO layer when it detects that the peer
/// has disconnected from us, or when it detects a connection error.
///
/// If you want to get the concrete error type of the
/// [`DisconnectReason::Error`], use [`anyhow::Error::downcast_ref`].
///
/// [session]: crate::session
#[derive(Debug, Deref, DerefMut, Event)]
pub struct Disconnected(pub DisconnectReason<anyhow::Error>);

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

/// Disconnect reason to use when an IO layer is dropped.
///
/// IO layer implementations may use this as a default disconnection reason when
/// their IO component is dropped, instead of being explicitly disconnected via
/// [`Disconnect`].
pub const DROP_DISCONNECT_REASON: &str = "dropped";

/// Provides [`DisconnectSessionsExt::disconnect_sessions`]
pub trait DisconnectSessionsExt {
    /// Requests [sessions] to gracefully disconnect from their peers with a
    /// given reason.
    ///
    /// See [`Disconnect`].
    ///
    /// # Examples
    ///
    /// ```
    /// use bevy_ecs::prelude::*;
    /// use aeronet::session::DisconnectSessionsExt;
    ///
    /// # fn run(mut commands: Commands, session: Entity, session1: Entity, session2: Entity) {
    /// // disconnect a single session
    /// commands.disconnect_sessions("show's over, go home", session);
    ///
    /// // disconnect multiple sessions at once
    /// commands.disconnect_sessions("show's over everyone, go home", [session1, session2]);
    /// # }
    /// ```
    ///
    /// [sessions]: crate::session
    fn disconnect_sessions(&mut self, reason: impl Into<String>, targets: impl TriggerTargets);
}

impl DisconnectSessionsExt for Commands<'_, '_> {
    fn disconnect_sessions(&mut self, reason: impl Into<String>, targets: impl TriggerTargets) {
        self.trigger_targets(Disconnect(reason.into()), targets);
    }
}

/// Instant at which a [session] connected to its peer.
///
/// This is automatically added to the session when [`Connected`] is added.
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
pub struct ConnectedAt(pub Instant);

/// Local socket address that this entity uses for connections.
///
/// Sessions or servers which use a network will use an OS socket for
/// communication. This component stores the local [`SocketAddr`] of this
/// socket.
///
/// This component may not be present in environments where there is no access
/// to OS sockets (i.e. WASM).
///
/// To access the remote socket address of a session, see [`RemoteAddr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Component)]
pub struct LocalAddr(pub SocketAddr);

/// Remote socket address that this [session] is connected to.
///
/// Sessions which use a network will use an OS socket for communication. This
/// component stores the [`SocketAddr`] of the peer, which this session is
/// connected to.
///
/// This component may not be present in environments where there is no access
/// to OS sockets (i.e. WASM).
///
/// To access the local socket address of a session, see [`LocalAddr`].
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Component)]
pub struct RemoteAddr(pub SocketAddr);

fn on_connected(trigger: Trigger<OnAdd, Connected>, mut commands: Commands) {
    let session = trigger.entity();

    commands.entity(session).insert(ConnectedAt(Instant::now()));
}

fn on_disconnect(trigger: Trigger<Disconnect>, mut commands: Commands) {
    let session = trigger.entity();
    let reason = DisconnectReason::User(trigger.event().0.clone());

    commands.trigger_targets(Disconnected(reason), session);
}

fn on_disconnected(trigger: Trigger<Disconnected>, mut commands: Commands) {
    let session = trigger.entity();

    commands.entity(session).despawn_recursive();
}
