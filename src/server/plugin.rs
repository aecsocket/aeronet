use std::marker::PhantomData;

use bevy::{prelude::*, utils::HashSet};

use crate::{
    error::AsPrettyError, ClientId, ServerTransport, ServerTransportEvent, TransportSettings,
};

/// Provides default functionality for consuming data from and sending data to a
/// [`ServerTransport`].
///
/// This plugin provides:
/// - [`ServerTransportEvent`] for consuming connect/disconnect client events
/// - [`ServerRecvEvent`] for consuming messages sent from a client
/// - [`ServerSendEvent`] for sending messages to a client
/// - [`ServerDisconnectClientEvent`] for disconnecting a client from the server
/// - [`ServerTransportError`] event for consuming errors that occurred while doing the above
///
/// This plugin is *not* required; you can implement receiving and sending messages entirely on
/// your own if you wish. This may be useful when you want ownership of the received message before
/// they are sent to the rest of your app, or when they are sent out.
#[derive(derivative::Derivative)]
#[derivative(Default)]
pub struct ServerTransportPlugin<S, T> {
    _phantom_s: PhantomData<S>,
    _phantom_t: PhantomData<T>,
}

impl<S: TransportSettings, T: ServerTransport<S> + Resource> Plugin
    for ServerTransportPlugin<S, T>
{
    fn build(&self, app: &mut App) {
        app.add_event::<ServerTransportEvent>()
            .add_event::<ServerRecvEvent<S>>()
            .add_event::<ServerSendEvent<S>>()
            .add_event::<ServerDisconnectClientEvent>()
            .add_event::<ServerTransportError>()
            .configure_set(
                PreUpdate,
                ServerTransportSet::Recv
                    .run_if(resource_exists::<T>().and_then(resource_exists::<ClientSet>())),
            )
            .configure_set(
                PostUpdate,
                ServerTransportSet::Send
                    .run_if(resource_exists::<T>().and_then(resource_exists::<ClientSet>())),
            )
            .add_systems(
                PreUpdate,
                (disconnect::<S, T>, pop_events::<S, T>, recv::<S, T>)
                    .chain()
                    .in_set(ServerTransportSet::Recv),
            )
            .add_systems(
                PostUpdate,
                send::<S, T>.chain().in_set(ServerTransportSet::Send),
            )
            .add_systems(PostUpdate, log);
    }
}

/// System set used by [`ServerTransportPlugin`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerTransportSet {
    /// When receiving events and messages from a transport.
    Recv,
    /// When sending messages out using a transport.
    Send,
}

/// Keeps track of clients which are connected to the current server transport.
///
/// The [`ServerTransportPlugin`] automatically adds and removes clients to/from this set
/// according to [`ServerTransportEvent`] events.
#[derive(Debug, Default, Resource)]
pub struct ClientSet(pub HashSet<ClientId>);

/// Sent when the transport receives a message from a client.
#[derive(Debug, Clone, Event)]
pub struct ServerRecvEvent<S: TransportSettings> {
    /// Which client sent the message.
    pub from: ClientId,
    /// The message received.
    ///
    /// Note that consumers in a system will only have access to this behind a shared reference;
    /// if you want ownership of this data, consider not using the plugin as described in the
    /// [plugin docs].
    ///
    /// [plugin docs]: struct.ServerTransportPlugin.html
    pub msg: S::C2S,
}

/// Sent when a system wants to send a message to a client.
#[derive(Debug, Clone, Event)]
pub struct ServerSendEvent<S: TransportSettings> {
    /// Which client to send the message to.
    pub to: ClientId,
    /// The message that the app wants to send using the transport.
    pub msg: S::S2C,
}

/// Sent when a system wants to disconnect a client from the server.
#[derive(Debug, Clone, Event)]
pub struct ServerDisconnectClientEvent {
    /// Which client to disconnect.
    pub client: ClientId,
}

/// Sent when the transport experiences an error.
#[derive(Debug, thiserror::Error, Event)]
pub enum ServerTransportError {
    /// Some message could not be received from a client.
    #[error("receiving data from client `{from}`")]
    Recv {
        /// The client from which data could not be received.
        from: ClientId,
        /// The source of the error.
        #[source]
        source: anyhow::Error,
    },
    /// Some message could not be sent to a client.
    #[error("sending data to client `{to}`")]
    Send {
        /// The client to which data could not be sent.
        to: ClientId,
        /// The source of the error.
        #[source]
        source: anyhow::Error,
    },
    /// A client could not be disconnected.
    #[error("disconnecting client `{client}`")]
    Disconnect {
        /// The client which could not be disconnected.
        client: ClientId,
        /// The source of the error.
        #[source]
        source: anyhow::Error,
    },
}

fn disconnect<S: TransportSettings, T: ServerTransport<S> + Resource>(
    mut transport: ResMut<T>,
    mut disconnect: EventReader<ServerDisconnectClientEvent>,
    mut errors: EventWriter<ServerTransportError>,
) {
    for ServerDisconnectClientEvent { client } in disconnect.iter() {
        if let Err(err) = transport.disconnect(*client) {
            errors.send(ServerTransportError::Disconnect {
                client: *client,
                source: err,
            });
        }
    }
}

fn pop_events<S: TransportSettings, T: ServerTransport<S> + Resource>(
    mut transport: ResMut<T>,
    mut clients: ResMut<ClientSet>,
    mut events: EventWriter<ServerTransportEvent>,
) {
    while let Some(event) = transport.pop_event() {
        match event {
            ServerTransportEvent::Connect { client } => clients.0.insert(client),
            ServerTransportEvent::Disconnect { client, .. } => clients.0.remove(&client),
        };
        events.send(event);
    }
}

fn recv<S: TransportSettings, T: ServerTransport<S> + Resource>(
    mut transport: ResMut<T>,
    clients: Res<ClientSet>,
    mut recv: EventWriter<ServerRecvEvent<S>>,
    mut errors: EventWriter<ServerTransportError>,
) {
    for from in clients.0.iter() {
        loop {
            match transport.recv(*from) {
                Ok(Some(msg)) => recv.send(ServerRecvEvent { from: *from, msg }),
                Ok(None) => break,
                Err(err) => {
                    errors.send(ServerTransportError::Recv {
                        from: *from,
                        source: err,
                    });
                    break;
                }
            }
        }
    }
}

fn send<S: TransportSettings, T: ServerTransport<S> + Resource>(
    mut transport: ResMut<T>,
    mut send: EventReader<ServerSendEvent<S>>,
    mut errors: EventWriter<ServerTransportError>,
) {
    for ServerSendEvent { to, msg } in send.iter() {
        if let Err(err) = transport.send(*to, msg.clone()) {
            errors.send(ServerTransportError::Send {
                to: *to,
                source: err,
            });
        }
    }
}

fn log(mut errors: EventReader<ServerTransportError>) {
    for err in errors.iter() {
        warn!("Server transport error: {:#}", err.as_pretty());
    }
}
