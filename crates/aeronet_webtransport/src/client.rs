use {
    crate::{
        runtime::WebTransportRuntime,
        session::{
            SessionBackend, SessionError, SessionMeta, WebTransportIo, WebTransportSessionPlugin,
        },
    },
    aeronet_io::{
        DefaultPacketBuffersCapacity, DisconnectReason, Disconnected, IoSet, LocalAddr,
        PacketBuffers, PacketMtu, PacketRtt, RemoteAddr, Session,
    },
    bevy_app::prelude::*,
    bevy_ecs::{prelude::*, system::EntityCommand},
    bytes::Bytes,
    futures::{
        channel::{mpsc, oneshot},
        never::Never,
    },
    std::{net::SocketAddr, time::Duration},
    thiserror::Error,
    tracing::{debug, debug_span, Instrument},
    xwt_core::{
        endpoint::{connect::Connecting, Connect},
        prelude::*,
    },
};

cfg_if::cfg_if! {
    if #[cfg(target_family = "wasm")] {
        pub type ClientConfig = xwt_web_sys::WebTransportOptions;
    } else {
        use wtransport::endpoint::endpoint_side;

        pub type ClientConfig = wtransport::ClientConfig;
        type ClientEndpoint = xwt_wtransport::Endpoint<endpoint_side::Client>;
    }
}

type ConnectError = <ClientEndpoint as Connect>::Error;
type AwaitConnectError = <<ClientEndpoint as Connect>::Connecting as Connecting>::Error;

#[derive(Debug)]
pub struct WebTransportClientPlugin;

impl Plugin for WebTransportClientPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<WebTransportSessionPlugin>() {
            app.add_plugins(WebTransportSessionPlugin);
        }

        app.add_systems(PreUpdate, poll_frontend.before(IoSet::Poll))
            .observe(on_client_added);
    }
}

#[derive(Debug, Component)]
pub struct WebTransportClient(Frontend);

impl WebTransportClient {
    #[must_use]
    pub fn connect(config: ClientConfig, target: impl Into<String>) -> impl EntityCommand {
        let target = target.into();
        |entity: Entity, world: &mut World| connect(entity, world, config, target)
    }
}

