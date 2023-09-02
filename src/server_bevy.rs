use std::marker::PhantomData;

use bevy::prelude::*;

use crate::{
    util::AsPrettyError, ClientId, ServerTransport, ServerTransportError, TransportSettings,
};

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
        app.add_event::<ServerRecvEvent<S>>()
            .add_event::<ServerSendEvent<S>>()
            .add_event::<ServerTransportError>()
            .add_systems(PreUpdate, recv::<S, T>.run_if(resource_exists::<T>()))
            .add_systems(PostUpdate, send::<S, T>.run_if(resource_exists::<T>()))
            .add_systems(PostUpdate, log);
    }
}

#[derive(Debug, Clone, Event)]
pub struct ServerRecvEvent<S: TransportSettings> {
    pub from: ClientId,
    pub msg: S::C2S,
}

#[derive(Debug, Clone, Event)]
pub struct ServerSendEvent<S: TransportSettings> {
    pub to: ClientId,
    pub msg: S::S2C,
}

fn recv<S: TransportSettings, T: ServerTransport<S> + Resource>(
    mut transport: ResMut<T>,
    mut recv: EventWriter<ServerRecvEvent<S>>,
    mut errors: EventWriter<ServerTransportError>,
) {
    for from in transport.clients() {
        while let Some(result) = transport.recv(from) {
            match result {
                Ok(msg) => recv.send(ServerRecvEvent { from, msg }),
                Err(err) => errors.send(err),
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
        match transport.send(*to, msg.clone()) {
            Ok(_) => {}
            Err(err) => errors.send(err),
        }
    }
}

fn log(mut errors: EventReader<ServerTransportError>) {
    for err in errors.iter() {
        warn!("Server transport error: {:#}", err.as_pretty());
    }
}
