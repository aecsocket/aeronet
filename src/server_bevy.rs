use std::marker::PhantomData;

use bevy::{prelude::*, utils::HashSet};

use crate::{
    util::AsPrettyError, ClientId, ServerTransport, ServerTransportEvent,
    TransportSettings,
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
        app.add_event::<ServerTransportEvent>()
            .add_event::<ServerRecvEvent<S>>()
            .add_event::<ServerSendEvent<S>>()
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
                (recv_events::<S, T>, recv::<S, T>)
                    .chain()
                    .in_set(ServerTransportSet::Recv),
            )
            .add_systems(PostUpdate, send::<S, T>.in_set(ServerTransportSet::Send))
            .add_systems(PostUpdate, log);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, SystemSet)]
pub enum ServerTransportSet {
    Recv,
    Send,
}

#[derive(Debug, Default, Resource)]
pub struct ClientSet(pub HashSet<ClientId>);

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

#[derive(Debug, thiserror::Error, Event)]
pub enum ServerTransportError {
    #[error("receiving client events")]
    RecvEvents(#[source] anyhow::Error),
    #[error("receiving data from client `{from}`")]
    Recv {
        from: ClientId,
        #[source]
        source: anyhow::Error,
    },
    #[error("sending data to client `{to}`")]
    Send {
        to: ClientId,
        #[source]
        source: anyhow::Error,
    }
}

fn recv_events<S: TransportSettings, T: ServerTransport<S> + Resource>(
    mut transport: ResMut<T>,
    mut clients: ResMut<ClientSet>,
    mut events: EventWriter<ServerTransportEvent>,
    mut errors: EventWriter<ServerTransportError>,
) {
    loop {
        match transport.recv_events() {
            Ok(Some(event)) => {
                match event {
                    ServerTransportEvent::Connect { id } => clients.0.insert(id),
                    ServerTransportEvent::Disconnect { id } => clients.0.remove(&id),
                };
                events.send(event);
            }
            Ok(None) => break,
            Err(err) => {
                errors.send(ServerTransportError::RecvEvents(err));
                break;
            }
        }
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
                    errors.send(ServerTransportError::Recv { from: *from, source: err });
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
        match transport.send(*to, msg.clone()) {
            Ok(_) => {}
            Err(err) => errors.send(ServerTransportError::Send { to: *to, source: err }),
        }
    }
}

fn log(mut errors: EventReader<ServerTransportError>) {
    for err in errors.iter() {
        warn!("Server transport error: {:#}", err.as_pretty());
    }
}
