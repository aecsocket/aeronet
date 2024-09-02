use std::any::type_name;

use bevy_app::prelude::*;
use bevy_core::Name;
use bevy_ecs::prelude::*;
use tracing::{error, info};

use crate::transport::TransportSet;

use super::{CloseReason, ServerClosed, ServerOpened, ServerOpening};

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
                .after(TransportSet::Recv),
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

        if let Some(name) = name {
            info!("Server {server:?} ({name}) opening");
        } else {
            info!("Server {server:?} opening");
        }
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

        if let Some(name) = name {
            info!("Server {server:?} ({name}) opened");
        } else {
            info!("Server {server:?} opened");
        }
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
