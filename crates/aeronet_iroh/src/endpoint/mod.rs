//! See [`IrohEndpoint`].

mod backend;

use {
    crate::{
        ALPN, IrohRuntime,
        session::{self, IrohSessionPlugin, SessionSide},
    },
    aeronet_io::{IoSystems, connection::DisconnectReason},
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    core::mem,
    derive_more::{Display, Error},
    futures::channel::{mpsc, oneshot},
    iroh::{EndpointAddr, EndpointId, endpoint::Builder},
    tracing::{Instrument, debug, debug_span},
};

/// Allows using [`IrohEndpoint`].
pub struct IrohEndpointPlugin;

impl Plugin for IrohEndpointPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<IrohSessionPlugin>() {
            app.add_plugins(IrohSessionPlugin);
        }

        app.add_systems(
            PreUpdate,
            (poll_opening, poll_opened)
                .in_set(IoSystems::Poll)
                .before(session::poll_connecting),
        )
        .add_observer(on_endpoint_closed);
    }
}

/// Symmetric Iroh endpoint which accepts incoming sessions and initiates
/// outgoing sessions.
///
/// Use [`IrohEndpoint::open`] to start opening an endpoint. The component is
/// added once the endpoint has successfully bound and is ready. You can observe
/// `On<Add, IrohEndpoint>` to learn when this happens.
///
/// When a peer attempts to connect, the endpoint will trigger a
/// [`SessionRequest`](session::SessionRequest). Your app **must** observe this,
/// and eventually use
/// [`SessionRequest::respond`](session::SessionRequest::respond) to set how the
/// endpoint should respond to this connection attempt.
///
/// Iroh endpoints have no fixed client or server role. The same endpoint can
/// accept and initiate any number of sessions. Every session entity is a child
/// of the endpoint entity which owns it.
#[derive(Debug, Component)]
pub struct IrohEndpoint {
    entity: Entity,
    raw: iroh::Endpoint,
}

impl IrohEndpoint {
    /// Creates an [`EntityCommand`] to set up and open an endpoint using the
    /// given Iroh `builder`.
    ///
    /// The builder's accepted ALPN protocols are replaced with Aeronet's
    /// [`ALPN`]. The rest of the builder, including its identity, address
    /// lookup services, direct transports, and relay configuration, is
    /// preserved.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use {
    ///     aeronet_iroh::endpoint::IrohEndpoint,
    ///     bevy_ecs::{prelude::*, system::EntityCommand},
    ///     iroh::endpoint::presets,
    /// };
    ///
    /// # fn run(mut commands: Commands, world: &mut World) {
    /// let builder = iroh::Endpoint::builder(presets::N0);
    ///
    /// // using `Commands`
    /// commands.spawn_empty().queue(IrohEndpoint::open(builder));
    ///
    /// // using mutable `World` access
    /// # let builder = iroh::Endpoint::builder(presets::N0);
    /// let endpoint = world.spawn_empty().id();
    /// IrohEndpoint::open(builder).apply(world.entity_mut(endpoint));
    /// # }
    /// ```
    #[must_use]
    pub fn open(builder: Builder) -> impl EntityCommand {
        move |entity: EntityWorldMut| open(entity, builder)
    }

    /// Creates an [`EntityCommand`] to connect a session entity to `target`.
    ///
    /// `target` may be an [`EndpointAddr`] containing direct and relay
    /// addresses, or an [`EndpointId`]. Connecting with only an endpoint ID
    /// requires the endpoint builder to have a suitable address lookup service.
    /// The session entity becomes a child of this endpoint entity.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use {aeronet_iroh::endpoint::IrohEndpoint, bevy_ecs::prelude::*, iroh::EndpointAddr};
    ///
    /// fn connect(mut commands: Commands, endpoints: Query<&IrohEndpoint>, target: Res<Target>) {
    ///     let endpoint = endpoints.single().unwrap();
    ///     commands
    ///         .spawn_empty()
    ///         .queue(endpoint.connect(target.0.clone()));
    /// }
    ///
    /// #[derive(Resource)]
    /// struct Target(EndpointAddr);
    /// ```
    #[must_use]
    pub fn connect<T>(&self, target: T) -> impl EntityCommand + use<T>
    where
        T: Into<EndpointAddr>,
    {
        let endpoint_entity = self.entity;
        let endpoint = self.raw.clone();
        let target = target.into();
        move |entity: EntityWorldMut| {
            session::connect(entity, endpoint_entity, endpoint, target);
        }
    }

    /// Returns this endpoint's cryptographic identity.
    #[must_use]
    pub fn id(&self) -> EndpointId {
        self.raw.id()
    }

    /// Returns this endpoint's current addressing information.
    ///
    /// The address may change as Iroh discovers direct addresses or connects
    /// to a relay. Use [`iroh::Endpoint::watch_addr`] through
    /// [`IrohEndpoint::raw`] if you need to observe every update.
    #[must_use]
    pub fn addr(&self) -> EndpointAddr {
        self.raw.addr()
    }

