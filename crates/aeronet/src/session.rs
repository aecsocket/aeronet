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

use std::fmt::Debug;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_reflect::prelude::*;

#[derive(Debug)]
pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<DisconnectSession>();
    }
}

/// Marker component for an [`Entity`] which is a [session].
///
/// [session]: crate::session
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Session;

/// A [session] has been spawned and is now connecting to its peer.
///
/// To listen for this event, use an [`Observer`].
///
/// [session]: crate::session
#[derive(Debug, Event)]
pub struct SessionConnecting;

/// A [session] has finalized the connection to its peer and will now send and
/// receive packets.
///
/// To listen for this event, use an [`Observer`].
///
/// [session]: crate::session
#[derive(Debug, Event)]
pub struct SessionConnected {
    pub session: Entity,
}

/// A [session] has lost connection to its peer and will be despawned.
///
/// To listen for this event, use an [`Observer`].
///
/// [session]: crate::session
#[derive(Debug, Event)]
pub struct SessionDisconnected {
    /// Why this session was disconnected from its peer.
    ///
    /// If you need access to the concrete error type, use
    /// [`anyhow::Error::downcast_ref`].
    pub reason: DisconnectReason<anyhow::Error>,
}

// todo how can we trigger a disconnect?
#[derive(Debug, Clone, PartialEq, Eq, Event, Reflect)]
#[reflect(Component)]
pub struct DisconnectSession {
    /// User-specified message on why this session should be disconnected.
    ///
    /// This will be available in the [`DisconnectReason`].
    pub reason: String,
}

/// Why a [session] was disconnected from its peer.
///
/// [session]: crate::session
#[derive(Debug)]
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
