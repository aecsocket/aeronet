use std::marker::PhantomData;

use aeronet::{
    client::{
        client_connected, ClientEvent, ClientFlushError, ClientState, ClientTransport,
        ClientTransportSet, LocalClientConnected, LocalClientDisconnected,
    },
    protocol::TransportProtocol,
};
use bevy::prelude::*;
use bevy_replicon::{
    client::{
        replicon_client::{RepliconClient, RepliconClientStatus},
        ClientSet,
    },
    server::ServerSet,
};
use derivative::Derivative;

use crate::protocol::RepliconMessage;

// this REPLACES ClientTransportPlugin btw
#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct RepliconAeronetClientPlugin<P, T> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<(P, T)>,
}

impl<P, T> Plugin for RepliconAeronetClientPlugin<P, T>
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ClientTransport<P> + Resource,
{
    fn build(&self, app: &mut App) {
        app.configure_sets(
            PreUpdate,
            (
                ClientTransportSet::Recv,
                ClientSet::ReceivePackets.after(ClientTransportSet::Recv),
            ),
        )
        .configure_sets(
            PostUpdate,
            (
                ClientTransportSet::Send,
                ClientSet::SendPackets.before(ClientTransportSet::Send),
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

pub fn replicon_aeronet_client_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ClientTransport<P> + Resource,
{
    RepliconAeronetClientPlugin::<P, T>::default().build(app)
}

impl<P, T> RepliconAeronetClientPlugin<P, T>
where
    P: TransportProtocol<C2S = RepliconMessage, S2C = RepliconMessage>,
    T: ClientTransport<P> + Resource,
{
    fn recv(
        time: Res<Time>,
        mut client: ResMut<T>,
        mut replicon: ResMut<RepliconClient>,
        mut connected: EventWriter<LocalClientConnected<P, T>>,
        mut disconnected: EventWriter<LocalClientDisconnected<P, T>>,
    ) {
        for event in client.poll(time.delta()) {
            match event {
                ClientEvent::Connected => {
                    connected.send(LocalClientConnected {
                        _phantom: PhantomData,
                    });
                }
                ClientEvent::Disconnected { reason } => {
                    disconnected.send(LocalClientDisconnected { reason });
                }
                ClientEvent::Recv { msg } => {
                    replicon.insert_received(msg.channel_id, msg.payload);
                }
                ClientEvent::Ack { .. } => {}
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

    fn send(
        mut client: ResMut<T>,
        mut replicon: ResMut<RepliconClient>,
        mut flush_errors: EventWriter<ClientFlushError<P, T>>,
    ) {
        for (channel_id, payload) in replicon.drain_sent() {
            // ignore send failures
            let _ = client.send(RepliconMessage {
                channel_id,
                payload,
            });
        }

        if let Err(error) = client.flush() {
            flush_errors.send(ClientFlushError { error });
        }
    }
}
