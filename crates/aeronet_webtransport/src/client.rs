use std::{net::SocketAddr, time::Duration};

use aeronet::{
    io::{IoSet, PacketBuffers, PacketMtu},
    session::DisconnectReason,
    stats::{LocalAddr, RemoteAddr, SessionStats},
};
use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
};
use thiserror::Error;
use tracing::{debug, debug_span, Instrument};
use xwt_core::{endpoint::Connect, prelude::*};

use crate::{
    runtime::WebTransportRuntime,
    session::{
        RawRtt, SessionBackend, SessionError, SessionMeta, WebTransportIo,
        WebTransportSessionPlugin, PACKET_BUF_CAP,
    },
};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub type ClientConfig = xwt_web_sys::WebTransportOptions;
    } else {
        pub type ClientConfig = wtransport::ClientConfig;
        type ClientEndpoint = xwt_wtransport::Endpoint<wtransport::endpoint::endpoint_side::Client>;
        type ConnectError = <ClientEndpoint as Connect>::Error;
        type AwaitConnectError = <<ClientEndpoint as Connect>::Connecting as xwt_core::endpoint::connect::Connecting>::Error;

        async fn create_endpoint(config: ClientConfig) -> Result<ClientEndpoint, ClientError> {
            let endpoint = wtransport::Endpoint::client(config)
                .map_err(SessionError::CreateEndpoint)?;
            Ok(xwt_wtransport::Endpoint(endpoint))
        }
    }
}

#[derive(Debug)]
pub struct WebTransportClientPlugin;

impl Plugin for WebTransportClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.add_systems(PreUpdate, update_frontend.before(IoSet::Recv));
    }
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("failed to connect")]
    Connect(#[source] ConnectError),
    #[error("failed to await connection")]
    AwaitConnect(#[source] AwaitConnectError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

pub trait ConnectWebTransportClientExt {
    fn connect_web_transport_client(
        &mut self,
        config: ClientConfig,
        target: impl Into<String>,
    ) -> Entity;
}

impl ConnectWebTransportClientExt for Commands<'_, '_> {
    fn connect_web_transport_client(
        &mut self,
        config: ClientConfig,
        target: impl Into<String>,
    ) -> Entity {
        connect_web_transport_client(self, config, target.into())
    }
}

fn connect_web_transport_client(
    this: &mut Commands,
    config: ClientConfig,
    target: String,
) -> Entity {
    let session = this.spawn_empty().id();
    this.push(move |world: &mut World| {
        world.resource_scope(|world, runtime: Mut<WebTransportRuntime>| {
            let (send_err, recv_err) = oneshot::channel::<anyhow::Error>();
            let (send_next, recv_next) = oneshot::channel::<ToConnected>();
            runtime.spawn({
                let runtime = runtime.clone();
                async move {
                    let Err(err) = backend(runtime, config, target, send_next).await else {
                        unreachable!();
                    };
                    match &err {
                        ClientError::Session(SessionError::FrontendClosed) => {
                            debug!("Disconnected due to frontend closing");
                        }
                        err => {
                            debug!("Disconnected: {err:?}");
                        }
                    }
                    let _ = send_err.send(err.into());
                }
                .instrument(debug_span!("client", ?session))
            });
            world.entity_mut(session).insert(Frontend::Connecting {
                recv_err,
                recv_next,
            });
        });
    });
    session
}

#[derive(Debug, Component)]
enum Frontend {
    Connecting {
        recv_err: oneshot::Receiver<ClientError>,
        recv_next: oneshot::Receiver<ToConnected>,
    },
    Finished,
}

#[derive(Debug)]
struct ToConnected {
    #[cfg(not(target_family = "wasm"))]
    local_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    initial_remote_addr: SocketAddr,
    #[cfg(not(target_family = "wasm"))]
    initial_rtt: Duration,
    initial_mtu: usize,
    recv_meta: mpsc::Receiver<SessionMeta>,
    recv_packet_b2f: mpsc::Receiver<Bytes>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    recv_dc: oneshot::Receiver<DisconnectReason<SessionError>>,
    send_user_dc: oneshot::Sender<String>,
}

fn update_frontend(mut commands: Commands, mut query: Query<(Entity, &mut Frontend)>) {
    for (session, mut frontend) in &mut query {
        replace_with::replace_with_or_abort(&mut *frontend, |state| match state {
            Frontend::Connecting {
                recv_err,
                recv_next,
            } => update_connecting(&mut commands, session, recv_err, recv_next),
            Frontend::Finished => state,
        });
    }
}

fn update_connecting(
    commands: &mut Commands,
    session: Entity,
    mut recv_err: oneshot::Receiver<anyhow::Error>,
    mut recv_next: oneshot::Receiver<ToConnected>,
) -> Frontend {
    let err = match recv_err.try_recv() {
        Ok(None) => None,
        Ok(Some(err)) => Some(err),
        Err(_) => Some(ClientError::Session(SessionError::BackendClosed).into()),
    };
    if let Some(err) = err {
        commands.entity(session).despawn();
        // todo send event
        return Frontend::Finished;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return Frontend::Connecting {
            recv_err,
            recv_next,
        };
    };

    commands.entity(session).insert((
        WebTransportIo {
            recv_err,
            recv_meta: next.recv_meta,
            recv_packet_b2f: next.recv_packet_b2f,
            send_packet_f2b: next.send_packet_f2b,
            recv_dc: next.recv_dc,
            send_user_dc: Some(next.send_user_dc),
        },
        PacketBuffers::default(),
        PacketMtu(next.initial_mtu),
        SessionStats::default(),
        #[cfg(not(target_family = "wasm"))]
        LocalAddr(next.local_addr),
        #[cfg(not(target_family = "wasm"))]
        RemoteAddr(next.initial_remote_addr),
        #[cfg(not(target_family = "wasm"))]
        RawRtt(next.initial_rtt),
    ));
    Frontend::Finished
}

async fn backend(
    runtime: WebTransportRuntime,
    config: ClientConfig,
    target: String,
    send_next: oneshot::Sender<ToConnected>,
) -> Result<Never, ClientError> {
    debug!("Spawning backend task to connect to {target:?}");

    let endpoint = create_endpoint(config).await?;
    debug!("Created endpoint");

    let conn = endpoint
        .connect(&target)
        .await
        .map_err(|err| ClientError::Connect(err.into()))?
        .wait_connect()
        .await
        .map_err(|err| ClientError::AwaitConnect(err.into()))?;
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
        #[cfg(not(target_family = "wasm"))]
        local_addr: endpoint.local_addr().map_err(SessionError::GetLocalAddr)?,
        #[cfg(not(target_family = "wasm"))]
        initial_remote_addr: conn.0.remote_address(),
        #[cfg(not(target_family = "wasm"))]
        initial_rtt: conn.0.rtt(),
        initial_mtu,
        recv_meta,
        recv_packet_b2f,
        send_packet_f2b,
        recv_dc: recv_dc_b2f,
        send_user_dc: send_dc_f2b,
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
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)?;

    debug!("Starting session loop");
    backend.start().await.map_err(ClientError::Session)
}
