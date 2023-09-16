use std::{collections::HashMap, marker::PhantomData};

use bevy::prelude::*;
use tokio::sync::mpsc::error::TryRecvError;

use crate::{server::WtServerFrontend, AsyncRuntime, ClientId, Stream, TransportConfig, ServerError};

use super::{B2F, F2B};

#[derive(Debug)]
pub struct WtServerPlugin<C> {
    pub logging: bool,
    _phantom: PhantomData<C>,
}

impl<C> Default for WtServerPlugin<C> {
    fn default() -> Self {
        Self {
            logging: true,
            _phantom: PhantomData::default(),
        }
    }
}

impl<C: TransportConfig> Plugin for WtServerPlugin<C> {
    fn build(&self, app: &mut App) {
        app.init_resource::<AsyncRuntime>()
            .add_event::<ServerSendEvent<C::S2C>>()
            .add_event::<ServerDisconnectClient>()
            .add_event::<ServerRecvEvent<C::C2S>>()
            .add_systems(
                PreUpdate,
                recv::<C>.run_if(resource_exists::<WtServerFrontend<C>>()),
            )
            .add_systems(
                PostUpdate,
                send::<C>.run_if(resource_exists::<WtServerFrontend<C>>()),
            );

        if self.logging {
            app.add_systems(PostUpdate, log.after(send::<C>));
        }
    }
}

#[derive(Debug, Clone, Event)]
pub struct ServerStartEvent;

#[derive(Debug, Clone, Event)]
pub struct ServerClientIncomingEvent {
    client: ClientId,
    authority: String,
    path: String,
    headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Event)]
pub struct ServerClientConnectEvent {
    client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct ServerClientDisconnectEvent {
    client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct ServerRecvEvent<C2S> {
    pub client: ClientId,
    pub msg: C2S,
}

#[derive(Debug, Clone, Event)]
pub struct ServerSendEvent<S2C> {
    pub client: ClientId,
    pub stream: Stream,
    pub msg: S2C,
}

#[derive(Debug, Clone, Event)]
pub struct ServerDisconnectClient {
    pub client: ClientId,
}

fn recv<C: TransportConfig>(
    mut commands: Commands,
    mut server: ResMut<WtServerFrontend<C>>,
    mut start: EventWriter<ServerStartEvent>,
    mut incoming: EventWriter<ServerClientIncomingEvent>,
    mut connect: EventWriter<ServerClientConnectEvent>,
    mut recv: EventWriter<ServerRecvEvent<C::C2S>>,
    mut disconnect: EventWriter<ServerClientDisconnectEvent>,
    mut error: EventWriter<ServerError>,
) {
    loop {
        match server.recv.try_recv() {
/*
    Start,
    Incoming {
        client: ClientId,
        authority: String,
        path: String,
        headers: HashMap<String, String>,
    },
    Connect {
        client: ClientId,
    },
    Recv {
        client: ClientId,
        msg: C2S,
    },
    Disconnect {
        client: ClientId,
    },
    Error(ServerError), */

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
            Ok(B2F::Recv { client, msg }) => {
                recv.send(ServerRecvEvent { client, msg });
            }
            Ok(B2F::Disconnect { client }) => {
                disconnect.send(ServerClientDisconnectEvent { client })
            }
            Ok(B2F::Error(err)) => error.send(err),
            //
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                commands.remove_resource::<WtServerFrontend<C>>();
                info!("Server closed");
                break;
            }
        }
    }
}

fn send<C: TransportConfig>(
    server: Res<WtServerFrontend<C>>,
    mut send: EventReader<ServerSendEvent<C::S2C>>,
    mut disconnect: EventReader<ServerDisconnectClient>,
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

    for ServerDisconnectClient { client } in disconnect.iter() {
        let _ = server.send.send(F2B::Disconnect { client: *client });
    }
}

fn log(
    mut errors: EventReader<ServerError>
) {
    for err in errors.iter() {
        
    }
}
