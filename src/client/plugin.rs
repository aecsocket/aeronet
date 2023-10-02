use std::marker::PhantomData;

use bevy::prelude::*;

use crate::{ClientEvent, ClientTransport, MessageTypes, RecvError, SessionError};

/// Configures a [`ClientTransport`] of type `T`.
///
/// This handles receiving data from the transport and forwarding it to the app via events,
/// as well as sending data to the transport by reading from events. The events provided are:
/// * Incoming
///   * [`LocalClientConnecting`] when the app asks the client to connect to a server
///     * Transport implementations are not required to send this event, so don't rely on it
///       on main logic
///   * [`LocalClientConnected`] when the client fully connects to the server
///     * Use this to run logic when connection is complete e.g. loading the level
///   * [`FromServer`] when the server sends data to this client
///   * [`LocalClientDisconnected`] when the client loses connection
///     * Use this to run logic to transition out of the game state
/// * Outgoing
///   * [`ToServer`] to send a message to a server
///
/// # Usage
///
/// You will need an implementation of [`ClientTransportConfig`] to use as the `C` type parameter.
/// See that type's docs to see how to implement one.
///
/// This plugin is not *required* to use the server transports. Using the plugin actually poses
/// the following limitations:
/// * You do not get ownership of incoming messages
///   * This means you are unable to mutate the messages before sending them to the rest of the app
///     via [`FromServer`]
/// * The transport implementation must implement [`Resource`]
///   * All inbuilt transports implement [`Resource`]
///
/// If these are unsuitable for your use case, consider manually using the transport APIs from your
/// app, bypassing the plugin altogether.
/// ```
/// use bevy::prelude::*;
/// use aeronet::ClientTransportPlugin;
///
/// # fn run<MyTransportConfig, MyTransportImpl>()
/// # where
/// #     MyTransportConfig: aeronet::ClientTransportConfig,
/// #     MyTransportImpl: aeronet::ClientTransport<MyTransportConfig> + Resource,
/// # {
/// App::new()
///     .add_plugins(TransportPlugin::<MyTransportConfig, MyTransportImpl>::default());
/// # }
/// ```
#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct ClientTransportPlugin<M, T> {
    _phantom_m: PhantomData<M>,
    _phantom_t: PhantomData<T>,
}

impl<C2S, S2C, M, T> Plugin for ClientTransportPlugin<M, T>
where
    C2S: Send + Sync + Clone + 'static,
    S2C: Send + Sync + 'static,
    M: MessageTypes<C2S = C2S, S2C = S2C>,
    T: ClientTransport<M> + Resource,
{
    fn build(&self, app: &mut App) {
        app.add_event::<LocalClientConnecting>()
            .add_event::<LocalClientConnected>()
            .add_event::<FromServer<S2C>>()
            .add_event::<LocalClientDisconnected>()
            .add_event::<ToServer<C2S>>()
            .configure_set(
                PreUpdate,
                ClientTransportSet::Recv.run_if(resource_exists::<T>()),
            )
            .configure_set(
                PostUpdate,
                ClientTransportSet::Send.run_if(resource_exists::<T>()),
            )
            .add_systems(
                PreUpdate,
                recv::<S2C, M, T>.in_set(ClientTransportSet::Recv),
            )
            .add_systems(
                PostUpdate,
                send::<C2S, M, T>.in_set(ClientTransportSet::Send),
            );
    }
}

/// A system set for client transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    /// Receives events from the connected server and forwards it to the app.
    Recv,
    /// Sends requests from the app to the connected server.
    Send,
}

/// See [`ClientEvent::Connecting`].
#[derive(Debug, Clone, Event)]
pub struct LocalClientConnecting;

/// See [`ClientEvent::Connected`].
#[derive(Debug, Clone, Event)]
pub struct LocalClientConnected;

/// See [`ClientEvent::Recv`].
#[derive(Debug, Clone, Event)]
pub struct FromServer<S2C> {
    /// See [`ClientEvent::Recv::msg`].
    pub msg: S2C,
}

/// See [`ClientEvent::Disconnected`].
#[derive(Debug, Event)]
pub struct LocalClientDisconnected {
    /// See [`ClientEvent::Disconnected::reason`].
    pub reason: SessionError,
}

/// Sends a message to a connected client using [`ClientTransport::send`].
#[derive(Debug, Event)]
pub struct ToServer<C2S> {
    /// The message to send.
    pub msg: C2S,
}

fn recv<S2C, M, T>(
    mut commands: Commands,
    mut client: ResMut<T>,
    mut connecting: EventWriter<LocalClientConnecting>,
    mut connected: EventWriter<LocalClientConnected>,
    mut from_server: EventWriter<FromServer<M::S2C>>,
    mut disconnected: EventWriter<LocalClientDisconnected>,
) where
    S2C: Send + Sync + 'static,
    M: MessageTypes<S2C = S2C>,
    T: ClientTransport<M> + Resource,
{
    loop {
        match client.recv() {
            Ok(ClientEvent::Connecting) => connecting.send(LocalClientConnecting),
            Ok(ClientEvent::Connected) => {
                connected.send(LocalClientConnected);
            }
            Ok(ClientEvent::Recv { msg }) => {
                from_server.send(FromServer { msg });
            }
            Ok(ClientEvent::Disconnected { reason }) => {
                disconnected.send(LocalClientDisconnected { reason });
            }
            Err(RecvError::Empty) => break,
            Err(RecvError::Closed) => {
                commands.remove_resource::<T>();
                break;
            }
        }
    }
}

fn send<C2S, M, T>(mut client: ResMut<T>, mut to_server: EventReader<ToServer<M::C2S>>)
where
    C2S: Send + Sync + Clone + 'static,
    M: MessageTypes<C2S = C2S>,
    T: ClientTransport<M> + Resource,
{
    for ToServer { msg } in to_server.iter() {
        client.send(msg.clone());
    }
}
