//! Bevy plugin for interacting with a server transport.
//!
//! See [`TransportPlugin`] for usage info.

use std::marker::PhantomData;

use bevy::prelude::*;

use crate::TransportConfig;

use super::{ClientId, Event, RecvError, Transport, SessionError};

#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct TransportPlugin<C, T> {
    _phantom_c: PhantomData<C>,
    _phantom_t: PhantomData<T>,
}

impl<C: TransportConfig, T: Transport<C> + Resource> Plugin for TransportPlugin<C, T> {
    fn build(&self, app: &mut App) {
        app.add_event::<ClientRequested>()
            .add_event::<ClientConnected>()
            .add_event::<FromClient<C::C2S>>()
            .add_event::<ClientDisconnected>()
            .add_event::<ToClient<C::S2C>>()
            .add_event::<DisconnectClient>()
            .configure_set(PreUpdate, TransportSet::Recv.run_if(resource_exists::<T>()))
            .configure_set(
                PostUpdate,
                TransportSet::Send.run_if(resource_exists::<T>()),
            )
            .add_systems(PreUpdate, recv::<C, T>.in_set(TransportSet::Recv))
            .add_systems(PostUpdate, send::<C, T>.in_set(TransportSet::Send));
    }
}

/// A system set for transport operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum TransportSet {
    /// Transports receiving data and forwarding it to the app.
    Recv,
    /// Transports sending data from the app.
    Send,
}

/// A client requested to connect and subsequently was given an ID.
/// 
/// This is analogous to [`Event::Requested`].
#[derive(Debug, Clone, Event)]
pub struct ClientRequested {
    /// The ID of the requesting client.
    pub client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct ClientConnected {
    pub client: ClientId,
}

#[derive(Debug, Clone, Event)]
pub struct FromClient<C2S> {
    pub client: ClientId,
    pub msg: C2S,
}

#[derive(Debug, Event)]
pub struct ClientDisconnected {
    pub client: ClientId,
    pub reason: SessionError,
}

#[derive(Debug, Event)]
pub struct ToClient<S2C> {
    pub client: ClientId,
    pub msg: S2C,
}

#[derive(Debug, Clone, Event)]
pub struct DisconnectClient {
    pub client: ClientId,
}

fn recv<C: TransportConfig, T: Transport<C> + Resource>(
    mut commands: Commands,
    mut server: ResMut<T>,
    mut requested: EventWriter<ClientRequested>,
    mut connected: EventWriter<ClientConnected>,
    mut from_client: EventWriter<FromClient<C::C2S>>,
    mut disconnected: EventWriter<ClientDisconnected>,
) {
    loop {
        match server.recv() {
            Ok(Event::Requested { client }) => {
                requested.send(ClientRequested { client });
            }
            Ok(Event::Connected { client }) => {
                connected.send(ClientConnected { client });
            }
            Ok(Event::Recv { client, msg }) => {
                from_client.send(FromClient { client, msg });
            }
            Ok(Event::Disconnected { client, reason }) => {
                disconnected.send(ClientDisconnected { client, reason });
            }
            Err(RecvError::Empty) => break,
            Err(RecvError::Closed) => {
                commands.remove_resource::<T>();
                break;
            }
        }
    }
}

fn send<C: TransportConfig, T: Transport<C> + Resource>(
    mut server: ResMut<T>,
    mut to_client: EventReader<ToClient<C::S2C>>,
    mut disconnect: EventReader<DisconnectClient>,
) {
    for ToClient { client, msg } in to_client.iter() {
        server.send(*client, msg.clone());
    }

    for DisconnectClient { client } in disconnect.iter() {
        server.disconnect(*client);
    }
}
