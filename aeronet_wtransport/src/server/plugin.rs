use std::marker::PhantomData;

use bevy::prelude::*;
use tokio::{runtime::Runtime, sync::mpsc::error::TryRecvError};
use wtransport::ServerConfig;

use crate::{server::SyncServer, AsyncRuntime, TransportConfig};

use super::A2S;

#[derive(Debug, derivative::Derivative)]
#[derivative(Default)]
pub struct WebTransportServerPlugin<C: TransportConfig> {
    _phantom: PhantomData<C>,
}

impl<T: TransportConfig> Plugin for WebTransportServerPlugin<T> {
    fn build(&self, app: &mut App) {
        app.init_resource::<AsyncRuntime>().add_systems(
            PreUpdate,
            recv::<T>.run_if(resource_exists::<WebTransportServer<T>>()),
        );
    }
}

#[derive(Resource)]
pub struct WebTransportServer<C: TransportConfig>(SyncServer<C>);

impl<C: TransportConfig> WebTransportServer<C> {
    pub fn new(config: ServerConfig, rt: &Runtime) -> Self {
        let (sync_server, async_server) = crate::server::create(config);

        rt.spawn(async move {
            // todo
            async_server.listen().await;
        });

        Self(sync_server)
    }
}

fn recv<C: TransportConfig>(mut commands: Commands, mut server: ResMut<WebTransportServer<C>>) {
    loop {
        match server.0.recv.try_recv() {
            Ok(A2S::Start) => {
                info!("Started server");
            }
            Ok(A2S::Incoming { client }) => {
                info!("Client {client} connecting");
            }
            Ok(A2S::Connect { client }) => {
                info!("Client {client} connected");
            }
            Ok(A2S::Disconnect { client }) => {
                info!("Client {client} disconnected");
            }
            Ok(A2S::Error(err)) => {
                let err = anyhow::Error::new(err);
                warn!("Server transport error: {err:#}");
            }
            Ok(_) => {}
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                commands.remove_resource::<WebTransportServer<C>>();
                info!("Server closed");
                break;
            }
        }
    }
}
