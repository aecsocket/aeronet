use std::marker::PhantomData;

use bevy::prelude::*;

use crate::{util::AsPrettyError, ClientTransport, TransportSettings};

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
        app.add_event::<ClientRecvEvent<S>>()
            .add_event::<ClientSendEvent<S>>()
            .add_event::<ClientTransportError>()
            .add_systems(PreUpdate, recv::<S, T>.run_if(resource_exists::<T>()))
            .add_systems(PostUpdate, send::<S, T>.run_if(resource_exists::<T>()))
            .add_systems(PostUpdate, log);
    }
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
                errors.send(ClientTransportError::Recv(err.into()));
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
        match transport.send(msg.clone()) {
            Ok(_) => {}
            Err(err) => errors.send(ClientTransportError::Send(err.into())),
        }
    }
}

fn log(mut errors: EventReader<ClientTransportError>) {
    for err in errors.iter() {
        warn!("Client transport error: {:#}", err.as_pretty());
    }
}
