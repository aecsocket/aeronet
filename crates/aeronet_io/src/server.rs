//! Logic for a [`Server`] entity which accepts and manages client [`Session`]s
//! connecting to it.
//!
//! Although the core logic of this crate is topology-agnostic, this module
//! provides an abstraction over the "server" part of the very common
//! server/client topology.
//!
//! [`Session`]: crate::Session

use {
    crate::connection::Disconnect,
    alloc::{string::String, vec::Vec},
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    bevy_platform_support::time::Instant,
    bevy_reflect::prelude::*,
    log::debug,
};

pub(crate) struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ServerEndpoint>()
            .register_type::<Server>()
            .add_observer(on_opening)
            .add_observer(on_opened)
            .add_observer(on_close)
            .add_observer(on_closed);
    }
}

/// Represents an [`Entity`] which may be preparing to, or has already started,
/// listening for client connections.
///
/// - If a server entity only has [`ServerEndpoint`], it is still opening.
/// - If a server entity has [`ServerEndpoint`] and [`Server`], it has
///   successfully opened.
#[derive(Debug, Clone, Copy, Default, Component, Reflect)]
pub struct ServerEndpoint;

/// Represents an [`Entity`] which listens for client connections, and spawns
/// [`Session`]s to communicate with those clients.
///
/// This represents the "server" part of the client/server networking model (a
/// client is represented as just a [`Session`]). Its responsibility is to
/// accept and coordinate connections between multiple clients. Note, however,
/// that this does not have to represent a *dedicated* server - you may run
/// a server, and connect a client to that server, in the same app.
///
/// The server starts in an opening state (when [`ServerEndpoint`] has been
/// added but [`Server`] is not yet present), and transitions to either an
/// opened state, or fails to open and is [`Closed`]. After the server is
/// opened, the server should not close unless there is a fatal server-internal
/// error which affects all connected clients - if a single client causes issues
/// e.g. sending illegal data or breaking some invariant, that single client
/// should be disconnected instead of the entire server being torn down.
///
/// To listen for when a server is opened, add an observer listening for
/// [`Trigger<OnAdd, Server>`].
///
/// When a client connects, it is spawned as a [child] of the server entity.
/// Therefore, to query for sessions spawned under a server, use
/// [`Query<Session, With<Parent>>`]. The rest of the connection process is the
/// same as [`Session`]. If the server is [`Close`]d with a user-given reason,
/// all connected clients will be disconnected with the same reason.
///
/// [child]: Children
/// [`Session`]: crate::Session
#[derive(Debug, Component, Reflect)]
#[reflect(from_reflect = false, Component)]
#[require(ServerEndpoint)]
pub struct Server {
    opened_at: Instant,
}

impl Server {
    /// Creates a new [`Server`].
    ///
    /// - `opened_at`: the instant at which the IO layer acknowledged that the
    ///   server is now ready to accept client connections.
    #[must_use]
    pub const fn new(opened_at: Instant) -> Self {
        Self { opened_at }
    }
}

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
/// commands.trigger_targets(
///     Close::new("show's over everyone, go home"),
///     [server1, server2],
/// );
/// # }
/// ```
///
/// [`Disconnect`]: crate::connection::Disconnect
#[derive(Debug, Clone, PartialEq, Eq, Event)]
pub struct Close {
    /// User-provided closing reason.
    ///
    /// Will be used as the reason in [`Closed::ByUser`], and as the
    /// disconnection reason when disconnecting sessions connected to this
    /// server.
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

/// Triggered when a [`Server`] is no longer able to accept or manage client
/// connections.
///
/// Immediately after this, the server and its clients will be despawned
/// **without a graceful disconnect**. If you want to *request* the server to
/// close and disconnect its clients gracefully via the IO layer, see [`Close`].
///
/// This must be triggered by the IO layer when it detects a fatal server error.
/// If the error only concerns a single client, that client must be disconnected
/// instead of closing the entire server.
#[derive(Debug, Event)]
pub enum Closed {
    /// Server was closed by the user on our side, with a provided reason.
    ///
    /// Connected clients will be disconnected with the same reason.
    ByUser(String),
    /// Server encountered a fatal connection error, and can no longer accept
    /// new clients or update existing clients.
    ///
    /// Typically, this will only occur when attempting to start the server, and
    /// will not occur once the server has fully opened. If a client enters some
    /// sort of illegal state or performs an illegal action, only that single
    /// client will be disconnected instead of the entire server being torn
    /// down.
    ///
    /// If you want to get the concrete error type, use
    /// [`anyhow::Error::downcast_ref`].
    ByError(anyhow::Error),
}

impl Closed {
    /// Creates a [`Closed::ByUser`] from the given reason.
    #[must_use]
    pub fn by_user(reason: impl Into<String>) -> Self {
        Self::ByUser(reason.into())
    }

