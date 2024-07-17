//! Client-side traits and items.

use std::marker::PhantomData;

use aeronet::{
    client::{
        client_connected, ClientEvent, ClientState, ClientTransport, ClientTransportSet,
        LocalClientConnected, LocalClientDisconnected,
    },
    error::pretty_error,
    lane::LaneIndex,
};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_replicon::{
    client::{
        replicon_client::{RepliconClient, RepliconClientStatus},
        ClientSet,
    },
    server::ServerSet,
};
use bevy_time::prelude::*;
use derivative::Derivative;
use tracing::warn;

/// Provides a [`bevy_replicon`] client backend using the given [`aeronet`]
/// transport.
///
/// You must use [`RepliconClientPlugin`] and Replicon's [`ClientPlugin`]
/// together.
///
/// System sets:
/// * [`ClientTransportSet::Recv`]
/// * [`ClientTransportSet::Send`]
///
/// Events:
/// * [`LocalClientConnected`]
/// * [`LocalClientDisconnected`]
///
/// [`ClientPlugin`]: bevy_replicon::client::ClientPlugin
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct RepliconClientPlugin<T> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<T>,
}

impl<T: ClientTransport + Resource> Plugin for RepliconClientPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_event::<LocalClientConnected<T>>()
            .add_event::<LocalClientDisconnected<T>>()
            .configure_sets(
                PreUpdate,
                (
                    ClientTransportSet::Recv,
                    ClientSet::ReceivePackets.after(ClientTransportSet::Recv),
                ),
            )
            .configure_sets(
                PostUpdate,
                (
                    ClientSet::SendPackets,
                    ClientTransportSet::Send.after(ClientSet::SendPackets),
                ),
            )
            .add_systems(
                PreUpdate,
                (
                    Self::recv
                        .run_if(resource_exists::<T>.and_then(resource_exists::<RepliconClient>)),
                    Self::update_state
                        .run_if(resource_exists::<T>.and_then(resource_exists::<RepliconClient>)),
                    Self::on_removed.run_if(
                        resource_removed::<T>().and_then(resource_exists::<RepliconClient>),
                    ),
                )
                    .chain()
                    .in_set(ServerSet::ReceivePackets),
            )
            .add_systems(
                PostUpdate,
                Self::send
                    .run_if(client_connected::<T>.and_then(resource_exists::<RepliconClient>))
                    .in_set(ServerSet::SendPackets),
            );
    }
}

impl<T: ClientTransport + Resource> RepliconClientPlugin<T> {
    fn recv(
        time: Res<Time>,
        mut client: ResMut<T>,
        mut replicon: ResMut<RepliconClient>,
        mut connected: EventWriter<LocalClientConnected<T>>,
        mut disconnected: EventWriter<LocalClientDisconnected<T>>,
    ) {
        for event in client.poll(time.delta()) {
            match event {
                ClientEvent::Connected => {
                    connected.send(LocalClientConnected {
                        _phantom: PhantomData,
                    });
                }
                ClientEvent::Disconnected { error } => {
                    disconnected.send(LocalClientDisconnected { error });
                }
                ClientEvent::Recv { msg, lane } => {
                    let Ok(channel) = u8::try_from(lane.into_raw()) else {
                        warn!(
                            "Received message on {lane:?}, which is not a valid Replicon channel"
                        );
                        continue;
                    };
                    replicon.insert_received(channel, msg);
                }
                ClientEvent::Ack { .. } | ClientEvent::Nack { .. } => {}
            }
        }
    }

    fn update_state(client: Res<T>, mut replicon: ResMut<RepliconClient>) {
        replicon.set_status(match client.state() {
            ClientState::Disconnected => RepliconClientStatus::Disconnected,
            ClientState::Connecting(_) => RepliconClientStatus::Connecting,
            ClientState::Connected(_) => RepliconClientStatus::Connected { client_id: None },
        });
    }

    fn on_removed(mut replicon: ResMut<RepliconClient>) {
        replicon.set_status(RepliconClientStatus::Disconnected);
    }

    fn send(mut client: ResMut<T>, mut replicon: ResMut<RepliconClient>) {
        for (channel_id, payload) in replicon.drain_sent() {
            let _ = client.send(payload, LaneIndex::from_raw(usize::from(channel_id)));
        }

        if let Err(error) = client.flush() {
            warn!("Failed to flush data: {:#}", pretty_error(&error));
        }
    }
}
