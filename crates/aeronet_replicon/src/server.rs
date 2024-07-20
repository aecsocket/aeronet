//! Server-side traits and items.

use std::{marker::PhantomData, ops::Deref, time::Duration};

use aeronet::{
    error::pretty_error,
    lane::LaneIndex,
    server::{
        server_open, RemoteClientConnected, RemoteClientConnecting, RemoteClientDisconnected,
        ServerClosed, ServerEvent, ServerOpened, ServerState, ServerTransport, ServerTransportSet,
    },
};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_replicon::{
    core::ClientId,
    server::{replicon_server::RepliconServer, ServerSet},
};
use bevy_time::{common_conditions::on_real_timer, prelude::*};
use bimap::{BiHashMap, Overwritten};
use bytes::Bytes;
use derivative::Derivative;
use tracing::{debug, warn};

/// Provides a [`bevy_replicon`] server backend using the given [`aeronet`]
/// transport.
///
/// You must use [`RepliconServerPlugin`] and Replicon's [`ServerPlugin`]
/// together.
///
/// System sets:
/// * [`ServerTransportSet::Recv`]
/// * [`ServerTransportSet::Send`]
///
/// Events:
/// * [`ServerOpened`]
/// * [`ServerClosed`]
/// * [`RemoteClientConnecting`]
/// * [`RemoteClientConnected`]
/// * [`RemoteClientDisconnected`]
///
/// [`ServerPlugin`]: bevy_replicon::server::ServerPlugin
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct RepliconServerPlugin<T> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<T>,
}

impl<T: ServerTransport + Resource> Plugin for RepliconServerPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_event::<ServerOpened<T>>()
            .add_event::<ServerClosed<T>>()
            .add_event::<RemoteClientConnecting<T>>()
            .add_event::<RemoteClientConnected<T>>()
            .add_event::<RemoteClientDisconnected<T>>()
            .init_resource::<ClientKeys<T>>()
            .configure_sets(
                PreUpdate,
                (
                    ServerTransportSet::Recv,
                    ServerSet::ReceivePackets.after(ServerTransportSet::Recv),
                ),
            )
            .configure_sets(
                PostUpdate,
                (
                    ServerTransportSet::Send,
                    ServerSet::SendPackets.before(ServerTransportSet::Send),
                ),
            )
            .add_systems(
                PreUpdate,
                (
                    Self::recv.run_if(
                        resource_exists::<T>
                            .and_then(resource_exists::<ClientKeys<T>>)
                            .and_then(resource_exists::<RepliconServer>), // .and_then(on_real_timer(Duration::from_millis(1))), // TODO remove this
                    ),
                    Self::update_state.run_if(resource_exists::<RepliconServer>),
                )
                    .chain()
                    .in_set(ServerSet::ReceivePackets),
            )
            .add_systems(
                PostUpdate,
                Self::flush
                    .run_if(
                        server_open::<T>.and_then(resource_exists::<RepliconServer>), // .and_then(on_real_timer(Duration::from_millis(1))), // TODO remove
                    )
                    .in_set(ServerSet::SendPackets),
            );
    }
}

type ClientMap<K> = BiHashMap<K, ClientId, ahash::RandomState, ahash::RandomState>;

/// Stores mappings between `T::ClientKey`s and [`ClientId`]s as a bidirectional
/// map.
#[derive(Derivative, Resource)]
#[derivative(Debug(bound = ""), Clone(bound = ""))]
pub struct ClientKeys<T: ServerTransport> {
    id_map: ClientMap<T::ClientKey>,
    next_id: ClientId,
}

impl<T: ServerTransport> Deref for ClientKeys<T> {
    type Target = ClientMap<T::ClientKey>;

    fn deref(&self) -> &Self::Target {
        &self.id_map
    }
}

impl<T: ServerTransport> ClientKeys<T> {
    /// Gets the mappings between `T::ClientKey`s and [`ClientId`]s.
    #[must_use]
    pub const fn map(&self) -> &ClientMap<T::ClientKey> {
        &self.id_map
    }

    fn next_id(&mut self) -> ClientId {
        let id = self.next_id;
        self.next_id = ClientId::new(self.next_id.get().wrapping_add(1));
        id
    }
}

impl<T: ServerTransport> Default for ClientKeys<T> {
    fn default() -> Self {
        Self {
            id_map: BiHashMap::with_hashers(ahash::RandomState::new(), ahash::RandomState::new()),
            // start at 1, because 0 is ClientId::SERVER
            next_id: ClientId::new(1),
        }
    }
}

type RepliconEvent = bevy_replicon::server::ServerEvent;

