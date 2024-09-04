use std::any::type_name;

use bevy_app::prelude::*;
use bevy_core::Name;
use bevy_ecs::prelude::*;
use tracing::{error, info, warn};

use crate::{session::SessionSet, util::display_name};

use super::{
    DisconnectReason, LocalClientConnected, LocalClientConnecting, LocalClientDisconnected,
};

#[derive(Debug)]
pub struct EventLogPlugin;

impl Plugin for EventLogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            (log_connecting, log_connected, log_disconnected).after(SessionSet::Recv),
        );
    }
}

fn log_connecting(
    mut connecting: EventReader<LocalClientConnecting>,
    clients: Query<Option<&Name>>,
    #[cfg(debug_assertions)] with_local_client: Query<(), With<super::LocalClient>>,
) {
    for &LocalClientConnecting { client } in connecting.read() {
        let Ok(name) = clients.get(client) else {
            error!("Client {client:?} reported as connecting but does not exist");
            continue;
        };

        #[cfg(debug_assertions)]
        {
            if with_local_client.get(client).is_err() {
                error!(
                    "Client {client:?} is missing `{}`",
                    type_name::<super::LocalClient>()
                );
            }
        }

        info!("Client {} connecting", display_name(client, name));
    }
}

fn log_connected(
    mut connected: EventReader<LocalClientConnected>,
    clients: Query<Option<&Name>>,
    #[cfg(debug_assertions)] with_local_client: Query<(), With<super::LocalClient>>,
    #[cfg(debug_assertions)] with_connected: Query<(), With<crate::session::ConnectedSession>>,
) {
    for &LocalClientConnected { client } in connected.read() {
        let Ok(name) = clients.get(client) else {
            error!("Client {client:?} reported as connected but does not exist");
            continue;
        };

        #[cfg(debug_assertions)]
        {
            if with_local_client.get(client).is_err() {
                error!(
                    "Client {client:?} is missing `{}`",
                    type_name::<super::LocalClient>()
                );
            }

            if with_connected.get(client).is_err() {
                error!(
                    "Client {client:?} is missing `{}`",
                    type_name::<crate::session::ConnectedSession>()
                );
            }
        }

        info!("Client {} connected", display_name(client, name));
    }
}

fn log_disconnected(
    mut disconnected: EventReader<LocalClientDisconnected>,
    #[cfg(debug_assertions)] clients: Query<()>,
) {
    for &LocalClientDisconnected { client, ref reason } in disconnected.read() {
        match reason {
            DisconnectReason::Local(reason) => {
                info!("Client {client:?} disconnected by user: {reason}")
            }
            DisconnectReason::Remote(reason) => {
                info!("Client {client:?} disconnected by remote: {reason}")
            }
            DisconnectReason::Error(err) => {
                warn!("Client {client:?} disconnected due to error: {err:#}")
            }
        }

        #[cfg(debug_assertions)]
        if clients.get(client).is_ok() {
            error!("Client {client:?} reported as disconnected but still exists");
        }
    }
}
