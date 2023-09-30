//! [`bevy`] plugin for interacting with a server transport.

use std::marker::PhantomData;

use bevy::prelude::*;

use super::{
    ClientId, RecvError, ServerEvent, ServerTransport, ServerTransportConfig, SessionError,
};

/// Configures a [`ServerTransport`] of type `T` using configuration `C`.
///
/// This handles receiving data from the transport and forwarding it to the app via events,
/// as well as sending data to the transport by reading from events. The events provided are:
/// * Incoming
///   * [`ClientIncoming`] when a client requests a connection
///   * [`ClientConnected`] when a client fully connects
///     * Use this to run logic when a client fully connects e.g. loading player data
///   * [`FromClient`] when a client sends data to the server
///   * [`ClientDisconnected`] when a client loses connection
///     * Use this to run logic when a client is dropped
/// * Outgoing
///   * [`ToClient`] to send a message to a client
///   * [`DisconnectClient`] to force a client to lose the connection
///
/// # Usage
///
/// You will need an implementation of [`ServerTransportConfig`] to use as the `C` type parameter.
/// See that type's docs to see how to implement one.
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
/// use aeronet::server::plugin::TransportPlugin;
///
/// # fn run<MyTransportConfig, MyTransportImpl>()
/// # where
/// #     MyTransportConfig: aeronet::server::TransportConfig,
/// #     MyTransportImpl: aeronet::server::Transport<MyTransportConfig> + Resource,
/// # {
/// App::new()
///     .add_plugins(TransportPlugin::<MyTransportConfig, MyTransportImpl>::default());
/// # }
/// ```
#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct ServerTransportPlugin<C, T> {
    _phantom_c: PhantomData<C>,
    _phantom_t: PhantomData<T>,
}

impl<C, T> Plugin for ServerTransportPlugin<C, T>
where
    C: ServerTransportConfig,
    T: ServerTransport<C> + Resource,
{
    fn build(&self, app: &mut App) {
        app.add_event::<ClientIncoming>()
            .add_event::<ClientConnected>()
            .add_event::<FromClient<C::C2S>>()
            .add_event::<ClientDisconnected>()
            .add_event::<ToClient<C::S2C>>()
            .add_event::<DisconnectClient>()
            .configure_set(
                PreUpdate,
                ServerTransportSet::Recv.run_if(resource_exists::<T>()),
            )
            .configure_set(
                PostUpdate,
                ServerTransportSet::Send.run_if(resource_exists::<T>()),
            )
            .add_systems(PreUpdate, recv::<C, T>.in_set(ServerTransportSet::Recv))
            .add_systems(PostUpdate, send::<C, T>.in_set(ServerTransportSet::Send));
    }
}

/// A system set for transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerTransportSet {
    /// Transports receiving data and forwarding it to the app.
    Recv,
    /// Transports sending data from the app.
    Send,
}

/// See [`ServerEvent::Incoming`].
#[derive(Debug, Clone, Event)]
pub struct ClientIncoming {
    /// See [`ServerEvent::Incoming::client`].
    pub client: ClientId,
}

/// See [`ServerEvent::Connected`].
#[derive(Debug, Clone, Event)]
pub struct ClientConnected {
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
pub struct ClientDisconnected {
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

fn recv<C, T>(
    mut commands: Commands,
    mut server: ResMut<T>,
    mut requested: EventWriter<ClientIncoming>,
    mut connected: EventWriter<ClientConnected>,
    mut from_client: EventWriter<FromClient<C::C2S>>,
    mut disconnected: EventWriter<ClientDisconnected>,
) where
    C: ServerTransportConfig,
    T: ServerTransport<C> + Resource,
{
    loop {
        match server.recv() {
            Ok(ServerEvent::Incoming { client }) => {
                requested.send(ClientIncoming { client });
            }
            Ok(ServerEvent::Connected { client }) => {
                connected.send(ClientConnected { client });
            }
            Ok(ServerEvent::Recv { client, msg }) => {
                from_client.send(FromClient { client, msg });
            }
            Ok(ServerEvent::Disconnected { client, reason }) => {
                disconnected.send(ClientDisconnected { client, reason });
            }
            Err(RecvError::Empty) => break,
            Err(RecvError::Closed) => {
                commands.remove_resource::<T>();
                break;
            }
        }
    }
}

fn send<C, T>(
    mut server: ResMut<T>,
    mut to_client: EventReader<ToClient<C::S2C>>,
    mut disconnect: EventReader<DisconnectClient>,
) where
    C: ServerTransportConfig,
    T: ServerTransport<C> + Resource,
{
    for ToClient { client, msg } in to_client.iter() {
        server.send(*client, msg.clone());
    }

    for DisconnectClient { client } in disconnect.iter() {
        server.disconnect(*client);
    }
}
