use std::marker::PhantomData;

use aeronet::{
    client::{ClientState, ClientTransport, ClientTransportSet},
    protocol::TransportProtocol,
};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_replicon::client::{
    replicon_client::{RepliconClient, RepliconClientStatus},
    ClientSet,
};
use derivative::Derivative;

pub fn replicon_aeronet_client_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    app.configure_sets(
        PreUpdate,
        ClientSet::ReceivePackets.after(ClientTransportSet::Recv),
    )
    .configure_sets(
        PostUpdate,
        ClientSet::SendPackets.before(ClientTransportSet::Send),
    )
    .add_systems(PreUpdate, update_state::<P, T>.run_if(resource_exists::<T>));
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct RepliconAeronetClientPlugin<P, T> {
    #[derivative(Debug = "ignore")]
    _phantom: PhantomData<(P, T)>,
}

impl<P, T> Plugin for RepliconAeronetClientPlugin<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    fn build(&self, app: &mut App) {
        replicon_aeronet_client_plugin::<P, T>(app);
    }
}

fn update_state<P, T>(client: Res<T>, mut replicon: ResMut<RepliconClient>)
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    replicon.set_status(match client.state() {
        ClientState::Disconnected => RepliconClientStatus::Disconnected,
        ClientState::Connecting(_) => RepliconClientStatus::Connecting,
        ClientState::Connected(_) => RepliconClientStatus::Connected { client_id: None },
    })
}
