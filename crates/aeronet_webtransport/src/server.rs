use std::{collections::HashMap, net::SocketAddr, time::Duration};

use aeronet::session::SessionSet;
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytes::Bytes;
use futures::{channel::mpsc, never::Never, SinkExt};
use thiserror::Error;
use tokio::sync::oneshot;
use tracing::{debug, debug_span, Instrument};
use wtransport::{endpoint::IncomingSession, error::ConnectionError, Endpoint, ServerConfig};

use crate::{
    runtime::WebTransportRuntime,
    session::{SessionError, SessionMeta, WebTransportSessionPlugin},
};

#[derive(Debug)]
pub struct WebTransportServerPlugin;

impl Plugin for WebTransportServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.add_systems(PreUpdate, frontend.before(SessionSet::Recv));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    /// Allow the client to connect.
    Accepted,
    /// 403 Forbidden.
    Forbidden,
    /// 404 Not Found.
    NotFound,
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[from("failed to accept session")]
    AcceptSessionRequest(#[source] ConnectionError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

pub trait OpenWebTransportServerExt {
    fn open_web_transport_server(&mut self, config: ServerConfig) -> Entity;
}

impl OpenWebTransportServerExt for Commands<'_, '_> {
    fn open_web_transport_server(&mut self, config: ServerConfig) -> Entity {
        let server = self.spawn_empty().id();
        self.push(move |world: &mut World| {
            world.resource_scope(|world, runtime: Mut<WebTransportRuntime>| {
                let (send_err, recv_err) = oneshot::channel::<anyhow::Error>();
                let (send_next, recv_next) = oneshot::channel::<ToOpen>();
                runtime.spawn({
                    let runtime = runtime.clone();
                    async move {
                        let Err(err) = backend(runtime, config, send_next).await else {
                            unreachable!();
                        };
                        match &err {
                            ServerError::Session(SessionError::FrontendClosed) => {
                                debug!("Closing due to frontend closing");
                            }
                            err => {
                                debug!("Closing: {err:?}");
                            }
                        }
                        let _ = send_err.send(err.into());
                    }
                    .instrument(debug_span!("server", ?server))
                });
                world.entity_mut(server).insert(Frontend::Opening {
                    recv_err,
                    recv_next,
                });
            });
        });
        server
    }
}

#[derive(Debug, Component)]
enum Frontend {
    Opening {
        recv_err: oneshot::Receiver<anyhow::Error>,
        recv_next: oneshot::Receiver<ToOpen>,
    },
    Finished,
}

#[derive(Debug)]
struct ToOpen {
    local_addr: SocketAddr,
    recv_connecting: mpsc::Receiver<ToConnecting>,
}

#[derive(Debug)]
struct ToConnecting {
    authority: String,
    path: String,
    origin: Option<String>,
    user_agent: Option<String>,
    headers: HashMap<String, String>,
    send_session_entity: oneshot::Sender<Entity>,
    send_conn_response: oneshot::Sender<ConnectionResponse>,
    send_conn_resp: oneshot::Sender<ConnectionResponse>,
    recv_connected: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    initial_remote_addr: SocketAddr,
    initial_rtt: Duration,
    recv_meta: mpsc::Receiver<SessionMeta>,
}

fn frontend(mut commands: Commands, mut query: Query<Entity>) {
    for session in &mut query {}
}

async fn backend(
    runtime: WebTransportRuntime,
    config: ServerConfig,
    send_next: oneshot::Sender<ToOpen>,
) -> Result<Never, ServerError> {
    debug!("Spawning backend task to open server");

    let endpoint = Endpoint::server(config).map_err(SessionError::CreateEndpoint)?;
    debug!("Created endpoint");

    let (send_connecting, recv_connecting) = mpsc::channel(1);

    let next = ToOpen {
        local_addr: endpoint.local_addr().map_err(SessionError::GetLocalAddr)?,
        recv_connecting,
    };
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting server loop");
    loop {
        let session = endpoint.accept().await;
        let send_connecting = send_connecting.clone();
        runtime.spawn(async move {
            if let Err(err) = accept_session(session, send_connecting).await {
                debug!("Failed to accept session: {err:?}");
            };
        });
    }
}

async fn accept_session(
    session: IncomingSession,
    mut send_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), ServerError> {
    let request = session.await.map_err(ServerError::AcceptSessionRequest)?;

    let (send_session_entity, recv_session_entity) = oneshot::channel::<Entity>();
    let (send_conn_response, recv_conn_response) = oneshot::channel::<ConnectionResponse>();
    let (send_connected, recv_connected) = send_connecting
        .send(ToConnecting {
            authority: request.authority().to_owned(),
            path: request.path().to_owned(),
            origin: request.origin().map(ToOwned::to_owned),
            user_agent: request.user_agent().map(ToOwned::to_owned),
            headers: request.headers().clone(),
            send_session_entity,
            send_conn_response,
            recv_connected: (),
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    let session_entity = recv_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)?;

    let err = async move {}
        .instrument(debug_span!("session", session = ?session_entity))
        .await;
    Ok(())
}
