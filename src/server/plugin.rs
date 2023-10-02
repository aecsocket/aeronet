use std::marker::PhantomData;

use bevy::prelude::*;

use super::{ClientId, MessageTypes, RecvError, ServerEvent, ServerTransport, SessionError};

/// Configures a [`ServerTransport`] of type `T`.
///
/// This handles receiving data from the transport and forwarding it to the app via events,
/// as well as sending data to the transport by reading from events. The events provided are:
/// * Incoming
///   * [`RemoteClientConnecting`] when a client requests a connection
///     * Transport implementations are not required to send this event, so don't rely on it
///       on main logic
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
/// use aeronet::ServerTransportPlugin;
///
/// # fn run<MyTransportConfig, MyTransportImpl>()
/// # where
/// #     MyTransportConfig: aeronet::ServerTransportConfig,
/// #     MyTransportImpl: aeronet::ServerTransport<MyTransportConfig> + Resource,
/// # {
/// App::new()
///     .add_plugins(TransportPlugin::<MyTransportConfig, MyTransportImpl>::default());
/// # }
/// ```
#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct ServerTransportPlugin<M, T> {
    _phantom_m: PhantomData<M>,
    _phantom_t: PhantomData<T>,
}

impl<C2S, S2C, M, T> Plugin for ServerTransportPlugin<M, T>
where
    C2S: Send + Sync + 'static,
    S2C: Send + Sync + Clone + 'static,
    M: MessageTypes<C2S = C2S, S2C = S2C>,
    T: ServerTransport<M> + Resource,
{
    fn build(&self, app: &mut App) {
        app.add_event::<RemoteClientConnecting>()
            .add_event::<RemoteClientConnected>()
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
                recv::<C2S, M, T>.in_set(ServerTransportSet::Recv),
            )
            .add_systems(
                PostUpdate,
                send::<S2C, M, T>.in_set(ServerTransportSet::Send),
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

/// See [`ServerEvent::Connecting`].
#[derive(Debug, Clone, Event)]
pub struct RemoteClientConnecting {
    /// See [`ServerEvent::Connecting::client`].
    pub client: ClientId,
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

fn recv<C2S, M, T>(
    mut commands: Commands,
    mut server: ResMut<T>,
    mut connecting: EventWriter<RemoteClientConnecting>,
    mut connected: EventWriter<RemoteClientConnected>,
    mut from_client: EventWriter<FromClient<C2S>>,
    mut disconnected: EventWriter<RemoteClientDisconnected>,
) where
    C2S: Send + Sync + 'static,
    M: MessageTypes<C2S = C2S>,
    T: ServerTransport<M> + Resource,
{
    loop {
        match server.recv() {
            Ok(ServerEvent::Connecting { client }) => {
                connecting.send(RemoteClientConnecting { client });
            }
            Ok(ServerEvent::Connected { client }) => {
                connected.send(RemoteClientConnected { client });
            }
            Ok(ServerEvent::Recv { client, msg }) => {
                from_client.send(FromClient { client, msg });
            }
            Ok(ServerEvent::Disconnected { client, reason }) => {
                disconnected.send(RemoteClientDisconnected { client, reason });
            }
            Err(RecvError::Empty) => break,
            Err(RecvError::Closed) => {
                commands.remove_resource::<T>();
                break;
            }
        }
    }
}

fn send<S2C, M, T>(
    mut server: ResMut<T>,
    mut to_client: EventReader<ToClient<S2C>>,
    mut disconnect: EventReader<DisconnectClient>,
) where
    S2C: Send + Sync + Clone + 'static,
    M: MessageTypes<S2C = S2C>,
    T: ServerTransport<M> + Resource,
{
    for ToClient { client, msg } in to_client.iter() {
        server.send(*client, msg.clone());
    }

    for DisconnectClient { client } in disconnect.iter() {
        server.disconnect(*client);
    }
}
