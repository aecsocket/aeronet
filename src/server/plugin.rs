//! Bevy plugin for interacting with a server transport.
//!
//! See [`TransportPlugin`] for usage info.

use std::marker::PhantomData;

use bevy::prelude::*;

use super::{ClientId, Event, RecvError, Transport, SessionError, TransportConfig};

/// Configures a server-side [`Transport`] of type `T` using configuration `C`.
/// 
/// This handles receiving data from the transport and forwarding it to the app via events,
/// as well as sending data to the transport by reading from events. The events provided are:
/// - Incoming
///   - [`ClientIncoming`] when a client requests a connection
///   - [`ClientConnected`] when a client fully connects
///     - Use this to run logic for when a client fully connects e.g. loading player data
///   - [`FromClient`] when a client sends data to the server
///   - [`ClientDisconnected`] when a client loses connection
///     - Use this to run logic for when a client is dropped
/// - Outgoing
///   - [`ToClient`] to send a message to a client
///   - [`DisconnectClient`] to force a client to lose the connection
/// 
/// # Usage
/// 
/// You will need an implementation of [`TransportConfig`] to use as the `C` type parameter.
/// See that type's docs to see how to implement one.
/// 
/// **Note:** this plugin is not *required* to use the server transports, however provides a simple
/// transport-agnostic event API to send and receive data. This means that you can use any type of
/// transport implementation on top of this app.
/// 
/// However if you have different requirements, such as reading incoming data and mutating it
/// before sending it to the rest of the app (which requires ownership of messages), consider
/// using your [`Transport`] directly as a resource.
#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct TransportPlugin<C, T> {
    _phantom_c: PhantomData<C>,
    _phantom_t: PhantomData<T>,
}

impl<C, T> Plugin for TransportPlugin<C, T>
where
    C: TransportConfig,
    T: Transport<C> + Resource,
{
    fn build(&self, app: &mut App) {
        app.add_event::<ClientIncoming>()
            .add_event::<ClientConnected>()
            .add_event::<FromClient<C::C2S>>()
            .add_event::<ClientDisconnected>()
            .add_event::<ToClient<C::S2C>>()
            .add_event::<DisconnectClient>()
            .configure_set(PreUpdate, TransportSet::Recv.run_if(resource_exists::<T>()))
            .configure_set(
                PostUpdate,
                TransportSet::Send.run_if(resource_exists::<T>()),
            )
            .add_systems(PreUpdate, recv::<C, T>.in_set(TransportSet::Recv))
            .add_systems(PostUpdate, send::<C, T>.in_set(TransportSet::Send));
    }
}

/// A system set for transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    /// Transports receiving data and forwarding it to the app.
    Recv,
    /// Transports sending data from the app.
    Send,
}

/// See [`Event::Incoming`].
#[derive(Debug, Clone, Event)]
pub struct ClientIncoming {
    /// See [`Event::Incoming::client`].
    pub client: ClientId,
}

/// See [`Event::Connected`].
#[derive(Debug, Clone, Event)]
pub struct ClientConnected {
    /// See [`Event::Connected::client`].
    pub client: ClientId,
}

/// See [`Event::Recv`].
#[derive(Debug, Clone, Event)]
pub struct FromClient<C2S> {
    /// See [`Event::Recv::client`].
    pub client: ClientId,
    /// See [`Event::Recv::msg`].
    pub msg: C2S,
}

/// See [`Event::Disconnected`].
#[derive(Debug, Event)]
pub struct ClientDisconnected {
    /// See [`Event::Disconnected::client`].
    pub client: ClientId,
    /// See [`Event::Disconnected::reason`].
    pub reason: SessionError,
}

/// Sends a message to a connected client using [`Transport::send`].
#[derive(Debug, Event)]
pub struct ToClient<S2C> {
    /// The ID of the client to send to.
    pub client: ClientId,
    /// The message to send.
    pub msg: S2C,
}

/// Forcefully disconnects a client using [`Transport::disconnect`].
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
)
where
    C: TransportConfig,
    T: Transport<C> + Resource,
{
    loop {
        match server.recv() {
            Ok(Event::Incoming { client }) => {
                requested.send(ClientIncoming { client });
            }
            Ok(Event::Connected { client }) => {
                connected.send(ClientConnected { client });
            }
            Ok(Event::Recv { client, msg }) => {
                from_client.send(FromClient { client, msg });
            }
            Ok(Event::Disconnected { client, reason }) => {
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
)
where
    C: TransportConfig,
    T: Transport<C> + Resource,
{
    for ToClient { client, msg } in to_client.iter() {
        server.send(*client, msg.clone());
    }

    for DisconnectClient { client } in disconnect.iter() {
        server.disconnect(*client);
    }
}
