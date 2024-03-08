use std::{fmt::Debug, marker::PhantomData};

use aeronet::{
    protocol::TransportProtocol,
    server::{
        RemoteClientConnected, RemoteClientConnecting, RemoteClientDisconnected, ServerClosed,
        ServerEvent, ServerOpened, ServerState, ServerTransport, ServerTransportSet,
    },
};
use ahash::AHashMap;
use bevy::prelude::*;
use bevy_replicon::{
    core::ClientId,
    server::{replicon_server::RepliconServer, ServerSet},
};
use derivative::Derivative;

use crate::protocol::RepliconMessage;

// this REPLACES ClientTransportPlugin btw
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct RepliconAeronetServerPlugin<P, T> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<(P, T)>,
}

pub fn replicon_aeronet_server_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ServerTransport<P> + Resource,
{
    RepliconAeronetServerPlugin::<P, T>::default().build(app)
}

#[derive(Derivative, Resource)]
#[derivative(
    Debug(bound = "T::ClientKey: Debug"),
    Clone(bound = "T::ClientKey: Clone")
)]
pub struct ClientKeys<P: TransportProtocol, T: ServerTransport<P>> {
    pub to_id: AHashMap<T::ClientKey, ClientId>,
    pub next_id: ClientId,
}

impl<P: TransportProtocol, T: ServerTransport<P>> Default for ClientKeys<P, T> {
    fn default() -> Self {
        Self {
            to_id: AHashMap::new(),
            next_id: ClientId::new(0),
        }
    }
}

impl<P, T> Plugin for RepliconAeronetServerPlugin<P, T>
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ServerTransport<P> + Resource,
{
    fn build(&self, app: &mut App) {
        app.init_resource::<ClientKeys<P, T>>()
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
                    .run_if(client_connected::<P, T>)
                    .in_set(ServerSet::SendPackets),
            );
    }
}

type RepliconEvent = bevy_replicon::server::ServerEvent;

impl<P, T> RepliconAeronetServerPlugin<P, T>
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ServerTransport<P> + Resource,
{
    fn recv(mut server: ResMut<T>) {}

    fn update_state(server: Res<T>, mut replicon: ResMut<RepliconServer>) {
        replicon.set_running(match server.state() {
            ServerState::Closed | ServerState::Opening(_) => false,
            ServerState::Open(_) => true,
        });
    }

    fn on_removed(mut replicon: ResMut<RepliconServer>) {
        replicon.set_running(false);
    }

    fn forward_events(
        mut server: ResMut<T>,
        mut replicon_server: ResMut<RepliconServer>,
        mut client_keys: ResMut<ClientKeys<P, T>>,
        mut replicon_events: EventWriter<bevy_replicon::server::ServerEvent>,
        mut opened: EventWriter<ServerOpened<P, T>>,
        mut closed: EventWriter<ServerClosed<P, T>>,
        mut connecting: EventWriter<RemoteClientConnecting<P, T>>,
        mut connected: EventWriter<RemoteClientConnected<P, T>>,
        mut disconnected: EventWriter<RemoteClientDisconnected<P, T>>,
    ) {
        for event in server.poll() {
            match event {
                ServerEvent::Opened => {
                    opened.send(ServerOpened {
                        _phantom: PhantomData,
                    });
                }
                ServerEvent::Closed { reason } => {
                    closed.send(ServerClosed { reason });
                }
                ServerEvent::Connecting { client_key } => {
                    connecting.send(RemoteClientConnecting { client_key });
                }
                ServerEvent::Connected { client_key } => {
                    let client_id = client_keys.next_id;
                    client_keys.next_id = ClientId::new(client_id.get().wrapping_add(1));
                    client_keys.to_id.insert(client_key, client_id);

                    connected.send(RemoteClientConnected { client_key });
                    replicon_events.send(RepliconEvent::ClientConnected { client_id: todo!() });
                }
                ServerEvent::Disconnected { client_key, reason } => {
                    disconnected.send(RemoteClientDisconnected { client_key, reason });
                    replicon_events.send(RepliconEvent::ClientDisconnected {
                        client_id: todo!(),
                        reason: format!("{:#}", aeronet::error::pretty_error(&reason)),
                    })
                }
                ServerEvent::Recv { client_key, msg } => {
                    replicon_server.insert_received(todo!(), msg.channel_id, msg.payload);
                }
                ServerEvent::Ack { .. } => {}
            }
        }
    }
}