    /// Returns the underlying Iroh endpoint.
    ///
    /// Aeronet owns the accept loop and accepted ALPN configuration. Do not
    /// close the returned endpoint while the entity is in use.
    #[must_use]
    pub const fn raw(&self) -> &iroh::Endpoint {
        &self.raw
    }
}

/// Error that occurs while opening or polling an [`IrohEndpoint`].
#[derive(Debug, Display, Error)]
#[non_exhaustive]
pub enum EndpointError {
    /// Frontend endpoint entity was dropped.
    #[display("frontend closed")]
    FrontendClosed,
    /// Backend async task was unexpectedly cancelled and dropped.
    #[display("backend closed")]
    BackendClosed,
    /// Failed to bind the Iroh endpoint.
    #[display("failed to bind endpoint")]
    Bind(iroh::endpoint::BindError),
    /// The Iroh endpoint closed unexpectedly.
    #[display("endpoint closed")]
    Closed,
}

/// Triggered when an [`IrohEndpoint`] fails to open or closes unexpectedly.
///
/// Immediately after this, the endpoint entity and all of its session children
/// are despawned.
#[derive(Debug, EntityEvent)]
pub struct EndpointClosed {
    /// [`IrohEndpoint`] entity which closed.
    #[event_target]
    pub entity: Entity,
    /// Error which caused the endpoint to close.
    pub error: EndpointError,
}

#[derive(Debug, Component)]
struct Opening {
    rx_error: oneshot::Receiver<EndpointError>,
    rx_next: oneshot::Receiver<ToOpen>,
}

#[derive(Debug, Component)]
struct Opened {
    rx_error: oneshot::Receiver<EndpointError>,
    rx_connecting: mpsc::Receiver<ToConnecting>,
    _tx_dropped: mpsc::Sender<()>,
}

#[derive(Debug)]
struct ToOpen {
    endpoint: iroh::Endpoint,
    rx_connecting: mpsc::Receiver<ToConnecting>,
    tx_dropped: mpsc::Sender<()>,
}

#[derive(Debug)]
struct ToConnecting {
    peer_id: EndpointId,
    tx_response: oneshot::Sender<session::SessionResponse>,
    rx_dc_reason: oneshot::Receiver<DisconnectReason>,
    rx_next: oneshot::Receiver<session::ToConnected>,
}

fn open(mut entity: EntityWorldMut, builder: Builder) {
    let runtime = entity.world().resource::<IrohRuntime>().clone();
    let (tx_error, rx_error) = oneshot::channel::<EndpointError>();
    let (tx_next, rx_next) = oneshot::channel::<ToOpen>();
    runtime.spawn_on_self(
        async move {
            let Err(err) = backend::start(builder.alpns(vec![ALPN.to_vec()]), tx_next).await;
            debug!("Endpoint closed: {err:?}");
            _ = tx_error.send(err);
        }
        .instrument(debug_span!("endpoint", entity = %entity.id())),
    );

    entity.insert(Opening { rx_error, rx_next });
}

fn poll_opening(mut commands: Commands, mut endpoints: Query<(Entity, &mut Opening)>) {
    for (entity, mut endpoint) in &mut endpoints {
        if try_close(&mut commands, entity, &mut endpoint.rx_error) {
            continue;
        }

        let Ok(Some(next)) = endpoint.rx_next.try_recv() else {
            continue;
        };
        let (_, dummy) = oneshot::channel();
        let rx_error = mem::replace(&mut endpoint.rx_error, dummy);
        commands.entity(entity).remove::<Opening>().insert((
            IrohEndpoint {
                entity,
                raw: next.endpoint,
            },
            Opened {
                rx_error,
                rx_connecting: next.rx_connecting,
                _tx_dropped: next.tx_dropped,
            },
        ));
    }
}

fn poll_opened(
    mut commands: Commands,
    mut endpoints: Query<(Entity, &mut Opened), With<IrohEndpoint>>,
) {
    for (entity, mut endpoint) in &mut endpoints {
        if try_close(&mut commands, entity, &mut endpoint.rx_error) {
            continue;
        }

        while let Ok(Some(connecting)) = endpoint.rx_connecting.try_next() {
            let session = commands
                .spawn((
                    ChildOf(entity),
                    session::IrohSession::new(entity, connecting.peer_id, SessionSide::Incoming),
                    session::Connecting::new(connecting.rx_dc_reason, connecting.rx_next),
                ))
                .id();

            commands.trigger(session::SessionRequest::new(
                entity,
                session,
                connecting.peer_id,
                connecting.tx_response,
            ));
        }
    }
}

fn try_close(
    commands: &mut Commands,
    entity: Entity,
    rx_error: &mut oneshot::Receiver<EndpointError>,
) -> bool {
    let error = match rx_error.try_recv() {
        Ok(None) => None,
        Ok(Some(error)) => Some(error),
        Err(_) => Some(EndpointError::BackendClosed),
    };
    error.is_some_and(|error| {
        commands.trigger(EndpointClosed { entity, error });
        true
    })
}

fn on_endpoint_closed(trigger: On<EndpointClosed>, mut commands: Commands) {
    let entity = trigger.event_target();
    debug!(error = ?trigger.error, "Endpoint closed");
    commands.entity(entity).try_despawn();
}
