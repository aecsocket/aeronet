use std::marker::PhantomData;

use bevy::prelude::*;

use crate::{util::AsPrettyError, ClientTransport, ClientTransportEvent, TransportSettings};

/// Provides default functionality for consuming data from and sending data to a
/// [`ClientTransport`].
///
/// This plugin provides:
/// - [`ClientTransportEvent`] for consuming connect/disconnect events
/// - [`ClientRecvEvent`] for consuming messages sent from the server
/// - [`ClientSendEvent`] for sending messages to the server
/// - [`ClientTransportError`] event for consuming errors that occurred while doing the above
/// 
/// The systems this plugin adds follow the order described in the [`ClientTransport`] docs.
/// In addition, when the transport is disconnected from the server, the `T` resource is
/// automatically removed.
///
/// This plugin is *not* required; you can implement receiving and sending messages entirely on
/// your own if you wish. This may be useful when you want ownership of the received message before
/// they are sent to the rest of your app, or when they are sent out.
#[derive(derivative::Derivative)]
#[derivative(Default)]
pub struct ClientTransportPlugin<S, T> {
    _phantom_s: PhantomData<S>,
    _phantom_t: PhantomData<T>,
}

impl<S: TransportSettings, T: ClientTransport<S> + Resource> Plugin
    for ClientTransportPlugin<S, T>
{
    fn build(&self, app: &mut App) {
        app.add_event::<ClientTransportEvent>()
            .add_event::<ClientRecvEvent<S>>()
            .add_event::<ClientSendEvent<S>>()
            .add_event::<ClientTransportError>()
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
                (
                    // If `pop_events` ends up removing the client, it means that the connection
                    // is closed - trying to receive any more data will be an error.
                    // Therefore, we immediately remove the resource, then re-check if the resource
                    // exists for the `recv` invocation.
                    (pop_events::<S, T>, apply_deferred)
                        .chain()
                        .in_set(ClientTransportSet::Recv),
                    recv::<S, T>.chain().in_set(ClientTransportSet::Recv),
                ),
            )
            .add_systems(
                PostUpdate,
                (send::<S, T>.in_set(ClientTransportSet::Send), log).chain(),
            );
    }
}

/// System set used by [`ClientTransportPlugin`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    /// When receiving events and messages from a transport.
    Recv,
    /// When sending messages out using a transport.
    Send,
}

/// Snet when the transport receives a message from the server.
#[derive(Debug, Clone, Event)]
pub struct ClientRecvEvent<S: TransportSettings> {
    /// The message received.
    ///
    /// Note that consumers in a system will only have access to this behind a shared reference;
    /// if you want ownership of this data, consider not using the plugin as described in the
    /// [plugin docs].
    ///
    /// [plugin docs]: struct.ClientTransportPlugin.html
    pub msg: S::S2C,
}

/// Snet when a system wants to send a message to the server.
#[derive(Debug, Clone, Event)]
pub struct ClientSendEvent<S: TransportSettings> {
    /// The message that the app wants to send using the transport.
    pub msg: S::C2S,
}

/// Sent when the transport experiences an error.
#[derive(Debug, thiserror::Error, Event)]
pub enum ClientTransportError {
    /// Some message could not be received from the server.
    #[error("receiving data from server")]
    Recv(#[source] anyhow::Error),
    /// Some message could not be sent to the server.
    #[error("sending data to server")]
    Send(#[source] anyhow::Error),
}

fn pop_events<S: TransportSettings, T: ClientTransport<S> + Resource>(
    mut commands: Commands,
    mut transport: ResMut<T>,
    mut events: EventWriter<ClientTransportEvent>,
) {
    while let Some(event) = transport.pop_event() {
        match event {
            ClientTransportEvent::Disconnect { .. } => commands.remove_resource::<T>(),
            _ => {}
        };
        events.send(event);
    }
}

fn recv<S: TransportSettings, T: ClientTransport<S> + Resource>(
    mut transport: ResMut<T>,
    mut recv: EventWriter<ClientRecvEvent<S>>,
    mut errors: EventWriter<ClientTransportError>,
) {
    loop {
        match transport.recv() {
            Ok(Some(msg)) => recv.send(ClientRecvEvent { msg }),
            Ok(None) => break,
            Err(err) => {
                errors.send(ClientTransportError::Recv(err));
                break;
            }
        }
    }
}

fn send<S: TransportSettings, T: ClientTransport<S> + Resource>(
    mut transport: ResMut<T>,
    mut send: EventReader<ClientSendEvent<S>>,
    mut errors: EventWriter<ClientTransportError>,
) {
    for ClientSendEvent { msg } in send.iter() {
        if let Err(err) = transport.send(msg.clone()) {
            errors.send(ClientTransportError::Send(err.into()));
        }
    }
}

fn log(mut errors: EventReader<ClientTransportError>) {
    for err in errors.iter() {
        warn!("Client transport error: {:#}", err.as_pretty());
    }
}
