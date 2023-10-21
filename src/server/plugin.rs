use std::marker::PhantomData;

use bevy::prelude::*;

use crate::Message;

use super::{ClientId, ServerEvent, ServerTransport, SessionError};

/// Handles [`ServerTransport`]s of type `T`.
///
/// This handles receiving data from the transport and forwarding it to the app via events,
/// as well as sending data to the transport by reading from events. The events provided are:
/// * Incoming
///   * [`RemoteClientConnected`] when a client fully connects
///     * Use this to run logic when a client fully connects e.g. loading player data
///   * [`FromClient`] when a client sends data to the server
///   * [`RemoteClientDisconnected`] when a client loses connection
///     * Use this to run logic when a client is dropped
/// * Outgoing
///   * [`ToClient`] to send a message to a client
///   * [`DisconnectClient`] to force a client to lose the connection
///
/// # Usage
///
/// This plugin is not *required* to use the server transports. Using the plugin actually poses
/// the following limitations:
/// * You do not get ownership of incoming messages
///   * This means you are unable to mutate the messages before sending them to the rest of the app
///     via [`FromClient`]
/// * The transport implementation must implement [`Resource`]
///   * All inbuilt transports implement [`Resource`]
///
/// If these are unsuitable for your use case, consider manually using the transport APIs from your
/// app, bypassing the plugin altogether.
/// ```
/// use bevy::prelude::*;
/// use aeronet::ServerTransportPlugin;
///
/// # use aeronet::{Message, ServerTransport};
/// # fn run<C2S: Message, S2C: Message + Clone, MyTransportImpl: ServerTransport<C2S, S2C> + Resource>() {
/// App::new()
///     .add_plugins(ServerTransportPlugin::<C2S, S2C, MyTransportImpl>::default());
/// # }
/// ```
#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct ServerTransportPlugin<C2S, S2C, T> {
    _phantom_c2s: PhantomData<C2S>,
    _phantom_s2c: PhantomData<S2C>,
    _phantom_t: PhantomData<T>,
}

impl<C2S, S2C, T> Plugin for ServerTransportPlugin<C2S, S2C, T>
where
    C2S: Message,
    S2C: Message + Clone,
    T: ServerTransport<C2S, S2C> + Resource,
{
    fn build(&self, app: &mut App) {
        app.add_event::<RemoteClientConnected>()
            .add_event::<FromClient<C2S>>()
            .add_event::<RemoteClientDisconnected>()
            .add_event::<ToClient<S2C>>()
            .add_event::<DisconnectClient>()
            .configure_set(
                PreUpdate,
                ServerTransportSet::Recv.run_if(resource_exists::<T>()),
            )
            .configure_set(
                PostUpdate,
                ServerTransportSet::Send.run_if(resource_exists::<T>()),
            )
            .add_systems(
                PreUpdate,
                recv::<C2S, S2C, T>.in_set(ServerTransportSet::Recv),
            )
            .add_systems(
                PostUpdate,
                send::<C2S, S2C, T>.in_set(ServerTransportSet::Send),
            );
    }
}

/// A system set for server transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerTransportSet {
    /// Receives events from connected clients and forwards it to the app.
    Recv,
    /// Sends requests from the app to connected clients.
    Send,
}

/// See [`ServerEvent::Connected`].
#[derive(Debug, Clone, Event)]
pub struct RemoteClientConnected {
    /// See [`ServerEvent::Connected::client`].
    pub client: ClientId,
}

/// See [`ServerEvent::Recv`].
#[derive(Debug, Clone, Event)]
pub struct FromClient<C2S> {
    /// See [`ServerEvent::Recv::client`].
    pub client: ClientId,
    /// See [`ServerEvent::Recv::msg`].
    pub msg: C2S,
}

/// See [`ServerEvent::Disconnected`].
#[derive(Debug, Event)]
pub struct RemoteClientDisconnected {
    /// See [`ServerEvent::Disconnected::client`].
    pub client: ClientId,
    /// See [`ServerEvent::Disconnected::reason`].
    pub reason: SessionError,
}

/// Sends a message to a connected client using [`ServerTransport::send`].
#[derive(Debug, Event)]
pub struct ToClient<S2C> {
    /// The ID of the client to send to.
    pub client: ClientId,
    /// The message to send.
    pub msg: S2C,
}

/// Forcefully disconnects a client using [`ServerTransport::disconnect`].
#[derive(Debug, Clone, Event)]
pub struct DisconnectClient {
    /// The ID of the client to disconnect.
    pub client: ClientId,
}

fn recv<C2S, S2C, T>(
    mut server: ResMut<T>,
    mut connected: EventWriter<RemoteClientConnected>,
    mut from_client: EventWriter<FromClient<C2S>>,
    mut disconnected: EventWriter<RemoteClientDisconnected>,
) where
    C2S: Message,
    S2C: Message,
    T: ServerTransport<C2S, S2C> + Resource,
{
    server.recv();
    for event in server.take_events() {
        match event {
            ServerEvent::Connected { client } => {
                connected.send(RemoteClientConnected { client });
            }
            ServerEvent::Recv { client, msg } => {
                from_client.send(FromClient { client, msg });
            }
            ServerEvent::Disconnected { client, reason } => {
                disconnected.send(RemoteClientDisconnected { client, reason });
            }
        }
    }
}

fn send<C2S, S2C, T>(
    mut server: ResMut<T>,
    mut to_client: EventReader<ToClient<S2C>>,
    mut disconnect: EventReader<DisconnectClient>,
) where
    C2S: Message,
    S2C: Message + Clone,
    T: ServerTransport<C2S, S2C> + Resource,
{
    for ToClient { client, msg } in to_client.iter() {
        server.send(*client, msg.clone());
    }

    for DisconnectClient { client } in disconnect.iter() {
        server.disconnect(*client);
    }
}
