use std::marker::PhantomData;

use bevy::prelude::*;
use derivative::Derivative;

use crate::{Message, TransportServer, ServerEvent};

/// Provides systems to send commands to, and receive events from, a
/// [`TransportServer`].
pub fn transport_server_plugin<C2S, S2C, T>(app: &mut App)
where
    C2S: Message,
    S2C: Message + Clone,
    T: TransportServer<C2S, S2C> + Resource,
{
    app.configure_sets(PreUpdate, TransportServerSet::Recv)
        .configure_sets(PostUpdate, TransportServerSet::Send)
        .add_event::<RemoteClientConnected<T::Client>>()
        .add_event::<FromClient<T::Client, C2S>>()
        .add_event::<RemoteClientDisconnected<T::Client, T::Error>>()
        .add_event::<ToClient<T::Client, S2C>>()
        .add_event::<DisconnectRemoteClient<T::Client>>()
        .add_systems(
            PreUpdate,
            recv::<C2S, S2C, T>.in_set(TransportServerSet::Recv),
        )
        .add_systems(
            PostUpdate,
            send::<C2S, S2C, T>.in_set(TransportServerSet::Send),
        );
}

/// Provides systems to send commands to, and receive events from, a
/// [`TransportServer`].
/// 
/// See [`transport_server_plugin`].
#[derive(Derivative)]
#[derivative(Debug, Default)]
pub struct TransportServerPlugin<C2S, S2C, T>
where
    C2S: Message,
    S2C: Message + Clone,
    T: TransportServer<C2S, S2C> + Resource,
{
    #[derivative(Debug = "ignore")]
    _phantom_c2s: PhantomData<C2S>,
    #[derivative(Debug = "ignore")]
    _phantom_s2c: PhantomData<S2C>,
    #[derivative(Debug = "ignore")]
    _phantom_t: PhantomData<T>,
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
pub struct RemoteClientConnected<C> {
    pub client: C,
}

#[derive(Debug, Clone, Event)]
pub struct FromClient<C, C2S> {
    pub client: C,
    pub msg: C2S,
}

#[derive(Debug, Clone, Event)]
pub struct RemoteClientDisconnected<C, E> {
    pub client: C,
    pub cause: E,
}

#[derive(Debug, Clone, Event)]
pub struct ToClient<C, S2C> {
    pub client: C,
    pub msg: S2C,
}

#[derive(Debug, Clone, Event)]
pub struct DisconnectRemoteClient<C> {
    pub client: C,
}

// systems

fn recv<C2S, S2C, T>(
    mut server: ResMut<T>,
    mut connected: EventWriter<RemoteClientConnected<T::Client>>,
    mut recv: EventWriter<FromClient<T::Client, C2S>>,
    mut disconnected: EventWriter<RemoteClientDisconnected<T::Client, T::Error>>,
)
where
    C2S: Message,
    S2C: Message + Clone,
    T: TransportServer<C2S, S2C> + Resource,
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

fn send<C2S, S2C, T>(
    mut server: ResMut<T>,
    mut send: EventReader<ToClient<T::Client, S2C>>,
    mut disconnect: EventReader<DisconnectRemoteClient<T::Client>>,
)
where
    C2S: Message,
    S2C: Message + Clone,
    T: TransportServer<C2S, S2C> + Resource,
{
    for ToClient { client, msg } in send.read() {
        let _ = server.send(client.clone(), msg.clone());
    }

    for DisconnectRemoteClient { client } in disconnect.read() {
        let _ = server.disconnect(client.clone());
    }
}
