use std::marker::PhantomData;

use bevy::prelude::*;

use crate::{util::AsPrettyError, ClientTransport, ClientTransportError};

#[derive(derivative::Derivative)]
#[derivative(Default)]
pub struct ClientTransportPlugin<T> {
    _phantom: PhantomData<T>,
}

impl<T: ClientTransport + Resource> Plugin for ClientTransportPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_event::<ClientTransportError>()
            .add_systems(PreUpdate, recv_client::<T>.run_if(resource_exists::<T>()));
    }
}

fn recv_client<T: ClientTransport + Resource>(
    mut transport: ResMut<T>,
    mut errors: EventWriter<ClientTransportError>,
) {
    while let Some(result) = transport.recv() {
        match result {
            Ok(msg) => {} // todo
            Err(err) => errors.send(err),
        }
    }
}

fn send_client<T: ClientTransport + Resource>(
    mut transport: ResMut<T>,
    mut errors: EventWriter<ClientTransportError>,
) {
    
}

fn log(mut errors: EventReader<ClientTransportError>) {
    for err in errors.iter() {
        warn!("Client transport error: {:#}", err.as_pretty());
    }
}
