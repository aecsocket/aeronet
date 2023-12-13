use std::marker::PhantomData;

use bevy::prelude::*;
use derivative::Derivative;

use crate::{Protocol, ServerEvent, TransportServer};

/// Provides systems to send commands to, and receive events from, a
/// [`TransportServer`].
///
/// To use a struct version of this plugin, see [`TransportServerPlugin`].
///
/// With this plugin added, the transport `T` will receive data and update its
/// state on [`PreUpdate`], and send out messages triggered by the app on
/// [`PostUpdate`]. This is controlled by the [`TransportServerSet`].
///
/// This plugin emits the events:
/// * [`RemoteClientConnected`]
/// * [`FromClient`]
/// * [`RemoteClientDisconnected`]
///
/// ...and consumes the events:
/// * [`ToClient`]
/// * [`DisconnectRemoteClient`]
///
/// Note that errors during operation will be silently ignored, e.g. if you
/// attempt to send a message to an unconnected client.
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
        .add_systems(PreUpdate, recv::<P, T>.in_set(TransportServerSet::Recv))
        .add_systems(PostUpdate, send::<P, T>.in_set(TransportServerSet::Send));
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

/// A client has fully connected to this server.
///
/// Use this event to do client setup logic, e.g. start loading player data.
///
/// See [`ServerEvent::Connected`].
#[derive(Debug, Clone, Event)]
pub struct RemoteClientConnected<P, T>
where
    P: Protocol,
    T: TransportServer<P>,
{
    /// The key of the connected client.
    pub client: T::Client,
}

/// A client sent a message to this server.
///
/// See [`ServerEvent::Recv`].
#[derive(Debug, Clone, Event)]
pub struct FromClient<P, T>
where
    P: Protocol,
    T: TransportServer<P>,
{
    /// The key of the client which sent the message.
    pub client: T::Client,
    /// The message received.
    pub msg: P::C2S,
}

/// A client has lost connection from this server, which cannot be recovered
/// from.
///
/// Use this event to do client teardown logic, e.g. removing the player
/// from the world.
///
/// See [`ServerEvent::Disconnected`].
#[derive(Debug, Clone, Event)]
pub struct RemoteClientDisconnected<P, T>
where
    P: Protocol,
    T: TransportServer<P>,
{
    /// The key of the client.
    pub client: T::Client,
    /// The reason why the client lost connection.
    pub cause: T::Error,
}

/// Sends a message along the server to a client.
///
/// See [`TransportServer::send`].
#[derive(Debug, Clone, Event)]
pub struct ToClient<P, T>
where
    P: Protocol,
    T: TransportServer<P>,
{
    /// The key of the client to send to.
    pub client: T::Client,
    /// The message to send.
    pub msg: P::S2C,
}

/// Forcefully disconnects a client from this server.
///
/// See [`TransportServer::disconnect`].
#[derive(Debug, Clone, Event)]
pub struct DisconnectRemoteClient<P, T>
where
    P: Protocol,
    T: TransportServer<P>,
{
    /// The key of the client to disconnect.
    pub client: T::Client,
}

// systems

fn recv<P, T>(
    mut server: ResMut<T>,
    mut connected: EventWriter<RemoteClientConnected<P, T>>,
    mut recv: EventWriter<FromClient<P, T>>,
    mut disconnected: EventWriter<RemoteClientDisconnected<P, T>>,
) where
    P: Protocol,
    P::S2C: Clone,
    T: TransportServer<P> + Resource,
{
    for event in server.recv() {
        match event.into() {
            None => {}
            Some(ServerEvent::Connected { client }) => {
                connected.send(RemoteClientConnected { client })
            }
            Some(ServerEvent::Recv { client, msg }) => recv.send(FromClient { client, msg }),
            Some(ServerEvent::Disconnected { client, cause }) => {
                disconnected.send(RemoteClientDisconnected { client, cause })
            }
        }
    }
}

fn send<P, T>(
    mut server: ResMut<T>,
    mut send: EventReader<ToClient<P, T>>,
    mut disconnect: EventReader<DisconnectRemoteClient<P, T>>,
) where
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
