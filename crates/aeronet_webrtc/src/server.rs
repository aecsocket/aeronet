use {
    crate::{
        ConfigError, IceServer, PeerConfig, SessionDescription, SessionDescriptionType, Signal,
        SignalData, WebRtcError, WebRtcIo, WebRtcRuntime, backend,
    },
    aeronet_io::{
        SessionEndpoint,
        connection::{DisconnectReason, Disconnected},
        server::{Server, ServerEndpoint},
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    bevy_platform::time::Instant,
};

/// Adds the native signaling-routed WebRTC server implementation.
pub struct WebRtcServerPlugin;

impl Plugin for WebRtcServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::session::WebRtcSessionPlugin>() {
            app.add_plugins(crate::session::WebRtcSessionPlugin);
        }
        app.add_observer(on_incoming_offer);
    }
}

/// Native WebRTC server parent which admits offerer endpoints.
#[derive(Debug, Clone, Component)]
#[require(ServerEndpoint)]
pub struct WebRtcServer {
    pub(crate) config: PeerConfig,
}

impl WebRtcServer {
    /// Creates a command that opens a server using `config`.
    ///
    /// # Errors
    ///
    /// Returns an error when the configuration is invalid.
    pub fn open(config: PeerConfig) -> Result<impl EntityCommand, ConfigError> {
        config.validate()?;
        Ok(move |mut entity: EntityWorldMut| {
            entity.insert((Self { config }, Server::new(Instant::now())));
        })
    }

    /// Validates a complete ICE replacement against this server's policy.
    ///
    /// # Errors
    ///
    /// Returns an error without changing the current template when the
    /// replacement is empty or violates the server's ICE policy.
    pub fn update_ice_servers(&mut self, ice_servers: Vec<IceServer>) -> Result<(), ConfigError> {
        if ice_servers.is_empty() {
            return Err(ConfigError::InvalidIceServer);
        }
        let mut config = self.config.clone();
        config.ice_servers = ice_servers;
        config.validate()?;
        self.config = config;
        Ok(())
    }
}

/// Injects an externally routed offer into a native server parent.
#[derive(Debug, Clone, EntityEvent)]
pub struct IncomingOffer {
    /// Server entity receiving the offer.
    pub entity: Entity,
    /// Offer and application routing key.
    pub signal: Signal,
}

impl IncomingOffer {
    /// Creates an offer event for `server`.
    #[must_use]
    pub const fn new(server: Entity, signal: Signal) -> Self {
        Self {
            entity: server,
            signal,
        }
    }
}

/// Synchronous application admission request for an incoming WebRTC offer.
///
/// The event targets [`Self::server_entity`]. [`Self::session_entity`] persists
/// as the accepted peer, so observers may attach application components before
/// responding. The first response wins; dropping an unanswered request safely
/// rejects it.
#[derive(Debug, EntityEvent)]
pub struct SessionRequest {
    #[event_target]
    /// Server that received the offer.
    pub server_entity: Entity,
    /// Pending child endpoint that becomes the accepted peer.
    pub session_entity: Entity,
    /// Application routing key supplied by the offerer.
    pub connection_id: String,
    response: Option<bool>,
}

impl SessionRequest {
    /// Accepts (`true`) or rejects (`false`) this request. Later calls are
    /// ignored and logged.
    pub fn respond(&mut self, accepted: bool) {
        if self.response.is_some() {
            tracing::warn!(
                entity = %self.session_entity,
                server = %self.server_entity,
                "WebRTC session request was already answered"
            );
            return;
        }
        self.response = Some(accepted);
    }
}

#[derive(Component)]
struct PendingConnection(String);

fn on_incoming_offer(
    trigger: On<IncomingOffer>,
    servers: Query<&WebRtcServer>,
    mut commands: Commands,
) {
    let server = trigger.event_target();
    if let Err(error) = trigger.signal.validate() {
        tracing::error!(%server, %error, "WebRTC server initial signal exceeds protocol size limits");
        return;
    }
    let Ok(_) = servers.get(server) else {
        tracing::error!(%server, "WebRTC offer targeted a non-WebRTC server");
        return;
    };
    let SignalData::SessionDescription(offer) = &trigger.signal.data else {
        tracing::error!(%server, "WebRTC server received a non-description initial signal");
        return;
    };
    if offer.kind != SessionDescriptionType::Offer {
        tracing::error!(%server, "WebRTC server initial description was not an offer");
        return;
    }
    let connection_id = trigger.signal.connection_id.clone();
    let offer = offer.clone();
    commands.queue(move |world: &mut World| admit_offer(world, server, connection_id, offer));
}

