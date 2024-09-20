//! Logic for connection and disconnection of a [`Session`].

use {
    bevy_app::prelude::*,
    bevy_derive::{Deref, DerefMut},
    bevy_ecs::prelude::*,
    bevy_hierarchy::DespawnRecursiveExt,
    bevy_reflect::prelude::*,
    std::{fmt::Debug, net::SocketAddr},
    tracing::debug,
    web_time::Instant,
};

#[derive(Debug)]
pub(crate) struct ConnectionPlugin;

impl Plugin for ConnectionPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Session>()
            .register_type::<Connected>()
            .observe(on_connecting)
            .observe(on_connected)
            .observe(on_disconnect)
            .observe(on_disconnected);
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

/// Marker component for a [`Session`] which is connected to its peer, and data
/// transmission should be possible.
///
/// Note that this is not a guarantee that the session is connected, since
/// networking operations such as working with OS sockets may fail at any time.
///
/// To listen for when a session is connected, add an observer listening for
/// [`Trigger<OnAdd, Connected>`].
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Connected;

/// Triggered when a user requests a [`Session`] to gracefully disconnect from
/// its peer with a given reason.
///
/// Triggering this will guarantee that the session is disconnected and
/// despawned immediately, however the disconnection reason will be transmitted
/// to the peer as a best-effort attempt. If the IO layer implementation does
/// not support disconnection reasons, or it cannot send your given reason (if
/// e.g. it is too long), the peer may not receive this disconnect reason.
///
/// # Examples
///
/// ```
/// use {aeronet_io::connection::Disconnect, bevy_ecs::prelude::*};
///
/// # fn run(mut commands: Commands, session: Entity, session1: Entity, session2: Entity) {
/// // disconnect a single session
/// commands.trigger_targets(Disconnect::new("show's over, go home"), session);
///
/// // disconnect multiple sessions at once
/// commands.trigger_targets(Disconnect::new("show's over everyone, go home"), [session1, session2]);
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Event)]
pub struct Disconnect {
    /// User-provided disconnection reason.
    ///
    /// Will be used as the reason in [`DisconnectReason::User`].
    pub reason: String,
}

impl Disconnect {
    /// Creates a [`Disconnect`] with the given reason.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

/// Triggered when a [`Session`] loses connection for any reason.
///
/// Immediately after this, the session will be despawned.
///
/// This must only be triggered by the IO layer when it detects that the peer
/// has disconnected from us, or when it detects a connection error.
///
/// If you want to get the concrete error type of the
/// [`DisconnectReason::Error`], use [`anyhow::Error::downcast_ref`].
#[derive(Debug, Deref, DerefMut, Event)]
pub struct Disconnected {
    /// Why the session was disconnected.
    pub reason: DisconnectReason<anyhow::Error>,
}

/// Why a [`Session`] was disconnected from its peer.
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
    /// - the peer pretending like there are network errors to discreetly force
    ///   us to disconnect
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

/// Disconnect reason to use when an IO layer component is dropped.
///
/// IO layer implementations may use this as a default disconnection reason when
/// their IO component is dropped, instead of being explicitly disconnected via
/// [`Disconnect`].
pub const DROP_DISCONNECT_REASON: &str = "dropped";

/// Instant at which a [`Session`] connected to its peer.
///
/// This is automatically added to the session when [`Connected`] is added.
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

/// Remote socket address that this [`Session`] is connected to.
///
/// Sessions which use a network will use an OS socket for communication. This
/// component stores the [`SocketAddr`] of the peer, which this session is
/// connected to.
///
/// This component may not be present in environments where there is no access
/// to OS sockets (i.e. WASM).
///
/// To access the local socket address of a session, see [`LocalAddr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Component)]
pub struct RemoteAddr(pub SocketAddr);

fn on_connecting(trigger: Trigger<OnAdd, Session>) {
    let session = trigger.entity();
    debug!("{session} connecting");
}

fn on_connected(trigger: Trigger<OnAdd, Connected>, mut commands: Commands) {
    let session = trigger.entity();
    debug!("{session} connected");

    commands.entity(session).insert(ConnectedAt(Instant::now()));
}

fn on_disconnect(trigger: Trigger<Disconnect>, mut commands: Commands) {
    let session = trigger.entity();
    let reason = DisconnectReason::User(trigger.event().reason.clone());
    commands.trigger_targets(Disconnected { reason }, session);
}

fn on_disconnected(trigger: Trigger<Disconnected>, mut commands: Commands) {
    let session = trigger.entity();
    match &**trigger.event() {
        DisconnectReason::User(reason) => {
            debug!("{session} disconnected by user: {reason}");
        }
        DisconnectReason::Peer(reason) => {
            debug!("{session} disconnected by user: {reason}");
        }
        DisconnectReason::Error(err) => {
            debug!("{session} disconnected due to error: {err:#}");
        }
    }

    commands.entity(session).despawn_recursive();
}
