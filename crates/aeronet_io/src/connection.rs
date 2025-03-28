//! Logic for connection and disconnection of a [`Session`].

use {
    crate::{Session, SessionEndpoint},
    alloc::string::String,
    bevy_app::prelude::*,
    bevy_ecs::prelude::*,
    core::{fmt::Debug, net::SocketAddr},
    derive_more::Deref,
    log::debug,
};

pub(crate) struct ConnectionPlugin;

impl Plugin for ConnectionPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_connecting)
            .add_observer(on_connected)
            .add_observer(on_disconnect)
            .add_observer(on_disconnected);
    }
}

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
/// commands.trigger_targets(
///     Disconnect::new("show's over everyone, go home"),
///     [session1, session2],
/// );
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Event)]
pub struct Disconnect {
    /// User-provided disconnection reason.
    ///
    /// Will be used as the reason in [`Disconnected::ByUser`].
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
/// Immediately after this, the session will be despawned **without a graceful
/// disconnect**. If you want to *request* the session to disconnect gracefully
/// via the IO layer, see [`Disconnect`].
///
/// This must be triggered by the IO layer when it detects that the peer has
/// disconnected from us, or when it detects a connection error.
///
/// This may also be used by code above the IO layer for e.g. signaling
/// transport errors, however this is not guaranteed.
#[derive(Debug, Event)]
pub enum Disconnected {
    /// Session was disconnected by the user on our side, with a provided
    /// reason.
    ///
    /// On the peer, this will be interpreted as a [`Disconnected::ByPeer`]
    /// with the same reason.
    ByUser(String),
    /// Session was disconnected by the peer on the other side, with a provided
    /// reason.
    ///
    /// On the peer, this will be interpreted as a [`Disconnected::ByUser`]
    /// with the same reason.
    ByPeer(String),
    /// Session encountered a fatal connection error, and communication between
    /// this session and the peer is no longer possible.
    ///
    /// This may be caused by:
    /// - a bad network condition for a prolonged period of time
    /// - a malicious or malfunctioning peer
    /// - failing to send a message reliably
    /// - the peer pretending like there are network errors to discreetly force
    ///   us to disconnect
    /// - ..and more
    ///
    /// If you want to get the concrete error type, use
    /// [`anyhow::Error::downcast_ref`].
    ByError(anyhow::Error),
}

impl Disconnected {
    /// Creates a [`Disconnected::ByUser`] from the given reason.
    #[must_use]
    pub fn by_user(reason: impl Into<String>) -> Self {
        Self::ByUser(reason.into())
    }

    /// Creates a [`Disconnected::ByPeer`] from the given reason.
    #[must_use]
    pub fn by_peer(reason: impl Into<String>) -> Self {
        Self::ByPeer(reason.into())
    }

    /// Creates a [`Disconnected::ByError`] from the given reason.
    #[must_use]
    pub fn by_error(reason: impl Into<anyhow::Error>) -> Self {
        Self::ByError(reason.into())
    }

    /// If this value is a [`Disconnected::ByError`], creates a new
    /// [`Disconnected::ByError`] using the mapping function.
    #[must_use]
    pub fn map_err(self, f: impl FnOnce(anyhow::Error) -> anyhow::Error) -> Self {
        match self {
            Self::ByUser(reason) => Self::ByUser(reason),
            Self::ByPeer(reason) => Self::ByPeer(reason),
            Self::ByError(err) => Self::ByError(f(err)),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for Disconnected {
    fn from(value: E) -> Self {
        Self::by_error(value)
    }
}

/// Disconnect reason to use when an IO layer component is dropped.
///
/// IO layer implementations may use this as a default disconnection reason when
/// their IO component is dropped, instead of being explicitly disconnected via
/// [`Disconnect`].
pub const DROP_DISCONNECT_REASON: &str = "(dropped)";

/// Disconnect reason to use when an IO layer component is disconnected for an
/// unknown reason.
pub const UNKNOWN_DISCONNECT_REASON: &str = "(unknown)";

/// Local socket address that this entity uses for connections.
///
/// Sessions or servers which use a network will use an OS socket for
/// communication. This component stores the local [`SocketAddr`] of this
/// socket.
///
/// This component may not be present in environments where there is no access
/// to OS sockets (i.e. WASM).
///
/// To access the socket address of a session's peer, see [`PeerAddr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, Component)]
pub struct LocalAddr(pub SocketAddr);

/// Socket address of the peer that this [`Session`] is connected to.
///
/// Sessions which use a network will use an OS socket for communication. This
/// component stores the [`SocketAddr`] of the peer, which this session is
/// connected to.
///
/// This component may not be present in environments where there is no access
/// to OS sockets (i.e. WASM).
///
/// To access the local socket address of a session, see [`LocalAddr`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deref, Component)]
pub struct PeerAddr(pub SocketAddr);

fn on_connecting(trigger: Trigger<OnAdd, SessionEndpoint>) {
    let target = trigger.target();
    debug!("{target} connecting");
}

fn on_connected(trigger: Trigger<OnAdd, Session>) {
    let target = trigger.target();
    debug!("{target} connected");
}

fn on_disconnect(trigger: Trigger<Disconnect>, mut commands: Commands) {
    let target = trigger.target();
    commands.trigger_targets(Disconnected::by_user(&trigger.reason), target);
}

fn on_disconnected(trigger: Trigger<Disconnected>, mut commands: Commands) {
    let target = trigger.target();
    match &*trigger {
        Disconnected::ByUser(reason) => {
            debug!("{target} disconnected by user: {reason}");
        }
        Disconnected::ByPeer(reason) => {
            debug!("{target} disconnected by peer: {reason}");
        }
        Disconnected::ByError(err) => {
            debug!("{target} disconnected due to error: {err:?}");
        }
    }

    commands.entity(target).despawn();
}

#[cfg(test)]
mod tests {
    use {super::*, crate::AeronetIoPlugin};

    #[test]
    fn remove_entity_on_disconnect() {
        const REASON: &str = "disconnect reason";

        #[derive(Resource)]
        struct HasDisconnected(bool);

        let mut app = App::new();
        app.add_plugins(AeronetIoPlugin)
            .insert_resource(HasDisconnected(false));

        let entity = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(entity).observe(
            |trigger: Trigger<Disconnected>, mut has_disconnected: ResMut<HasDisconnected>| {
                assert!(matches!(
                    &*trigger,
                    Disconnected::ByUser(reason) if reason == REASON
                ));

                has_disconnected.0 = true;
            },
        );

        app.world_mut()
            .trigger_targets(Disconnect::new(REASON), entity);
        app.update();

        assert!(app.world().get_entity(entity).is_err());
        assert!(app.world().resource::<HasDisconnected>().0);
    }
}