fn admit_offer(
    world: &mut World,
    server: Entity,
    connection_id: String,
    offer: SessionDescription,
) {
    if world.get::<WebRtcServer>(server).is_none() {
        tracing::error!(%server, "WebRTC server disappeared before offer admission");
        return;
    }
    let mut existing = world.query::<(&ChildOf, Option<&PendingConnection>, Option<&WebRtcIo>)>();
    for (child_of, pending, io) in existing.iter(world) {
        if child_of.parent() != server {
            continue;
        }
        if pending
            .map(|pending| pending.0.as_str())
            .or_else(|| io.map(WebRtcIo::connection_id))
            == Some(connection_id.as_str())
        {
            tracing::warn!(
                %server,
                "rejecting duplicate WebRTC connection ID"
            );
            return;
        }
    }
    let client = world
        .spawn((
            ChildOf(server),
            SessionEndpoint,
            PendingConnection(connection_id.clone()),
        ))
        .id();
    let mut request = SessionRequest {
        server_entity: server,
        session_entity: client,
        connection_id: connection_id.clone(),
        response: None,
    };
    world.trigger_ref(&mut request);
    world.flush();
    let accepted = match request.response {
        Some(true) => true,
        Some(false) => {
            tracing::debug!(%client, %server, "WebRTC session request was explicitly rejected");
            false
        }
        None => {
            tracing::warn!(%client, %server, "rejecting unanswered WebRTC session request");
            false
        }
    };
    if !accepted {
        reject(world, client);
        return;
    }

    let valid_child = world.get_entity(client).is_ok_and(|child| {
        child.contains::<SessionEndpoint>()
            && child
                .get::<ChildOf>()
                .is_some_and(|relationship| relationship.parent() == server)
            && child
                .get::<PendingConnection>()
                .is_some_and(|pending| pending.0 == connection_id)
    });
    if !valid_child {
        tracing::error!(%client, %server, "accepted WebRTC session request invalidated its child endpoint");
        reject(world, client);
        return;
    }
    let Some(config) = world
        .get::<WebRtcServer>(server)
        .map(|server| server.config.clone())
    else {
        tracing::error!(%client, %server, "WebRTC server disappeared during admission");
        reject(world, client);
        return;
    };
    let Some(runtime) = world.get_resource::<WebRtcRuntime>().cloned() else {
        tracing::error!(%client, %server, "accepted WebRTC session has no WebRtcServerPlugin runtime");
        reject(world, client);
        return;
    };
    world
        .entity_mut(client)
        .remove::<PendingConnection>()
        .insert(WebRtcIo::new(
            connection_id,
            backend::start_server(config, offer, runtime),
        ));
}