impl<T: ServerTransport + Resource> RepliconServerPlugin<T> {
    fn recv(
        time: Res<Time>,
        mut server: ResMut<T>,
        mut replicon_server: ResMut<RepliconServer>,
        mut client_keys: ResMut<ClientKeys<T>>,
        mut replicon_events: EventWriter<RepliconEvent>,
        mut opened: EventWriter<ServerOpened<T>>,
        mut closed: EventWriter<ServerClosed<T>>,
        mut connecting: EventWriter<RemoteClientConnecting<T>>,
        mut connected: EventWriter<RemoteClientConnected<T>>,
        mut disconnected: EventWriter<RemoteClientDisconnected<T>>,
    ) {
        for event in server.poll(time.delta()) {
            match event {
                ServerEvent::Opened => {
                    opened.send(ServerOpened::default());
                }
                ServerEvent::Closed { error } => {
                    closed.send(ServerClosed { error });
                }
                ServerEvent::Connecting { client_key } => {
                    connecting.send(RemoteClientConnecting { client_key });
                }
                ServerEvent::Connected { client_key } => Self::on_connected(
                    client_keys.as_mut(),
                    &mut connected,
                    &mut replicon_events,
                    client_key,
                ),
                ServerEvent::Disconnected { client_key, error } => Self::on_disconnected(
                    client_keys.as_mut(),
                    &mut disconnected,
                    &mut replicon_events,
                    client_key,
                    error,
                ),
                ServerEvent::Recv {
                    client_key,
                    msg,
                    lane,
                } => Self::on_recv(
                    client_keys.as_ref(),
                    replicon_server.as_mut(),
                    client_key,
                    msg,
                    lane,
                ),
                ServerEvent::Ack { .. } | ServerEvent::Nack { .. } => {}
            }
        }
    }

    fn on_connected(
        client_keys: &mut ClientKeys<T>,
        connected: &mut EventWriter<RemoteClientConnected<T>>,
        replicon_events: &mut EventWriter<RepliconEvent>,
        client_key: T::ClientKey,
    ) {
        connected.send(RemoteClientConnected {
            client_key: client_key.clone(),
        });

        let client_id = client_keys.next_id();
        debug!("Associating {client_key:?} with {client_id:?}");
        match client_keys.id_map.insert(client_key, client_id) {
            Overwritten::Neither => {}
            overwritten => {
                warn!("Inserted duplicate client key/ID pair: {overwritten:?}");
            }
        }
        replicon_events.send(RepliconEvent::ClientConnected { client_id });
    }

    fn on_disconnected(
        client_keys: &mut ClientKeys<T>,
        disconnected: &mut EventWriter<RemoteClientDisconnected<T>>,
        replicon_events: &mut EventWriter<RepliconEvent>,
        client_key: T::ClientKey,
        error: T::Error,
    ) {
        let reason_str = format!("{:#}", aeronet::error::pretty_error(&error));
        disconnected.send(RemoteClientDisconnected {
            client_key: client_key.clone(),
            error,
        });

        let Some((_, client_id)) = client_keys.id_map.remove_by_left(&client_key) else {
            warn!("Disconnected client {client_key:?} which does not have a client ID");
            return;
        };
        debug!("Removed {client_key:?} associated with {client_id:?}");
        replicon_events.send(RepliconEvent::ClientDisconnected {
            client_id,
            reason: reason_str,
        });
    }

    fn on_recv(
        client_keys: &ClientKeys<T>,
        replicon_server: &mut RepliconServer,
        client_key: T::ClientKey,
        msg: Bytes,
        lane: LaneIndex,
    ) {
        let Ok(channel) = u8::try_from(lane.into_raw()) else {
            warn!("Received message on {lane:?}, which is not a valid Replicon channel");
            return;
        };
        let Some(client_id) = client_keys.id_map.get_by_left(&client_key) else {
            warn!("Received message from client {client_key:?} which does not have a client ID");
            return;
        };
        replicon_server.insert_received(*client_id, channel, msg);
    }

    fn update_state(server: Option<Res<T>>, mut replicon: ResMut<RepliconServer>) {
        let running = if let Some(server) = server {
            match server.state() {
                ServerState::Closed | ServerState::Opening(_) => false,
                ServerState::Open(_) => true,
            }
        } else {
            false
        };

        if running != replicon.is_running() {
            replicon.set_running(running);
        }
    }

    fn flush(
        mut server: ResMut<T>,
        mut replicon: ResMut<RepliconServer>,
        client_keys: Res<ClientKeys<T>>,
    ) {
        for (client_id, channel_id, payload) in replicon.drain_sent() {
            let Some(client_key) = client_keys.id_map.get_by_right(&client_id) else {
                warn!(
                    "Sending message to client with ID {client_id:?} with no associated client key"
                );
                continue;
            };

            let _ = server.send(
                client_key.clone(),
                payload,
                LaneIndex::from_raw(u64::from(channel_id)),
            );
        }

        if let Err(error) = server.flush() {
            warn!("Failed to flush data: {:#}", pretty_error(&error));
        }
    }
}
