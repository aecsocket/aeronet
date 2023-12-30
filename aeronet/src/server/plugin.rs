use std::{fmt::Debug, marker::PhantomData, time::Instant};

use bevy::prelude::*;
use derivative::Derivative;

use crate::{ClientKey, MessageTicket, ServerEvent, ServerTransport, TransportProtocol};

pub fn server_transport_plugin<P, T>(app: &mut App)
where
    P: TransportProtocol,
    T: ServerTransport<P> + Resource,
{
    app.add_event::<ServerOpening>()
        .add_event::<ServerOpened>()
        .add_event::<ServerClosed<P, T>>()
        .add_event::<RemoteConnecting>()
        .add_event::<RemoteConnected>()
        .add_event::<RemoteDisconnected<P, T>>()
        .add_event::<FromClient<P>>()
        .add_event::<ServerAck>()
        .add_event::<ServerNack>()
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
pub struct ServerOpening;

#[derive(Debug, Clone, Event)]
pub struct ServerOpened;

#[derive(Debug, Clone, Event)]
pub struct ServerClosed<P, T>
where
    P: TransportProtocol,
    T: ServerTransport<P>,
{
    pub reason: T::Error,
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
#[derivative(Debug(bound = "P::Recv: Debug"), Clone(bound = "P::Recv: Clone"))]
pub struct FromClient<P>
where
    P: TransportProtocol,
{
    pub client: ClientKey,
    pub msg: P::Recv,
    pub at: Instant,
}

#[derive(Debug, Clone, Event)]
pub struct ServerAck {
    pub client: ClientKey,
    pub msg: MessageTicket,
    pub at: Instant,
}

#[derive(Debug, Clone, Event)]
pub struct ServerNack {
    pub client: ClientKey,
    pub msg: MessageTicket,
    pub at: Instant,
}

#[allow(clippy::too_many_arguments)]
fn recv<P, T>(
    mut client: ResMut<T>,
    mut opening: EventWriter<ServerOpening>,
    mut opened: EventWriter<ServerOpened>,
    mut closed: EventWriter<ServerClosed<P, T>>,
    mut connecting: EventWriter<RemoteConnecting>,
    mut connected: EventWriter<RemoteConnected>,
    mut disconnected: EventWriter<RemoteDisconnected<P, T>>,
    mut recv: EventWriter<FromClient<P>>,
    mut ack: EventWriter<ServerAck>,
    mut nack: EventWriter<ServerNack>,
) where
    P: TransportProtocol,
    T: ServerTransport<P> + Resource,
{
    for event in client.update() {
        match event {
            ServerEvent::Opening => opening.send(ServerOpening),
            ServerEvent::Opened => opened.send(ServerOpened),
            ServerEvent::Closed { reason } => closed.send(ServerClosed { reason }),
            ServerEvent::Connecting { client } => connecting.send(RemoteConnecting { client }),
            ServerEvent::Connected { client } => connected.send(RemoteConnected { client }),
            ServerEvent::Disconnected { client, reason } => {
                disconnected.send(RemoteDisconnected { client, reason })
            }
            ServerEvent::Recv { client, msg, at } => recv.send(FromClient { client, msg, at }),
            ServerEvent::Ack { client, msg, at } => ack.send(ServerAck { client, msg, at }),
            ServerEvent::Nack { client, msg, at } => nack.send(ServerNack { client, msg, at }),
        }
    }
}
