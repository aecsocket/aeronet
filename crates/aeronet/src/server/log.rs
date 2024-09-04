use std::any::type_name;

use bevy_app::prelude::*;
use bevy_core::Name;
use bevy_ecs::prelude::*;
use tracing::{error, info};

use crate::{session::SessionSet, util::display_name};

use super::{
    CloseReason, RemoteClient, RemoteClientConnecting, ServerClosed, ServerOpened, ServerOpening,
};

#[derive(Debug)]
pub struct EventLogPlugin;

impl Plugin for EventLogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            (
                (log_opening, log_opened, log_closed),
                // (log_connecting, log_connected, log_disconnected),
            )
                .after(SessionSet::Recv),
        );
    }
}

fn log_opening(
    mut opening: EventReader<ServerOpening>,
    servers: Query<Option<&Name>>,
    #[cfg(debug_assertions)] with_server: Query<(), With<super::Server>>,
) {
    for &ServerOpening { server } in opening.read() {
        let Ok(name) = servers.get(server) else {
            error!("Server {server:?} reported as opening but does not exist");
            continue;
        };

        #[cfg(debug_assertions)]
        {
            if with_server.get(server).is_err() {
                error!(
                    "Server {server:?} is missing `{}`",
                    type_name::<super::Server>()
                );
            }
        }

        info!("Server {} opening", display_name(server, name));
    }
}

fn log_opened(
    mut opened: EventReader<ServerOpened>,
    servers: Query<Option<&Name>>,
    #[cfg(debug_assertions)] with_server: Query<(), With<super::Server>>,
    #[cfg(debug_assertions)] with_open: Query<(), With<super::Open>>,
) {
    for &ServerOpened { server } in opened.read() {
        let Ok(name) = servers.get(server) else {
            error!("Server {server:?} reported as opened but does not exist");
            continue;
        };

        #[cfg(debug_assertions)]
        {
            if with_server.get(server).is_err() {
                error!(
                    "Server {server:?} is missing `{}`",
                    type_name::<super::Server>()
                );
            }

            if with_open.get(server).is_err() {
                error!(
                    "Server {server:?} is missing `{}`",
                    type_name::<super::Open>()
                );
            }
        }

        info!("Server {} opened", display_name(server, name));
    }
}

fn log_closed(mut closed: EventReader<ServerClosed>, #[cfg(debug_assertions)] servers: Query<()>) {
    for &ServerClosed { server, ref reason } in closed.read() {
        match reason {
            CloseReason::Local(reason) => {
                info!("Server {server:?} closed by user: {reason}");
            }
            CloseReason::Error(err) => {
                info!("Server {server:?} closed due to error: {err:#}");
            }
        }

        #[cfg(debug_assertions)]
        if servers.get(server).is_ok() {
            error!("Server {server:?} reported as closed but still exists");
        }
    }
}

fn log_connecting(
    mut connecting: EventReader<RemoteClientConnecting>,
    clients: Query<(&RemoteClient, Option<&Name>)>,
    servers: Query<Option<&Name>>,
    #[cfg(debug_assertions)] with_server: Query<(), With<super::Server>>,
) {
    for &RemoteClientConnecting { client } in connecting.read() {
        let Ok((remote_client, client_name)) = clients.get(client) else {
            error!("Client {client:?} reported as connecting but does not exist");
            continue;
        };

        let server = remote_client.server();
        let server_name = servers.get(server).unwrap_or_else(|_| {
            panic!(
                "target of `{}` should always exist",
                type_name::<RemoteClient>()
            );
        });

        #[cfg(debug_assertions)]
        {
            if with_server.get(server).is_err() {
                error!(
                    "Server {server:?} is missing `{}`",
                    type_name::<super::Server>()
                );
            }
        }

        info!(
            "Client {} connecting to server {}",
            display_name(client, client_name),
            display_name(server, server_name)
        );
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

        if let Some(name) = name {
            info!("Client {client:?} ({name}) connected");
        } else {
            info!("Client {client:?} connected");
        }
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