    /// Creates a [`Closed::ByError`] from the given reason.
    #[must_use]
    pub fn by_error(reason: impl Into<anyhow::Error>) -> Self {
        Self::ByError(reason.into())
    }

    /// If this value is a [`Closed::ByError`], creates a new
    /// [`Closed::ByError`] using the mapping function.
    #[must_use]
    pub fn map_err(self, f: impl FnOnce(anyhow::Error) -> anyhow::Error) -> Self {
        match self {
            Self::ByUser(reason) => Self::ByUser(reason),
            Self::ByError(err) => Self::ByError(f(err)),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for Closed {
    fn from(value: E) -> Self {
        Self::by_error(value)
    }
}

fn on_opening(trigger: Trigger<OnAdd, ServerEndpoint>) {
    let target = trigger.target();
    debug!("{target} opening");
}

fn on_opened(trigger: Trigger<OnAdd, Server>) {
    let target = trigger.target();
    debug!("{target} opened");
}

fn on_close(trigger: Trigger<Close>, mut commands: Commands) {
    let target = trigger.target();
    commands.trigger_targets(Closed::by_user(&trigger.reason), target);
}

fn on_closed(trigger: Trigger<Closed>, children: Query<&Children>, mut commands: Commands) {
    let target = trigger.target();
    let children = children
        .get(target)
        .map(|children| children.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    match &*trigger {
        Closed::ByUser(reason) => {
            debug!("{target} closed by user: {reason}");
            commands.trigger_targets(Disconnect::new(reason), children);
        }
        Closed::ByError(err) => {
            debug!("{target} closed due to error: {err:?}");
        }
    }

    commands.entity(target).despawn();
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{AeronetIoPlugin, connection::Disconnected},
    };

    #[test]
    fn disconnect_clients_on_close() {
        const REASON: &str = "disconnect reason";

        #[derive(Resource)]
        struct HasClosed(bool);

        #[derive(Resource)]
        struct HasDisconnected(bool);

        let mut app = App::new();
        app.add_plugins(AeronetIoPlugin)
            .insert_resource(HasClosed(false))
            .insert_resource(HasDisconnected(false));

        let client = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(client).observe(
            |trigger: Trigger<Disconnected>, mut has_disconnected: ResMut<HasDisconnected>| {
                assert!(matches!(
                    &*trigger,
                    Disconnected::ByUser(reason) if reason == REASON
                ));

                has_disconnected.0 = true;
            },
        );

        let server = app.world_mut().spawn_empty().id();
        app.world_mut()
            .entity_mut(server)
            .add_child(client)
            .observe(
                |trigger: Trigger<Closed>, mut has_closed: ResMut<HasClosed>| {
                    assert!(matches!(
                        &*trigger,
                        Closed::ByUser(reason) if reason == REASON
                    ));

                    has_closed.0 = true;
                },
            );

        app.world_mut().trigger_targets(Close::new(REASON), server);
        app.update();

        assert!(app.world().get_entity(client).is_err());
        assert!(app.world().resource::<HasDisconnected>().0);

        assert!(app.world().get_entity(server).is_err());
        assert!(app.world().resource::<HasClosed>().0);
    }
}
