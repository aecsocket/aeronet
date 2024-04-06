use std::{fmt::Debug, marker::PhantomData};

use aeronet::{
    protocol::TransportProtocol,
    server::{
        server_open, RemoteClientConnected, RemoteClientConnecting, RemoteClientDisconnected,
        ServerClosed, ServerConnectionError, ServerEvent, ServerFlushError, ServerOpened,
        ServerState, ServerTransport, ServerTransportSet,
    },
};
use bevy::prelude::*;
use bevy_replicon::{
    core::ClientId,
    server::{replicon_server::RepliconServer, ServerSet},
};
use bimap::{BiHashMap, Overwritten};
use derivative::Derivative;

use crate::protocol::RepliconMessage;

// this REPLACES ClientTransportPlugin btw
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct RepliconServerPlugin<P, T> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<(P, T)>,
}

pub fn replicon_aeronet_server_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ServerTransport<P> + Resource,
{
    RepliconServerPlugin::<P, T>::default().build(app)
}

#[derive(Derivative, Resource)]
#[derivative(
    Debug(bound = "T::ClientKey: Debug"),
    Clone(bound = "T::ClientKey: Clone")
)]
pub struct ClientKeys<P: TransportProtocol, T: ServerTransport<P>> {
    id_map: BiHashMap<T::ClientKey, ClientId, ahash::RandomState, ahash::RandomState>,
    next_id: ClientId,
}

impl<P: TransportProtocol, T: ServerTransport<P>> ClientKeys<P, T> {
    pub fn id_map(
        &self,
    ) -> &BiHashMap<T::ClientKey, ClientId, ahash::RandomState, ahash::RandomState> {
        &self.id_map
    }

    fn next_id(&mut self) -> ClientId {
        let id = self.next_id;
        self.next_id = ClientId::new(self.next_id.get().wrapping_add(1));
        id
    }
}

impl<P: TransportProtocol, T: ServerTransport<P>> Default for ClientKeys<P, T> {
    fn default() -> Self {
        Self {
            id_map: BiHashMap::with_hashers(ahash::RandomState::new(), ahash::RandomState::new()),
            // start at 1, because 0 is ClientId::SERVER
            next_id: ClientId::new(1),
        }
    }
}

impl<P, T> Plugin for RepliconServerPlugin<P, T>
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ServerTransport<P> + Resource,
{
    fn build(&self, app: &mut App) {
        app.add_event::<ServerOpened<P, T>>()
            .add_event::<ServerClosed<P, T>>()
            .add_event::<RemoteClientConnecting<P, T>>()
            .add_event::<RemoteClientConnected<P, T>>()
            .add_event::<RemoteClientDisconnected<P, T>>()
            .add_event::<ServerConnectionError<P, T>>()
            .add_event::<ServerFlushError<P, T>>()
            .init_resource::<ClientKeys<P, T>>()
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
                    ServerTransportSet::Flush,
                    ServerSet::SendPackets.before(ServerTransportSet::Flush),
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
                    .run_if(server_open::<P, T>)
                    .in_set(ServerSet::SendPackets),
            );
    }
}

type RepliconEvent = bevy_replicon::server::ServerEvent;

impl<P, T> RepliconServerPlugin<P, T>
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ServerTransport<P> + Resource,
{
    #[allow(clippy::too_many_arguments)]
    fn recv(
        time: Res<Time>,
        mut server: ResMut<T>,
        mut replicon_server: ResMut<RepliconServer>,
        mut client_keys: ResMut<ClientKeys<P, T>>,
        mut replicon_events: EventWriter<bevy_replicon::server::ServerEvent>,
        mut opened: EventWriter<ServerOpened<P, T>>,
        mut closed: EventWriter<ServerClosed<P, T>>,
        mut connecting: EventWriter<RemoteClientConnecting<P, T>>,
        mut connected: EventWriter<RemoteClientConnected<P, T>>,
        mut disconnected: EventWriter<RemoteClientDisconnected<P, T>>,
        mut errors: EventWriter<ServerConnectionError<P, T>>,
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
                            warn!("Inserted duplicate client key/ID pair: {overwritten:?}")
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
                        warn!(
                            "Disconnected client {client_key:?} which does not have a replicon ID"
                        );
                        return;
                    };
                    debug!("Removed {client_key:?} associated with {client_id:?}");
                    replicon_events.send(RepliconEvent::ClientDisconnected {
                        client_id,
                        reason: reason_str,
                    });
                }
                ServerEvent::Recv { client_key, msg } => {
                    let Some(client_id) = client_keys.id_map.get_by_left(&client_key) else {
                        warn!("Received message from client {client_key:?} which does not have a replicon ID");
                        return;
                    };
                    info!("Recv from {client_id:?} pkt {}", msg.payload.len());
                    replicon_server.insert_received(*client_id, msg.channel_id, msg.payload);
                }
                ServerEvent::Ack { .. } => {}
                ServerEvent::ConnectionError { client_key, error } => {
                    errors.send(ServerConnectionError { client_key, error });
                }
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
        client_keys: Res<ClientKeys<P, T>>,
        mut flush_errors: EventWriter<ServerFlushError<P, T>>,
    ) {
        for (client_id, channel_id, payload) in replicon.drain_sent() {
            let Some(client_key) = client_keys.id_map.get_by_right(&client_id) else {
                warn!(
                    "Sending message to client with ID {client_id:?} with no associated client key"
                );
                continue;
            };

            // ignore send failures
            let _ = server.send(
                client_key.clone(),
                RepliconMessage {
                    channel_id,
                    payload,
                },
            );
        }

        if let Err(error) = server.flush() {
            flush_errors.send(ServerFlushError { error });
        }
    }
}
