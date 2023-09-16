use std::{collections::HashMap, marker::PhantomData};

use bevy::prelude::*;
use tokio::sync::mpsc::error::TryRecvError;

use crate::{
    server::WtServerFrontend, AsyncRuntime, ClientId, ServerError, Stream, TransportConfig,
};

use super::{B2F, F2B};

#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct WtServerPlugin<C> {
    _phantom: PhantomData<C>,
}

impl<C: TransportConfig> Plugin for WtServerPlugin<C> {
    fn build(&self, app: &mut App) {
        app.init_resource::<AsyncRuntime>()
            .add_event::<ServerStartEvent>()
            .add_event::<ServerClientIncomingEvent>()
            .add_event::<ServerClientConnectEvent>()
            .add_event::<ServerRecvEvent<C::C2S>>()
            .add_event::<ServerClientDisconnectEvent>()
            .add_event::<ServerSendEvent<C::S2C>>()
            .add_event::<ServerDisconnectClientEvent>()
            .add_event::<ServerCloseEvent>()
            .add_event::<ServerError>()
            .add_systems(
                PreUpdate,
                recv::<C>.run_if(resource_exists::<WtServerFrontend<C>>()),
            )
            .add_systems(
                PostUpdate,
                (send::<C>.run_if(resource_exists::<WtServerFrontend<C>>()),).chain(),
            );
    }
}

#[derive(Debug, Clone, Event)]
pub struct ServerStartEvent;

#[derive(Debug, Clone, Event)]
pub struct ServerClientIncomingEvent {
    pub client: ClientId,
    pub authority: String,
    pub path: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Event)]
pub struct ServerClientConnectEvent {
    pub client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct ServerRecvEvent<C2S> {
    pub client: ClientId,
    pub msg: C2S,
}

#[derive(Debug, Clone, Event)]
pub struct ServerClientDisconnectEvent {
    pub client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct ServerSendEvent<S2C> {
    pub client: ClientId,
    pub stream: Stream,
    pub msg: S2C,
}

#[derive(Debug, Clone, Event)]
pub struct ServerDisconnectClientEvent {
    pub client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct ServerCloseEvent;

fn recv<C: TransportConfig>(
    mut commands: Commands,
    mut server: ResMut<WtServerFrontend<C>>,
    mut start: EventWriter<ServerStartEvent>,
    mut incoming: EventWriter<ServerClientIncomingEvent>,
    mut connect: EventWriter<ServerClientConnectEvent>,
    mut recv: EventWriter<ServerRecvEvent<C::C2S>>,
    mut disconnect: EventWriter<ServerClientDisconnectEvent>,
    mut close: EventWriter<ServerCloseEvent>,
    mut error: EventWriter<ServerError>,
) {
    loop {
        match server.recv.try_recv() {
            Ok(B2F::Start) => start.send(ServerStartEvent),
            Ok(B2F::Incoming {
                client,
                authority,
                path,
                headers,
            }) => incoming.send(ServerClientIncomingEvent {
                client,
                authority,
                path,
                headers,
            }),
            Ok(B2F::Connect { client }) => connect.send(ServerClientConnectEvent { client }),
            Ok(B2F::Recv { client, msg }) => recv.send(ServerRecvEvent { client, msg }),
            Ok(B2F::Disconnect { client }) => {
                disconnect.send(ServerClientDisconnectEvent { client })
            }
            Ok(B2F::Error(err)) => {
                warn!(
                    "Server transport error: {:#}",
                    aeronet::error::AsPrettyError::as_pretty(&err)
                );
                error.send(err);
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                commands.remove_resource::<WtServerFrontend<C>>();
                close.send(ServerCloseEvent);
                break;
            }
        }
    }
}

fn send<C: TransportConfig>(
    server: Res<WtServerFrontend<C>>,
    mut send: EventReader<ServerSendEvent<C::S2C>>,
    mut disconnect: EventReader<ServerDisconnectClientEvent>,
) {
    for ServerSendEvent {
        client,
        stream,
        msg,
    } in send.iter()
    {
        let _ = server.send.send(F2B::Send {
            client: *client,
            stream: *stream,
            msg: msg.clone(),
        });
    }

    for ServerDisconnectClientEvent { client } in disconnect.iter() {
        let _ = server.send.send(F2B::Disconnect { client: *client });
    }
}
