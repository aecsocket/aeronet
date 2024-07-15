//! Server-side traits and items.

use std::{marker::PhantomData, ops::Deref};

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
use bevy_time::prelude::*;
use bimap::{BiHashMap, Overwritten};
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
                    Self::recv.run_if(resource_exists::<T>),
                    Self::update_state.run_if(resource_exists::<T>),
                    Self::on_removed.run_if(resource_removed::<T>()),
                )
                    .chain()
                    .in_set(ServerSet::ReceivePackets),
            )
            .add_systems(
                PostUpdate,
                Self::send
                    .run_if(server_open::<T>)
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
    pub fn map(&self) -> &ClientMap<T::ClientKey> {
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
        mut replicon_events: EventWriter<bevy_replicon::server::ServerEvent>,
        mut opened: EventWriter<ServerOpened<T>>,
        mut closed: EventWriter<ServerClosed<T>>,
        mut connecting: EventWriter<RemoteClientConnecting<T>>,
        mut connected: EventWriter<RemoteClientConnected<T>>,
        mut disconnected: EventWriter<RemoteClientDisconnected<T>>,
    ) {
        for event in server.poll(time.delta()) {
            match event {
                ServerEvent::Opened => {
                    opened.send(ServerOpened {
                        _phantom: PhantomData,
                    });
                }
                ServerEvent::Closed { error } => {
                    closed.send(ServerClosed { error });
                }
                ServerEvent::Connecting { client_key } => {
                    connecting.send(RemoteClientConnecting { client_key });
                }
                ServerEvent::Connected { client_key } => {
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
                ServerEvent::Disconnected { client_key, error } => {
                    let reason_str = format!("{:#}", aeronet::error::pretty_error(&error));
                    disconnected.send(RemoteClientDisconnected {
                        client_key: client_key.clone(),
                        error,
                    });

                    let Some((_, client_id)) = client_keys.id_map.remove_by_left(&client_key)
                    else {
                        warn!("Disconnected client {client_key:?} which does not have a client ID");
                        return;
                    };
                    debug!("Removed {client_key:?} associated with {client_id:?}");
                    replicon_events.send(RepliconEvent::ClientDisconnected {
                        client_id,
                        reason: reason_str,
                    });
                }
                ServerEvent::Recv {
                    client_key,
                    msg,
                    lane,
                } => {
                    let Ok(channel) = u8::try_from(lane.into_raw()) else {
                        warn!(
                            "Received message on {lane:?}, which is not a valid Replicon channel"
                        );
                        continue;
                    };
                    let Some(client_id) = client_keys.id_map.get_by_left(&client_key) else {
                        warn!("Received message from client {client_key:?} which does not have a client ID");
                        continue;
                    };
                    replicon_server.insert_received(*client_id, channel, msg);
                }
                ServerEvent::Ack { .. } | ServerEvent::Nack { .. } => {}
            }
        }
    }

    fn update_state(server: Res<T>, mut replicon: ResMut<RepliconServer>) {
        replicon.set_running(match server.state() {
            ServerState::Closed | ServerState::Opening(_) => false,
            ServerState::Open(_) => true,
        });
    }

    fn on_removed(mut replicon: ResMut<RepliconServer>) {
        replicon.set_running(false);
    }

    fn send(
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
                LaneIndex::from_raw(usize::from(channel_id)),
            );
        }

        if let Err(error) = server.flush() {
            warn!("Failed to flush data: {:#}", pretty_error(&error));
        }
    }
}