fn reject(world: &mut World, entity: Entity) {
    if world.get_entity(entity).is_ok() {
        world.trigger(Disconnected {
            entity,
            reason: DisconnectReason::by_error(WebRtcError::Rejected),
        });
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bevy_ecs::system::EntityCommand,
        futures::channel::{mpsc, oneshot},
    };

    #[derive(Resource, Default)]
    struct Requested {
        entity: Option<Entity>,
        count: usize,
    }

    #[derive(Component)]
    struct AttachedBeforeAccept;

    fn offer_description() -> SessionDescription {
        SessionDescription {
            kind: SessionDescriptionType::Offer,
            sdp: "v=0\r\n".to_owned(),
        }
    }

    fn offer() -> Signal {
        offer_with_connection_id("server-child-test")
    }

    fn offer_with_connection_id(connection_id: impl Into<String>) -> Signal {
        Signal {
            connection_id: connection_id.into(),
            data: SignalData::SessionDescription(offer_description()),
        }
    }

    fn spawn_server(app: &mut App, config: PeerConfig) -> Entity {
        let entity = app.world_mut().spawn_empty().id();
        WebRtcServer::open(config)
            .unwrap()
            .apply(app.world_mut().entity_mut(entity));
        entity
    }

    #[test]
    fn accepted_request_atomically_creates_active_child() {
        let mut app = App::new();
        app.add_plugins(WebRtcServerPlugin)
            .init_resource::<Requested>();
        app.add_observer(
            |mut request: On<SessionRequest>,
             mut requested: ResMut<Requested>,
             mut commands: Commands| {
                assert_eq!(request.event_target(), request.server_entity);
                requested.entity = Some(request.session_entity);
                requested.count += 1;
                commands
                    .entity(request.session_entity)
                    .insert(AttachedBeforeAccept);
                request.respond(true);
            },
        );
        let server = spawn_server(&mut app, PeerConfig::default());
        app.world_mut().trigger(IncomingOffer::new(server, offer()));
        app.update();

        let child = app
            .world()
            .resource::<Requested>()
            .entity
            .expect("request was not observed");
        let child_ref = app.world().entity(child);
        assert_eq!(child_ref.get::<ChildOf>().unwrap().parent(), server);
        assert!(child_ref.contains::<SessionEndpoint>());
        assert!(child_ref.contains::<WebRtcIo>());
        assert!(child_ref.contains::<AttachedBeforeAccept>());
    }

    #[test]
    fn rejected_request_despawns_child() {
        let mut app = App::new();
        app.add_plugins(WebRtcServerPlugin)
            .init_resource::<Requested>();
        app.add_observer(
            |mut request: On<SessionRequest>, mut requested: ResMut<Requested>| {
                requested.entity = Some(request.session_entity);
                requested.count += 1;
                request.respond(false);
            },
        );
        let server = spawn_server(&mut app, PeerConfig::default());
        app.world_mut().trigger(IncomingOffer::new(server, offer()));
        app.update();
        app.update();
        let child = app.world().resource::<Requested>().entity.unwrap();
        assert!(app.world().get_entity(child).is_err());
    }

    #[test]
    fn accepted_request_must_preserve_its_endpoint_and_server() {
        type Mutation = fn(Entity, Entity, &mut Commands);
        let mutations: [Mutation; 5] = [
            |entity, server, commands| {
                commands.entity(entity).insert(ChildOf(server));
            },
            |entity, _, commands| {
                commands.entity(entity).remove::<SessionEndpoint>();
            },
            |entity, _, commands| {
                commands.entity(entity).remove::<PendingConnection>();
            },
            |entity, _, commands| {
                commands
                    .entity(entity)
                    .insert(PendingConnection("replaced-reservation".to_owned()));
            },
            |entity, _, commands| {
                commands.entity(entity).despawn();
            },
        ];

        for mutate in mutations {
            let mut app = App::new();
            app.add_plugins(WebRtcServerPlugin)
                .init_resource::<Requested>();
            let server = spawn_server(&mut app, PeerConfig::default());
            let other_server = app.world_mut().spawn_empty().id();
            app.add_observer(
                move |mut request: On<SessionRequest>,
                      mut requested: ResMut<Requested>,
                      mut commands: Commands| {
                    requested.entity = Some(request.session_entity);
                    mutate(request.session_entity, other_server, &mut commands);
                    request.respond(true);
                },
            );

            app.world_mut().trigger(IncomingOffer::new(server, offer()));
            app.update();

            let child = app
                .world()
                .resource::<Requested>()
                .entity
                .expect("request was not observed");
            assert!(app.world().get_entity(child).is_err());
        }
    }

    #[test]
    fn duplicate_connection_id_leaves_active_child_untouched() {
        let mut app = App::new();
        app.add_observer(on_incoming_offer)
            .init_resource::<Requested>()
            .add_observer(
                |mut request: On<SessionRequest>, mut requested: ResMut<Requested>| {
                    requested.entity = Some(request.session_entity);
                    requested.count += 1;
                    request.respond(false);
                },
            );
        let server = spawn_server(&mut app, PeerConfig::default());

        let (tx_signal, _rx_signal) = mpsc::channel(4);
        let (tx_packet, _rx_packet) = mpsc::channel(4);
        let (_tx_event, rx_event) = mpsc::channel(4);
        let (_tx_diagnostic, rx_diagnostic) = mpsc::channel(4);
        let (_tx_incoming, rx_incoming) = mpsc::channel(4);
        let (cancel, _rx_cancel) = oneshot::channel();
        let active = app
            .world_mut()
            .spawn((
                ChildOf(server),
                WebRtcIo::new(
                    "active-duplicate".to_owned(),
                    backend::Backend {
                        tx_signal,
                        tx_packet,
                        rx_event,
                        rx_diagnostic,
                        rx_incoming,
                        sent: backend::SendCompletions::default(),
                        cancel: Some(cancel),
                        capacity: 4,
                    },
                ),
            ))
            .id();
        app.world_mut().trigger(IncomingOffer::new(
            server,
            offer_with_connection_id("active-duplicate"),
        ));

        assert_eq!(app.world().resource::<Requested>().count, 0);
        assert!(app.world().entity(active).contains::<WebRtcIo>());
        assert_eq!(
            app.world().entity(server).get::<Children>().unwrap().len(),
            1
        );
    }

    #[test]
    fn reentrant_duplicate_offer_is_rejected_by_pending_reservation() {
        let mut app = App::new();
        app.add_plugins(WebRtcServerPlugin)
            .init_resource::<Requested>();
        let server = spawn_server(&mut app, PeerConfig::default());
        app.add_observer(
            move |mut request: On<SessionRequest>,
                  mut requested: ResMut<Requested>,
                  mut commands: Commands| {
                requested.entity = Some(request.session_entity);
                requested.count += 1;
                commands.trigger(IncomingOffer::new(
                    server,
                    offer_with_connection_id("reentrant-duplicate"),
                ));
                request.respond(true);
            },
        );

        app.world_mut().trigger(IncomingOffer::new(
            server,
            offer_with_connection_id("reentrant-duplicate"),
        ));
        app.update();

        let requested = app.world().resource::<Requested>();
        assert_eq!(requested.count, 1);
    }

    #[test]
    fn oversized_offer_is_not_admitted() {
        let mut app = App::new();
        app.add_observer(on_incoming_offer)
            .init_resource::<Requested>()
            .add_observer(
                |mut request: On<SessionRequest>, mut requested: ResMut<Requested>| {
                    requested.entity = Some(request.session_entity);
                    requested.count += 1;
                    request.respond(false);
                },
            );
        let server = spawn_server(&mut app, PeerConfig::default());
        let oversized = Signal {
            connection_id: "oversized".to_owned(),
            data: SignalData::SessionDescription(SessionDescription {
                kind: SessionDescriptionType::Offer,
                sdp: "x".repeat(crate::MAX_SESSION_DESCRIPTION_BYTES + 1),
            }),
        };

        app.world_mut()
            .trigger(IncomingOffer::new(server, oversized));

        assert_eq!(app.world().resource::<Requested>().count, 0);
        assert!(app.world().entity(server).get::<Children>().is_none());
    }

    #[test]
    fn server_updates_validate_templates_atomically() {
        let mut app = App::new();
        app.add_plugins(WebRtcServerPlugin);
        let server = spawn_server(&mut app, PeerConfig::default());
        let ice_servers = vec![crate::IceServer {
            urls: vec!["turn:example.test".to_owned()],
            username: "new-user".to_owned(),
            credential: "new-credential".to_owned(),
        }];

        app.world_mut()
            .get_mut::<WebRtcServer>(server)
            .unwrap()
            .update_ice_servers(ice_servers)
            .unwrap();

        let server_config = &app
            .world()
            .entity(server)
            .get::<WebRtcServer>()
            .unwrap()
            .config;
        assert_eq!(server_config.ice_servers[0].username, "new-user");

        let relay_server = spawn_server(
            &mut app,
            PeerConfig {
                ice_servers: vec![IceServer {
                    urls: vec!["turn:original.test".to_owned()],
                    username: "user".to_owned(),
                    credential: "credential".to_owned(),
                }],
                ice_transport_policy: crate::IceTransportPolicy::RelayOnly,
                ..Default::default()
            },
        );

        assert!(matches!(
            app.world_mut()
                .get_mut::<WebRtcServer>(relay_server)
                .unwrap()
                .update_ice_servers(vec![IceServer {
                    urls: vec!["stun:replacement.test".to_owned()],
                    ..Default::default()
                }],),
            Err(ConfigError::RelayWithoutTurnServer)
        ));
        assert_eq!(
            app.world()
                .get::<WebRtcServer>(relay_server)
                .unwrap()
                .config
                .ice_servers[0]
                .urls[0],
            "turn:original.test"
        );
    }

    #[test]
    fn first_session_response_wins() {
        let mut request = SessionRequest {
            server_entity: Entity::PLACEHOLDER,
            session_entity: Entity::PLACEHOLDER,
            connection_id: "unanswered".to_owned(),
            response: None,
        };
        request.respond(true);
        request.respond(false);
        assert_eq!(request.response, Some(true));
    }
}
