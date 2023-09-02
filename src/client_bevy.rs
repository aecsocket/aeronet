use std::marker::PhantomData;

use bevy::prelude::*;

use crate::{util::AsPrettyError, ClientTransport, ClientTransportError, TransportSettings};

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
        app//.add_event::<ClientRecvEvent<S>>()
            //.add_event::<ClientSendEvent<S>>()
            //.add_event::<ClientTransportError>()
            .add_systems(PreUpdate, recv::<S, T>.run_if(resource_exists::<T>()));
            //.add_systems(PostUpdate, send::<S, T>.run_if(resource_exists::<T>()))
            //.add_systems(PostUpdate, log);
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

fn recv<S: TransportSettings, T: ClientTransport<S> + Resource>(
    mut transport: ResMut<T>,
    //mut recv: EventWriter<ClientRecvEvent<S>>,
    //mut errors: EventWriter<ClientTransportError>,
) {
    while let Some(result) = transport.recv() {
        // match result {
        //     Ok(msg) => {},//recv.send(ClientRecvEvent { msg }),
        //     Err(err) => {},//errors.send(err),
        // }
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
            Err(err) => errors.send(err),
        }
    }
}

fn log(mut errors: EventReader<ClientTransportError>) {
    for err in errors.iter() {
        warn!("Client transport error: {:#}", err.as_pretty());
    }
}
