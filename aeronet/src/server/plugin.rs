use std::{fmt::Debug, marker::PhantomData, time::Instant};

use bevy::prelude::*;
use derivative::Derivative;

use crate::{ClientKey, ServerEvent, ServerTransport, TransportProtocol};

pub fn server_transport_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol,
    T: ServerTransport<P> + Resource,
{
    app.add_event::<RemoteConnecting>()
        .add_event::<RemoteConnected>()
        .add_event::<RemoteDisconnected<P, T>>()
        .add_event::<FromClient<P>>()
        .add_systems(PreUpdate, recv::<P, T>);
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct ServerTransportPlugin<P, T> {
    #[derivative(Debug = "ignore")]
    _phantom_p: PhantomData<P>,
    #[derivative(Debug = "ignore")]
    _phantom_t: PhantomData<T>,
}

impl<P, T> Plugin for ServerTransportPlugin<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P> + Resource,
{
    fn build(&self, app: &mut App) {
        server_transport_plugin::<P, T>(app);
    }
}

#[derive(Debug, Clone, Event)]
pub struct RemoteConnecting {
    pub client: ClientKey,
}

#[derive(Debug, Clone, Event)]
pub struct RemoteConnected {
    pub client: ClientKey,
}

#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct RemoteDisconnected<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    pub client: ClientKey,
    pub reason: T::Error,
}

#[derive(Derivative, Event)]
#[derivative(Debug(bound = "P::C2S: Debug"), Clone(bound = "P::C2S: Clone"))]
pub struct FromClient<P>
where
    P: TransportProtocol,
{
    pub client: ClientKey,
    pub msg: P::C2S,
    pub at: Instant,
}

#[allow(clippy::too_many_arguments)]
fn recv<P, T>(
    mut server: ResMut<T>,
    mut connecting: EventWriter<RemoteConnecting>,
    mut connected: EventWriter<RemoteConnected>,
    mut disconnected: EventWriter<RemoteDisconnected<P, T>>,
    mut recv: EventWriter<FromClient<P>>,
) where
    P: TransportProtocol,
    T: ServerTransport<P> + Resource,
{
    for event in server.update() {
        match event {
            ServerEvent::Connecting { client } => connecting.send(RemoteConnecting { client }),
            ServerEvent::Connected { client } => connected.send(RemoteConnected { client }),
            ServerEvent::Disconnected { client, reason } => {
                disconnected.send(RemoteDisconnected { client, reason })
            }
            ServerEvent::Recv { client, msg, at } => recv.send(FromClient { client, msg, at }),
        }
    }
}
