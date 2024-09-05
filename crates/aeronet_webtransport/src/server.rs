use std::{collections::HashMap, net::SocketAddr, time::Duration};

use aeronet::{io::IoSet, session::DisconnectReason};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
    SinkExt,
};
use thiserror::Error;
use tracing::{debug, debug_span, Instrument};
use wtransport::{
    endpoint::{IncomingSession, SessionRequest},
    error::ConnectionError,
    Endpoint, ServerConfig,
};
use xwt_core::prelude::*;

use crate::{
    runtime::WebTransportRuntime,
    session::{
        SessionBackend, SessionError, SessionMeta, WebTransportSessionPlugin, PACKET_BUF_CAP,
    },
};

#[derive(Debug)]
pub struct WebTransportServerPlugin;

impl Plugin for WebTransportServerPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.add_systems(PreUpdate, update_frontend.before(IoSet::Recv));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionResponse {
    Accepted,
    Forbidden,
    NotFound,
}

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("failed to await session request")]
    AwaitSessionRequest(#[source] ConnectionError),
    #[error("frontend rejected session")]
    Rejected,
    #[error("failed to accept session")]
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
    recv_connected: oneshot::Receiver<ToConnected>,
}

#[derive(Debug)]
struct ToConnected {
    initial_remote_addr: SocketAddr,
    initial_rtt: Duration,
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<SessionMeta>,
    recv_packet_b2f: mpsc::Receiver<Bytes>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    recv_dc_b2f: oneshot::Receiver<DisconnectReason>,
    send_dc_f2b: oneshot::Sender<String>,
}

fn update_frontend(mut commands: Commands, mut query: Query<Entity>) {
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
        runtime.spawn({
            let runtime = runtime.clone();
            let session = endpoint.accept().await;
            let send_connecting = send_connecting.clone();
            async move {
                if let Err(err) = accept_session(runtime, session, send_connecting).await {
                    debug!("Failed to accept session: {err:?}");
                };
            }
        });
    }
}

async fn accept_session(
    runtime: WebTransportRuntime,
    session: IncomingSession,
    mut send_connecting: mpsc::Sender<ToConnecting>,
) -> Result<(), ServerError> {
    let request = session.await.map_err(ServerError::AwaitSessionRequest)?;

    let (send_session_entity, recv_session_entity) = oneshot::channel::<Entity>();
    let (send_conn_response, recv_conn_response) = oneshot::channel::<ConnectionResponse>();
    let (send_connected, recv_connected) = oneshot::channel::<ToConnected>();
    send_connecting
        .send(ToConnecting {
            authority: request.authority().to_owned(),
            path: request.path().to_owned(),
            origin: request.origin().map(ToOwned::to_owned),
            user_agent: request.user_agent().map(ToOwned::to_owned),
            headers: request.headers().clone(),
            send_session_entity,
            send_conn_response,
            recv_connected,
        })
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    let session_entity = recv_session_entity
        .await
        .map_err(|_| SessionError::FrontendClosed)?;

    let err = async move {
        let Err(err) = handle_session(runtime, request, recv_conn_response, send_connected).await
        else {
            unreachable!()
        };
        match &err {
            ServerError::FrontendClosed => {
                debug!("Session closed");
            }
            err => {
                debug!("Session closed: {:#}", pretty_error(err));
            }
        }
        err
    }
    .instrument(debug_span!("session", session = ?session_entity))
    .await;
    Ok(())
}

async fn handle_session(
    runtime: WebTransportRuntime,
    request: SessionRequest,
    recv_conn_response: oneshot::Receiver<ConnectionResponse>,
    send_connected: oneshot::Sender<ToConnected>,
) -> Result<Never, ServerError> {
    debug!(
        "New session request from {}{}",
        request.authority(),
        request.path()
    );

    let conn_response = recv_conn_response
        .await
        .map_err(|_| SessionError::FrontendClosed)?;
    debug!("Frontend responded to this request with {conn_response:?}");

    let conn = match conn_response {
        ConnectionResponse::Accepted => request.accept(),
        ConnectionResponse::Forbidden => {
            request.forbidden().await;
            return Err(ServerError::Rejected.into());
        }
        ConnectionResponse::NotFound => {
            request.not_found().await;
            return Err(ServerError::Rejected.into());
        }
    }
    .await
    .map(xwt_wtransport::Connection)
    .map_err(ServerError::AcceptSessionRequest)?;
    let initial_mtu = conn
        .max_datagram_size()
        .ok_or(SessionError::DatagramsNotSupported)?;
    debug!("Connected");

    let (send_meta, recv_meta) = mpsc::channel::<SessionMeta>(1);
    let (send_packet_b2f, recv_packet_b2f) = mpsc::channel::<Bytes>(PACKET_BUF_CAP);
    let (send_packet_f2b, recv_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (send_dc_b2f, recv_dc_b2f) = oneshot::channel::<DisconnectReason>();
    let (send_dc_f2b, recv_dc_f2b) = oneshot::channel::<String>();
    let next = ToConnected {
        initial_remote_addr: conn.0.remote_address(),
        initial_rtt: conn.0.rtt(),
        initial_mtu,
        recv_meta,
        recv_packet_b2f,
        send_packet_f2b,
        recv_dc_b2f,
        send_dc_f2b,
    };
    let backend = SessionBackend {
        runtime,
        conn,
        send_meta,
        send_packet_b2f,
        recv_packet_f2b,
        send_dc: send_dc_b2f,
        recv_user_dc: recv_dc_f2b,
    };
    send_connected
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting session loop");
    backend.start().await.map_err(ServerError::Session)
}
