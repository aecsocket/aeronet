use std::{fmt::Debug, marker::PhantomData, time::Instant};

use bevy::prelude::*;
use derivative::Derivative;

use crate::{ClientEvent, ClientTransport, TransportProtocol};

pub fn client_transport_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    app.add_event::<LocalConnecting>()
        .add_event::<LocalConnected>()
        .add_event::<LocalDisconnected<P, T>>()
        .add_event::<FromServer<P>>()
        .add_systems(PreUpdate, recv::<P, T>);
}

#[derive(Derivative)]
#[derivative(Debug(bound = ""), Clone(bound = ""), Default(bound = ""))]
pub struct ClientTransportPlugin<P, T> {
    #[derivative(Debug = "ignore")]
    _phantom_p: PhantomData<P>,
    #[derivative(Debug = "ignore")]
    _phantom_t: PhantomData<T>,
}

impl<P, T> Plugin for ClientTransportPlugin<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    fn build(&self, app: &mut App) {
        client_transport_plugin::<P, T>(app);
    }
}

#[derive(Debug, Clone, Event)]
pub struct LocalConnecting;

#[derive(Debug, Clone, Event)]
pub struct LocalConnected;

#[derive(Derivative, Event)]
#[derivative(Debug(bound = "T::Error: Debug"), Clone(bound = "T::Error: Clone"))]
pub struct LocalDisconnected<P, T>
where
    P: TransportProtocol,
    T: ClientTransport<P>,
{
    pub reason: T::Error,
}

#[derive(Derivative, Event)]
#[derivative(Debug(bound = "P::Recv: Debug"), Clone(bound = "P::Recv: Clone"))]
pub struct FromServer<P>
where
    P: TransportProtocol,
{
    pub msg: P::Recv,
    pub at: Instant,
}

#[allow(clippy::too_many_arguments)]
fn recv<P, T>(
    mut client: ResMut<T>,
    mut connecting: EventWriter<LocalConnecting>,
    mut connected: EventWriter<LocalConnected>,
    mut disconnected: EventWriter<LocalDisconnected<P, T>>,
    mut recv: EventWriter<FromServer<P>>,
) where
    P: TransportProtocol,
    T: ClientTransport<P> + Resource,
{
    for event in client.update() {
        match event {
            ClientEvent::Connecting => connecting.send(LocalConnecting),
            ClientEvent::Connected => connected.send(LocalConnected),
            ClientEvent::Disconnected { reason } => disconnected.send(LocalDisconnected { reason }),
            ClientEvent::Recv { msg, at } => recv.send(FromServer { msg, at }),
        }
    }
}
