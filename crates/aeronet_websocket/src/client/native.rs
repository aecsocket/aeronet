use std::net::SocketAddr;

use crate::{
    session::{native_backend::SessionBackend, SessionError, WebSocketIo, WebSocketSessionPlugin},
    tungstenite,
};
use aeronet_io::{
    connection::{DisconnectReason, Disconnected, LocalAddr, RemoteAddr, Session},
    packet::PacketMtu,
    IoSet,
};
use bevy_app::prelude::*;
use bevy_ecs::{prelude::*, system::EntityCommand};
use bytes::Bytes;
use futures::{
    channel::{mpsc, oneshot},
    never::Never,
};
use tokio_tungstenite::{
    tungstenite::{
        client::IntoClientRequest, handshake::client::Request, protocol::WebSocketConfig,
    },
    Connector, MaybeTlsStream,
};
use tracing::{debug, debug_span, Instrument};

use crate::WebSocketRuntime;

use super::ClientError;

#[derive(Debug)]
pub struct WebSocketClientPlugin;

impl Plugin for WebSocketClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebSocketSessionPlugin>() {
            app.add_plugins(WebSocketSessionPlugin);
        }

        app.add_systems(PreUpdate, poll_clients.before(IoSet::Poll))
            .observe(on_client_added);
    }
}

#[derive(Debug, Component)]
pub struct WebSocketClient(ClientFrontend);

impl WebSocketClient {
    #[must_use]
    pub fn connect(config: ClientConfig, target: impl IntoClientRequest) -> impl EntityCommand {
        let target = target.into_client_request();
        move |session: Entity, world: &mut World| connect(session, world, config, target)
    }
}

#[derive(Clone)]
pub struct ClientConfig {
    pub socket: WebSocketConfig,
    pub nagle: bool,
    pub connector: Connector,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            socket: WebSocketConfig::default(),
            nagle: true,
            connector: Connector::Plain,
        }
    }
}

fn connect(
    session: Entity,
    world: &mut World,
    config: ClientConfig,
    target: Result<Request, tungstenite::Error>,
) {
    let runtime = world.resource::<WebSocketRuntime>().clone();
    let packet_mtu = config.socket.max_message_size.unwrap_or_else(|| {
        WebSocketConfig::default()
            .max_message_size
            .expect("default impl has a value set")
    });

    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ClientError>>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
    runtime.spawn({
        let runtime = runtime.clone();
        async move {
            let Err(reason) = start(runtime, 0, config, target, send_next).await else {
                unreachable!();
            };
            let _ = send_dc.send(reason);
        }
        .instrument(debug_span!("client", %session))
    });

    world.entity_mut(session).insert((
        WebSocketClient(ClientFrontend::Connecting { recv_dc, recv_next }),
        PacketMtu(packet_mtu),
    ));
}

#[derive(Debug)]
enum ClientFrontend {
    Connecting {
        recv_dc: oneshot::Receiver<DisconnectReason<ClientError>>,
        recv_next: oneshot::Receiver<ToConnected>,
    },
    Connected {
        recv_dc: oneshot::Receiver<DisconnectReason<ClientError>>,
    },
    Disconnected,
}

#[derive(Debug)]
struct ToConnected {
    local_addr: SocketAddr,
    remote_addr: SocketAddr,
    recv_packet_b2f: mpsc::Receiver<Bytes>,
    send_packet_f2b: mpsc::UnboundedSender<Bytes>,
    send_user_dc: oneshot::Sender<String>,
}

// TODO: required components
fn on_client_added(trigger: Trigger<OnAdd, WebSocketClient>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert(Session);
}

fn poll_clients(mut commands: Commands, mut frontends: Query<(Entity, &mut WebSocketClient)>) {
    for (session, mut frontend) in &mut frontends {
        replace_with::replace_with_or_abort(&mut frontend.0, |state| match state {
            ClientFrontend::Connecting { recv_dc, recv_next } => {
                poll_connecting(&mut commands, session, recv_dc, recv_next)
            }
            ClientFrontend::Connected { mut recv_dc } => {
                if should_disconnect(&mut commands, session, &mut recv_dc) {
                    ClientFrontend::Disconnected
                } else {
                    ClientFrontend::Connected { recv_dc }
                }
            }
            ClientFrontend::Disconnected => state,
        });
    }
}

