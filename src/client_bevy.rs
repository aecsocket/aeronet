use std::marker::PhantomData;

use bevy::prelude::*;

use crate::{util::AsPrettyError, ClientTransport, ClientTransportEvent, TransportSettings};

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
                (pop_events::<S, T>, recv::<S, T>)
                    .chain()
                    .in_set(ClientTransportSet::Recv),
            )
            .add_systems(
                PostUpdate,
                (send::<S, T>.in_set(ClientTransportSet::Send), log).chain(),
            );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, SystemSet)]
pub enum ClientTransportSet {
    Recv,
    Send,
}

#[derive(Debug, Clone, Event)]
pub struct ClientRecvEvent<S: TransportSettings> {
    pub msg: S::S2C,
}

#[derive(Debug, Clone, Event)]
pub struct ClientSendEvent<S: TransportSettings> {
    pub msg: S::C2S,
}

#[derive(Debug, thiserror::Error, Event)]
pub enum ClientTransportError {
    #[error("receiving data from server")]
    Recv(#[source] anyhow::Error),
    #[error("sending data to server")]
    Send(#[source] anyhow::Error),
}

fn pop_events<S: TransportSettings, T: ClientTransport<S> + Resource>(
    mut transport: ResMut<T>,
    mut events: EventWriter<ClientTransportEvent>,
) {
    while let Some(event) = transport.pop_event() {
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
