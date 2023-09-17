use std::{collections::HashMap, marker::PhantomData};

use bevy::prelude::*;
use tokio::sync::mpsc::error::TryRecvError;

use crate::{AsyncRuntime, TransportConfig};

use super::{ClientId, DisconnectReason, ServerStream, WtServerFrontend, B2F, F2B};

#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct WtServerPlugin<C> {
    _phantom: PhantomData<C>,
}

impl<C: TransportConfig> Plugin for WtServerPlugin<C> {
    fn build(&self, app: &mut App) {
        app.init_resource::<AsyncRuntime>()
            .add_event::<ServerStarted>()
            .add_event::<ServerClientIncoming>()
            .add_event::<ServerClientConnected>()
            .add_event::<ServerRecv<C::C2S>>()
            .add_event::<ServerClientDisconnected>()
            .add_event::<ServerSend<C::S2C>>()
            .add_event::<ServerDisconnectClient>()
            .add_event::<ServerClosed>()
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
pub struct ServerStarted;

#[derive(Debug, Clone, Event)]
pub struct ServerClientIncoming {
    pub client: ClientId,
    pub authority: String,
    pub path: String,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Event)]
pub struct ServerClientConnected {
    pub client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct ServerRecv<C2S> {
    pub client: ClientId,
    pub msg: C2S,
}

#[derive(Debug, Event)]
pub struct ServerClientDisconnected {
    pub client: ClientId,
    pub reason: DisconnectReason,
}

#[derive(Debug, Clone, Event)]
pub struct ServerSend<S2C> {
    pub client: ClientId,
    pub stream: ServerStream,
    pub msg: S2C,
}

#[derive(Debug, Clone, Event)]
pub struct ServerDisconnectClient {
    pub client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct ServerClosed;

fn recv<C: TransportConfig>(
    mut commands: Commands,
    mut server: ResMut<WtServerFrontend<C>>,
    mut started: EventWriter<ServerStarted>,
    mut incoming: EventWriter<ServerClientIncoming>,
    mut connected: EventWriter<ServerClientConnected>,
    mut recv: EventWriter<ServerRecv<C::C2S>>,
    mut disconnected: EventWriter<ServerClientDisconnected>,
    mut closed: EventWriter<ServerClosed>,
) {
    loop {
        match server.recv.try_recv() {
            Ok(B2F::Started) => started.send(ServerStarted),
            Ok(B2F::Incoming {
                client,
                authority,
                path,
                headers,
            }) => incoming.send(ServerClientIncoming {
                client,
                authority,
                path,
                headers,
            }),
            Ok(B2F::Connected { client }) => connected.send(ServerClientConnected { client }),
            Ok(B2F::Recv { client, msg }) => recv.send(ServerRecv { client, msg }),
            Ok(B2F::Disconnected { client, reason }) => {
                disconnected.send(ServerClientDisconnected { client, reason })
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                commands.remove_resource::<WtServerFrontend<C>>();
                closed.send(ServerClosed);
                break;
            }
        }
    }
}

fn send<C: TransportConfig>(
    server: Res<WtServerFrontend<C>>,
    mut send: EventReader<ServerSend<C::S2C>>,
    mut disconnect: EventReader<ServerDisconnectClient>,
) {
    for ServerSend {
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
        debug!("Sending disconnect to {client}");
        let _ = server.send.send(F2B::Disconnect { client: *client });
    }
}
