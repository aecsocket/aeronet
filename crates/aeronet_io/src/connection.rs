//! Logic for connection and disconnection of a [`Session`].

use {
    crate::{Session, SessionEndpoint},
    bevy_app::prelude::*,
    bevy_derive::Deref,
    bevy_ecs::prelude::*,
    bevy_hierarchy::DespawnRecursiveExt,
    core::{fmt::Debug, net::SocketAddr},
    tracing::debug,
};

#[derive(Debug)]
pub(crate) struct ConnectionPlugin;

impl Plugin for ConnectionPlugin {
    fn build(&self, app: &mut App) {
        app.observe(on_connecting)
            .observe(on_connected)
            .observe(on_disconnect)
            .observe(on_disconnected);
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
/// commands.trigger_targets(Disconnect::new("show's over everyone, go home"), [
///     session1, session2,
/// ]);
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
#[derive(Debug, Event)]
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
    /// - failing to send a message reliably
    /// - the peer pretending like there are network errors to discreetly force
    ///   us to disconnect
    /// - ..and more
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
    let entity = trigger.entity();
    debug!("{entity} connecting");
}

fn on_connected(trigger: Trigger<OnAdd, Session>) {
    let entity = trigger.entity();
    debug!("{entity} connected");
}

fn on_disconnect(trigger: Trigger<Disconnect>, mut commands: Commands) {
    let entity = trigger.entity();
    let reason = DisconnectReason::User(trigger.event().reason.clone());
    commands.trigger_targets(Disconnected { reason }, entity);
}

fn on_disconnected(trigger: Trigger<Disconnected>, mut commands: Commands) {
    let entity = trigger.entity();
    match &trigger.event().reason {
        DisconnectReason::User(reason) => {
            debug!("{entity} disconnected by user: {reason}");
        }
        DisconnectReason::Peer(reason) => {
            debug!("{entity} disconnected by peer: {reason}");
        }
        DisconnectReason::Error(err) => {
            debug!("{entity} disconnected due to error: {err:#}");
        }
    }

    if let Some(entity) = commands.get_entity(entity) {
        entity.despawn_recursive();
    }
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
                    &trigger.event().reason,
                    DisconnectReason::User(reason) if reason == REASON
                ));

                has_disconnected.0 = true;
            },
        );

        app.world_mut()
            .trigger_targets(Disconnect::new(REASON), entity);
        app.update();

        assert!(app.world().get_entity(entity).is_none());
        assert!(app.world().resource::<HasDisconnected>().0);
    }
}