fn poll_connecting(
    commands: &mut Commands,
    session: Entity,
    mut recv_dc: oneshot::Receiver<DisconnectReason<ClientError>>,
    mut recv_next: oneshot::Receiver<ToConnected>,
) -> ClientFrontend {
    if should_disconnect(commands, session, &mut recv_dc) {
        return ClientFrontend::Disconnected;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return ClientFrontend::Connecting { recv_dc, recv_next };
    };

    commands.entity(session).insert((
        WebSocketIo {
            recv_packet_b2f: next.recv_packet_b2f,
            send_packet_f2b: next.send_packet_f2b,
            send_user_dc: Some(next.send_user_dc),
        },
        LocalAddr(next.local_addr),
        RemoteAddr(next.remote_addr),
    ));
    ClientFrontend::Connected { recv_dc }
}

fn should_disconnect(
    commands: &mut Commands,
    session: Entity,
    recv_dc: &mut oneshot::Receiver<DisconnectReason<ClientError>>,
) -> bool {
    let dc_reason = match recv_dc.try_recv() {
        Ok(None) => None,
        Ok(Some(dc_reason)) => Some(dc_reason),
        Err(_) => Some(ClientError::Session(SessionError::BackendClosed).into()),
    };
    dc_reason.map_or(false, |reason| {
        let reason = reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Disconnected { reason }, session);
        true
    })
}

async fn start(
    runtime: WebSocketRuntime,
    packet_buf_cap: usize,
    config: ClientConfig,
    target: Result<Request, tungstenite::Error>,
    send_next: oneshot::Sender<ToConnected>,
) -> Result<Never, DisconnectReason<ClientError>> {
    let target = target.map_err(ClientError::IntoRequest)?;
    debug!("Spawning backend task to connect to {:?}", target.uri());

    let (stream, _) = {
        let socket_config = Some(config.socket);
        let disable_nagle = !config.nagle;

        #[cfg(feature = "__tls")]
        {
            tokio_tungstenite::connect_async_tls_with_config(
                target,
                socket_config,
                disable_nagle,
                Some(config.connector),
            )
        }

        #[cfg(not(feature = "__tls"))]
        {
            tokio_tungstenite::connect_async_with_config(target, socket_config, disable_nagle)
        }
    }
    .await
    .map_err(ClientError::Connect)?;
    debug!("Created stream");

    let socket = match stream.get_ref() {
        MaybeTlsStream::Plain(stream) => stream,
        #[cfg(feature = "native-tls")]
        MaybeTlsStream::NativeTls(stream) => stream.get_ref().get_ref().get_ref(),
        #[cfg(feature = "__rustls-tls")]
        MaybeTlsStream::Rustls(stream) => stream.get_ref().0,
        _ => unreachable!("should only be one of these variants"),
    };
    let local_addr = socket
        .local_addr()
        .map_err(SessionError::GetLocalAddr)
        .map_err(ClientError::Session)?;
    let remote_addr = socket
        .peer_addr()
        .map_err(SessionError::GetRemoteAddr)
        .map_err(ClientError::Session)?;

    let (send_packet_b2f, recv_packet_b2f) = mpsc::channel::<Bytes>(packet_buf_cap);
    let (send_packet_f2b, recv_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (send_user_dc, recv_user_dc) = oneshot::channel::<String>();
    let next = ToConnected {
        local_addr,
        remote_addr,
        recv_packet_b2f,
        send_packet_f2b,
        send_user_dc,
    };
    let backend = SessionBackend {
        stream,
        send_packet_b2f,
        recv_packet_f2b,
        recv_user_dc,
    };
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ClientError::Session)?;

    debug!("Starting session loop");
    backend
        .start()
        .await
        .map_err(|reason| reason.map_err(ClientError::Session))
}
