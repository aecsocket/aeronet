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
use bevy_ecs::{prelude::*, system::SystemParam};
use bevy_replicon::{
    client::ClientSet,
    prelude::{RepliconClient, RepliconClientStatus},
    server::ServerSet,
};
use bevy_time::prelude::*;
use derivative::Derivative;
use tracing::{trace, warn};

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
                    Self::update_state.run_if(resource_exists::<RepliconClient>),
                )
                    .chain()
                    .in_set(ServerSet::ReceivePackets),
            )
            .add_systems(
                PostUpdate,
                Self::flush
                    .run_if(client_connected::<T>.and_then(resource_exists::<RepliconClient>))
                    .in_set(ServerSet::SendPackets),
            );
    }
}

#[derive(SystemParam)]
struct Events<'w, T: ClientTransport + Resource> {
    connected: EventWriter<'w, LocalClientConnected<T>>,
    disconnected: EventWriter<'w, LocalClientDisconnected<T>>,
}

impl<T: ClientTransport + Resource> RepliconClientPlugin<T> {
    fn recv(
        time: Res<Time>,
        mut client: ResMut<T>,
        mut replicon: ResMut<RepliconClient>,
        mut events: Events<T>,
    ) {
        let mut bytes_recv = 0usize;
        for event in client.poll(time.delta()) {
            match event {
                ClientEvent::Connected => {
                    events.connected.send(LocalClientConnected::default());
                }
                ClientEvent::Disconnected { reason } => {
                    events.disconnected.send(LocalClientDisconnected { reason });
                }
                ClientEvent::Recv { msg, lane } => {
                    let Ok(channel) = u8::try_from(lane.into_raw()) else {
                        warn!(
                            "Received message on {lane:?}, which is not a valid Replicon channel"
                        );
                        continue;
                    };
                    bytes_recv = bytes_recv.saturating_add(msg.len());
                    replicon.insert_received(channel, msg);
                }
                ClientEvent::Ack { .. } | ClientEvent::Nack { .. } => {}
            }
        }

        if bytes_recv > 0 {
            trace!(bytes_recv, dt = debug(time.delta()), "Received messages");
        }
    }

    fn update_state(client: Option<Res<T>>, mut replicon: ResMut<RepliconClient>) {
        let status = client.map_or(RepliconClientStatus::Disconnected, |client| {
            match client.state() {
                ClientState::Disconnected => RepliconClientStatus::Disconnected,
                ClientState::Connecting(_) => RepliconClientStatus::Connecting,
                ClientState::Connected(_) => RepliconClientStatus::Connected { client_id: None },
            }
        });

        if status != replicon.status() {
            replicon.set_status(status);
        }
    }

    fn flush(mut client: ResMut<T>, mut replicon: ResMut<RepliconClient>) {
        let mut bytes_sent = 0usize;
        for (channel_id, payload) in replicon.drain_sent() {
            bytes_sent = bytes_sent.saturating_add(payload.len());
            let _ = client.send(payload, LaneIndex::from_raw(u64::from(channel_id)));
        }

        if let Err(error) = client.flush() {
            warn!("Failed to flush data: {:#}", pretty_error(&error));
        }

        if bytes_sent > 0 {
            trace!(bytes_sent, "Flushed messages");
        }
    }
}
