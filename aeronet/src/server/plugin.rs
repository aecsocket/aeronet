use std::marker::PhantomData;

use bevy::prelude::*;
use derivative::Derivative;

use crate::{TransportServer, ServerEvent, Protocol};

/// Provides systems to send commands to, and receive events from, a
/// [`TransportServer`].
pub fn transport_server_plugin<P, T>(app: &mut App)
where
    P: Protocol,
    P::S2C: Clone,
    T: TransportServer<P> + Resource,
{
    app.configure_sets(PreUpdate, TransportServerSet::Recv)
        .configure_sets(PostUpdate, TransportServerSet::Send)
        .add_event::<RemoteClientConnected<P, T>>()
        .add_event::<FromClient<P, T>>()
        .add_event::<RemoteClientDisconnected<P, T>>()
        .add_event::<ToClient<P, T>>()
        .add_event::<DisconnectRemoteClient<P, T>>()
        .add_systems(
            PreUpdate,
            recv::<P, T>.in_set(TransportServerSet::Recv),
        )
        .add_systems(
            PostUpdate,
            send::<P, T>.in_set(TransportServerSet::Send),
        );
}

/// Provides systems to send commands to, and receive events from, a
/// [`TransportServer`].
/// 
/// See [`transport_server_plugin`].
#[derive(Derivative)]
#[derivative(Debug, Default)]
pub struct TransportServerPlugin<P, T>
where
    P: Protocol,
    P::S2C: Clone,
    T: TransportServer<P> + Resource,
{
    #[derivative(Debug = "ignore")]
    _phantom_p: PhantomData<P>,
    #[derivative(Debug = "ignore")]
    _phantom_t: PhantomData<T>,
}

impl<P, T> Plugin for TransportServerPlugin<P, T>
where
    P: Protocol,
    P::S2C: Clone,
    T: TransportServer<P> + Resource,
{
    fn build(&self, app: &mut App) {
        transport_server_plugin::<P, T>(app);
    }
}

/// Group of systems for sending and receiving data to/from a
/// [`TransportServer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportServerSet {
    /// Receiving data from client connections and updating the server's
    /// internal state.
    Recv,
    /// Sending out messages and commands requested by the app.
    Send,
}

#[derive(Debug, Clone, Event)]
pub struct RemoteClientConnected<P: Protocol, T: TransportServer<P>> {
    pub client: T::Client,
}

#[derive(Debug, Clone, Event)]
pub struct FromClient<P: Protocol, T: TransportServer<P>> {
    pub client: T::Client,
    pub msg: P::C2S,
}

#[derive(Debug, Clone, Event)]
pub struct RemoteClientDisconnected<P: Protocol, T: TransportServer<P>> {
    pub client: T::Client,
    pub cause: T::Error,
}

#[derive(Debug, Clone, Event)]
pub struct ToClient<P: Protocol, T: TransportServer<P>> {
    pub client: T::Client,
    pub msg: P::S2C,
}

#[derive(Debug, Clone, Event)]
pub struct DisconnectRemoteClient<P: Protocol, T: TransportServer<P>> {
    pub client: T::Client,
}

// systems

fn recv<P, T>(
    mut server: ResMut<T>,
    mut connected: EventWriter<RemoteClientConnected<P, T>>,
    mut recv: EventWriter<FromClient<P, T>>,
    mut disconnected: EventWriter<RemoteClientDisconnected<P, T>>,
)
where
    P: Protocol,
    P::S2C: Clone,
    T: TransportServer<P> + Resource,
{
    for event in server.recv() {
        match event.into() {
            None => {},
            Some(ServerEvent::Connected { client }) => connected.send(RemoteClientConnected { client }),
            Some(ServerEvent::Recv { client, msg }) => recv.send(FromClient { client, msg }),
            Some(ServerEvent::Disconnected { client, cause }) => disconnected.send(RemoteClientDisconnected { client, cause }),
        }
    }
}

fn send<P, T>(
    mut server: ResMut<T>,
    mut send: EventReader<ToClient<P, T>>,
    mut disconnect: EventReader<DisconnectRemoteClient<P, T>>,
)
where
    P: Protocol,
    P::S2C: Clone,
    T: TransportServer<P> + Resource,
{
    for ToClient { client, msg } in send.read() {
        let _ = server.send(client.clone(), msg.clone());
    }

    for DisconnectRemoteClient { client } in disconnect.read() {
        let _ = server.disconnect(client.clone());
    }
}
