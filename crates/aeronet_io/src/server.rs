//! Logic for a [`Server`] entity which accepts and manages client [`Session`]s
//! connecting to it.

use {
    crate::connection::Disconnect,
    bevy_app::prelude::*,
    bevy_derive::{Deref, DerefMut},
    bevy_ecs::prelude::*,
    bevy_hierarchy::{Children, DespawnRecursiveExt},
    bevy_reflect::prelude::*,
    tracing::debug,
    web_time::Instant,
};

#[derive(Debug)]
pub(crate) struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<Server>()
            .register_type::<Opened>()
            .observe(on_opening)
            .observe(on_opened)
            .observe(on_close)
            .observe(on_closed);
    }
}

#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Server;

/// Marker component for a [`Server`] which is currently attempting to receive
/// client connections and spawn [`Session`]s.
///
/// To listen for when a server is opened, add an observer listening for
/// [`Trigger<OnAdd, Opened>`].
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
#[reflect(Component)]
pub struct Opened;

/// Triggered when a user requests a [`Server`] to gracefully shut down and
/// disconnect all of its connected clients.
///
/// Triggering this will guarantee that the server is closed and despawned
/// immediately, and we attempt to disconnect all connected clients with the
/// given reason, but note that this is only best-effort (see [`Disconnect`]).
///
/// # Examples
///
/// ```
/// use {aeronet_io::server::Close, bevy_ecs::prelude::*};
///
/// # fn run(mut commands: Commands, server: Entity, server1: Entity, server2: Entity) {
/// // close a single server
/// commands.trigger_targets(Close::new("show's over, go home"), server);
///
/// // disconnect multiple sessions at once
/// commands.trigger_targets(Close::new("show's over everyone, go home"), [server1, server2]);
/// # }
/// ```
///
/// [`Disconnect`]: crate::connection::Disconnect
#[derive(Debug, Clone, PartialEq, Eq, Event)]
pub struct Close {
    /// User-provided closing reason.
    ///
    /// Will be used as the disconnection reason when disconnecting sessions
    /// connected to this server.
    pub reason: String,
}

impl Close {
    /// Creates a new [`Close`] with the given reason.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Deref, DerefMut, Event)]
pub struct Closed {
    pub reason: CloseReason<anyhow::Error>,
}

/// Why a [`Server`] was closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason<E> {
    /// Server was closed by the user on our side, with a provided reason.
    ///
    /// Connected clients will be disconnected with the same reason.
    User(String),
    /// Server encountered a fatal connection error, and can no longer accept
    /// new clients or update existing clients.
    ///
    /// Typically, this will only occur when attempting to start the server, and
    /// will not occur once the server has fully opened. If a client enters some
    /// sort of illegal state or performs an illegal action, only that single
    /// client will be disconnected instead of the entire server being torn
    /// down.
    Error(E),
}

impl<E> CloseReason<E> {
    /// Maps a [`CloseReason<E>`] to a [`CloseReason<F>`] by mapping the
    /// [`CloseReason::Error`] variant.
    pub fn map_err<F>(self, f: impl FnOnce(E) -> F) -> CloseReason<F> {
        match self {
            Self::User(reason) => CloseReason::User(reason),
            Self::Error(err) => CloseReason::Error(f(err)),
        }
    }
}

impl<E> From<E> for CloseReason<E> {
    fn from(value: E) -> Self {
        Self::Error(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, DerefMut, Component, Reflect)]
#[reflect(Component)]
pub struct OpenedAt(pub Instant);

fn on_opening(trigger: Trigger<OnAdd, Server>) {
    let server = trigger.entity();
    debug!("{server} opening");
}

fn on_opened(trigger: Trigger<OnAdd, Opened>, mut commands: Commands) {
    let server = trigger.entity();
    commands.entity(server).insert(OpenedAt(Instant::now()));
    debug!("{server} opened");
}

fn on_close(trigger: Trigger<Close>, mut commands: Commands) {
    let server = trigger.entity();
    let reason = CloseReason::User(trigger.event().reason.clone());
    commands.trigger_targets(Closed { reason }, server);
}

fn on_closed(trigger: Trigger<Closed>, children: Query<&Children>, mut commands: Commands) {
    let server = trigger.entity();
    let children = children
        .get(server)
        .map(|children| children.iter().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    match &**trigger.event() {
        CloseReason::User(reason) => {
            debug!("{server} closed by user: {reason}");
            commands.trigger_targets(Disconnect::new(reason), children);
        }
        CloseReason::Error(err) => {
            debug!("{server} closed due to error: {err:#}");
        }
    }

    commands.entity(server).despawn_recursive();
}