fn connect(session: Entity, world: &mut World, config: ClientConfig, target: String) {
    let (send_dc, recv_dc) = oneshot::channel::<DisconnectReason<ClientError>>();
    let (send_next, recv_next) = oneshot::channel::<ToConnected>();
    let runtime = world.resource::<WebTransportRuntime>().clone();
    let packet_buf_cap = world
        .get::<PacketBuffers>(session)
        .map(PacketBuffers::capacity)
        .unwrap_or_else(|| **world.resource::<DefaultPacketBuffersCapacity>());

    runtime.spawn({
        let runtime = runtime.clone();
        async move {
            let Err(reason) = backend(runtime, config, target, send_next, packet_buf_cap).await
            else {
                unreachable!();
            };
            let _ = send_dc.send(reason);
        }
        .instrument(debug_span!("client", %session))
    });

    world
        .entity_mut(session)
        .insert(WebTransportClient(Frontend::Connecting {
            recv_dc,
            recv_next,
        }));
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

#[derive(Debug)]
enum Frontend {
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
    send_user_dc: oneshot::Sender<String>,
}

// TODO: required components
fn on_client_added(trigger: Trigger<OnAdd, WebTransportClient>, mut commands: Commands) {
    let session = trigger.entity();
    commands.entity(session).insert(Session);
}

fn poll_frontend(mut commands: Commands, mut query: Query<(Entity, &mut WebTransportClient)>) {
    for (session, mut client) in &mut query {
        replace_with::replace_with_or_abort(&mut client.0, |state| match state {
            Frontend::Connecting { recv_dc, recv_next } => {
                poll_connecting(&mut commands, session, recv_dc, recv_next)
            }
            Frontend::Connected { mut recv_dc } => {
                if should_disconnect(&mut commands, session, &mut recv_dc) {
                    Frontend::Disconnected
                } else {
                    Frontend::Connected { recv_dc }
                }
            }
            Frontend::Disconnected => state,
        });
    }
}

fn poll_connecting(
    commands: &mut Commands,
    session: Entity,
    mut recv_dc: oneshot::Receiver<DisconnectReason<ClientError>>,
    mut recv_next: oneshot::Receiver<ToConnected>,
) -> Frontend {
    if should_disconnect(commands, session, &mut recv_dc) {
        return Frontend::Disconnected;
    }

    let Ok(Some(next)) = recv_next.try_recv() else {
        return Frontend::Connecting { recv_dc, recv_next };
    };

    commands.entity(session).insert((
        WebTransportIo {
            recv_meta: next.recv_meta,
            recv_packet_b2f: next.recv_packet_b2f,
            send_packet_f2b: next.send_packet_f2b,
            send_user_dc: Some(next.send_user_dc),
        },
        PacketMtu(next.initial_mtu),
        #[cfg(not(target_family = "wasm"))]
        LocalAddr(next.local_addr),
        #[cfg(not(target_family = "wasm"))]
        RemoteAddr(next.initial_remote_addr),
        #[cfg(not(target_family = "wasm"))]
        PacketRtt(next.initial_rtt),
    ));
    Frontend::Connected { recv_dc }
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
    if let Some(dc_reason) = dc_reason {
        let dc_reason = dc_reason.map_err(anyhow::Error::new);
        commands.trigger_targets(Disconnected(dc_reason), session);
        true
    } else {
        false
    }
}

async fn backend(
    runtime: WebTransportRuntime,
    config: ClientConfig,
    target: String,
    send_next: oneshot::Sender<ToConnected>,
    packet_buf_cap: usize,
) -> Result<Never, DisconnectReason<ClientError>> {
    debug!("Spawning backend task to connect to {target:?}");

    let endpoint = {
        #[cfg(target_family = "wasm")]
        {
            todo!()
        }

        #[cfg(not(target_family = "wasm"))]
        {
            wtransport::Endpoint::client(config)
                .map(xwt_wtransport::Endpoint)
                .map_err(SessionError::CreateEndpoint)
                .map_err(ClientError::Session)?
        }
    };
    debug!("Created endpoint");

    let conn = endpoint
        .connect(&target)
        .await
        .map_err(|err| ClientError::Connect(err.into()))?
        .wait_connect()
        .await
        .map_err(|err| ClientError::AwaitConnect(err.into()))?;
    debug!("Connected");

    let (send_meta, recv_meta) = mpsc::channel::<SessionMeta>(1);
    let (send_packet_b2f, recv_packet_b2f) = mpsc::channel::<Bytes>(packet_buf_cap);
    let (send_packet_f2b, recv_packet_f2b) = mpsc::unbounded::<Bytes>();
    let (send_user_dc, recv_user_dc) = oneshot::channel::<String>();
    let next = ToConnected {
        #[cfg(not(target_family = "wasm"))]
        local_addr: endpoint
            .local_addr()
            .map_err(SessionError::GetLocalAddr)
            .map_err(ClientError::Session)?,
        #[cfg(not(target_family = "wasm"))]
        initial_remote_addr: conn.0.remote_address(),
        #[cfg(not(target_family = "wasm"))]
        initial_rtt: conn.0.rtt(),
        initial_mtu: conn
            .max_datagram_size()
            .ok_or(SessionError::DatagramsNotSupported.into())
            .map_err(ClientError::Session)?,
        recv_meta,
        recv_packet_b2f,
        send_packet_f2b,
        send_user_dc,
    };
    let backend = SessionBackend {
        runtime,
        conn,
        send_meta,
        send_packet_b2f,
        recv_packet_f2b,
        recv_user_dc,
    };
    send_next
        .send(next)
        .map_err(|_| SessionError::FrontendClosed)
        .map_err(ClientError::Session)?;

    debug!("Starting session loop");
    Err(backend.start().await.map_err(ClientError::Session))
}
